//! Framing, waiting and retrying, written once for every packet transport.
//!
//! [`Ddc`] is the adapter that turns a [`DdcTransport`] — bytes to an address
//! and bytes back — into a [`VcpBackend`]. Linux and macOS both go through it.
//! Windows does not, because `dxva2` does all of this itself.
//!
//! Everything here exists because of one property of DDC/CI: **it has no
//! sequence numbers.** Nothing in a reply says which request it belongs to
//! except the feature code the monitor echoes back, and a reply read too early
//! is the answer to the previous question. Left unchecked that does not look
//! like a bug, it looks like a monitor with strange values: brightness read
//! from a contrast answer, every subsequent reading shifted by one, and
//! nothing reporting an error. So the echo is checked (in `roadie-ddc`, on
//! every parse) and the clock is respected (here).

use std::thread::sleep;
use std::time::{Duration, Instant};

use roadie_ddc::packet::{MAX_REPLY, Reply, Request};
use roadie_ddc::{Capabilities, Feature, Value};

use crate::backend::{DdcTransport, DisplayError, VcpBackend};

/// How long to leave between messages, and between a request and its reply.
///
/// The defaults are the DDC/CI specification's minimums. They are **floors,
/// not targets**: panels that want considerably more exist, and the failure
/// mode of rushing one is a garbled or mismatched reply rather than a clean
/// error, which is exactly the failure this crate is least able to distinguish
/// from a monitor that does not speak DDC at all. Hence the type — a panel
/// that needs more can be given more without touching this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pacing {
    /// Between a request going out and its reply being read.
    pub reply: Duration,
    /// Between one message finishing and the next going out.
    pub between: Duration,
    /// After a write, before anything else is sent.
    ///
    /// Longer than [`Self::between`] in the specification, because the monitor
    /// is acting on the write rather than merely having heard it.
    pub after_write: Duration,
}

impl Pacing {
    /// The specification's minimums: 40 ms to a reply, 50 ms between messages,
    /// 50 ms after a write.
    pub const SPECIFIED: Self = Self {
        reply: Duration::from_millis(40),
        between: Duration::from_millis(50),
        after_write: Duration::from_millis(50),
    };

    /// No waiting at all.
    ///
    /// For a transport that paces itself, and for tests of the exchange logic
    /// that would otherwise spend their whole runtime asleep. Never for a real
    /// I²C bus.
    pub const NONE: Self = Self {
        reply: Duration::ZERO,
        between: Duration::ZERO,
        after_write: Duration::ZERO,
    };
}

impl Default for Pacing {
    fn default() -> Self {
        Self::SPECIFIED
    }
}

/// Tracks when the bus was last used, so a gap already spent is not spent
/// again.
///
/// Pure and clock-injected: every method takes `now` rather than reading it,
/// which is what lets the waiting policy be tested with literal durations
/// instead of by sleeping and hoping.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Pacer {
    /// When the last message finished, and how long must pass after it.
    owed: Option<(Instant, Duration)>,
}

impl Pacer {
    /// How long to wait, at `now`, before the next message may go out.
    ///
    /// Zero once the gap has already elapsed — which it usually has, since a
    /// caller reading four features does other work between them.
    pub(crate) fn wait_at(self, now: Instant) -> Duration {
        let Some((finished, gap)) = self.owed else {
            return Duration::ZERO;
        };
        gap.saturating_sub(now.saturating_duration_since(finished))
    }

    /// Record that a message finished at `now` and owes `gap` before the next.
    pub(crate) fn finished(&mut self, now: Instant, gap: Duration) {
        self.owed = Some((now, gap));
    }
}

/// What to do about a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    /// Ask again. The fault is one a second try genuinely fixes.
    Again,
    /// Stop. Asking again would produce the same answer.
    Stop,
}

/// Whether `error` is worth another attempt.
///
/// The split is between *the monitor said something* and *the monitor was not
/// ready*. A monitor that reports a feature unsupported has answered, and
/// answered the same way it will answer next time. A null message, a
/// mismatched echo, or a bad checksum are all the same underlying event — the
/// bus was read at the wrong moment — and that is what retrying is for.
fn retry_for(error: &DisplayError) -> Retry {
    use roadie_ddc::packet::ProtocolError as P;

    let source = match error {
        DisplayError::Protocol { source, .. } => source,
        // The backend already judged this one: a busy bus or an
        // unacknowledged address is a moment, a permission error is forever.
        DisplayError::Transport { retryable, .. } => {
            return if *retryable {
                Retry::Again
            } else {
                Retry::Stop
            };
        }
        _ => return Retry::Stop,
    };
    match source {
        P::Null
        | P::Checksum { .. }
        | P::WrongFeature { .. }
        | P::WrongOffset { .. }
        | P::TooShort { .. }
        | P::Truncated { .. }
        | P::NotFromDisplay { .. }
        | P::MalformedLength { .. }
        | P::UnexpectedOpcode { .. } => Retry::Again,
        P::Unsupported { .. } | P::Failed { .. } | P::NotAnswered => Retry::Stop,
    }
}

/// How many bytes a reply to `request` can occupy.
///
/// Reads are sized to the answer rather than to the buffer. A monitor clocks
/// out exactly as many bytes as it is asked for, and asking for a hundred and
/// thirty when the answer is eleven makes some panels return padding and
/// others give up on the transfer altogether.
fn reply_len(request: Request) -> usize {
    match request {
        // Source, length, checksum, around an eight-byte payload: opcode,
        // result, the echoed feature, the type byte, then maximum and current
        // as sixteen-bit values.
        Request::Get(_) => 11,
        // The same framing around an opcode, a two-byte offset, and a
        // fragment the specification caps at thirty-two bytes.
        Request::Capabilities { .. } => 38,
        // Neither is answered; nothing is read after them.
        Request::Set { .. } | Request::SaveSettings => 0,
    }
}

/// How many times to ask before calling a display silent.
///
/// Three, because the fault this retries is a timing race and a race lost
/// three times running is not a race any more.
const ATTEMPTS: u8 = 3;

/// The most capability-string bytes to accept from one monitor.
///
/// A terminating fragment is an empty one, so a monitor that never sends an
/// empty fragment never ends the loop. Real strings run to a few hundred bytes
/// and the longest seen in the wild is under two thousand, so this is generous
/// by an order of magnitude and still bounded.
const MAX_CAPABILITIES: usize = 8192;

/// A [`VcpBackend`] built from a raw packet transport.
#[derive(Debug)]
pub struct Ddc<T: DdcTransport> {
    transport: T,
    pacing: Pacing,
    pacer: Pacer,
}

impl<T: DdcTransport> Ddc<T> {
    /// Wrap `transport`, waiting the specification's minimums.
    pub fn new(transport: T) -> Self {
        Self::with_pacing(transport, Pacing::SPECIFIED)
    }

    /// Wrap `transport` with a chosen [`Pacing`].
    pub fn with_pacing(transport: T, pacing: Pacing) -> Self {
        Self {
            transport,
            pacing,
            pacer: Pacer::default(),
        }
    }

    /// The transport underneath.
    ///
    /// Public because the mock panel is public: a caller driving the CLI with
    /// no hardware wants to see what went on the wire, and a caller driving a
    /// real bus has nothing interesting to look at here anyway.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The transport underneath, mutably, for scripting a mock mid-run.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Wait out whatever the previous message still owes.
    fn settle(&mut self) {
        let wait = self.pacer.wait_at(Instant::now());
        if !wait.is_zero() {
            sleep(wait);
        }
    }

    /// Send `request` and read one reply, exactly once.
    fn exchange<'buffer>(
        &mut self,
        request: Request,
        buffer: &'buffer mut [u8; MAX_REPLY],
    ) -> Result<Reply<'buffer>, DisplayError> {
        self.settle();
        self.transport.send(&request.frame())?;

        // The reply gap is owed from the moment the request lands, not from
        // the last message, so it is waited out directly rather than recorded.
        if !self.pacing.reply.is_zero() {
            sleep(self.pacing.reply);
        }

        let read = self.transport.receive(&mut buffer[..reply_len(request)]);
        self.pacer.finished(Instant::now(), self.pacing.between);
        let read = read?;

        request
            .parse_reply(&buffer[..read])
            .map_err(|source| DisplayError::Protocol {
                name: self.transport.name(),
                source,
            })
    }

    /// Send `request` and read one reply, retrying the faults that are worth
    /// retrying.
    fn ask(&mut self, request: Request) -> Result<OwnedReply, DisplayError> {
        let mut last = None;
        for _ in 0..ATTEMPTS {
            let mut buffer = [0_u8; MAX_REPLY];
            match self.exchange(request, &mut buffer) {
                Ok(reply) => return Ok(OwnedReply::from(reply)),
                Err(error) => {
                    if retry_for(&error) == Retry::Stop {
                        return Err(error);
                    }
                    tracing::debug!(%error, "retrying a DDC exchange");
                    last = Some(error);
                }
            }
        }
        Err(match last {
            Some(last) => DisplayError::Silent {
                name: self.transport.name(),
                attempts: ATTEMPTS,
                last: Box::new(last),
            },
            // Unreachable while ATTEMPTS is non-zero: the loop either returns
            // or records an error on every pass. Written as a value rather
            // than a panic so that changing ATTEMPTS to zero is a silent
            // no-op instead of a crash on real hardware.
            None => DisplayError::Silent {
                name: self.transport.name(),
                attempts: 0,
                last: Box::new(DisplayError::Unsupported {
                    platform: std::env::consts::OS,
                }),
            },
        })
    }

    /// Send a request the monitor does not answer.
    fn tell(&mut self, request: Request, gap: Duration) -> Result<(), DisplayError> {
        self.settle();
        self.transport.send(&request.frame())?;
        self.pacer.finished(Instant::now(), gap);
        Ok(())
    }
}

/// A reply, detached from the buffer it was parsed out of.
///
/// [`Reply`] borrows the read buffer, which cannot outlive one attempt of a
/// retry loop. Only the capability fragment is actually owned data, and it is
/// at most 32 bytes.
enum OwnedReply {
    /// A reading. The feature is not carried: `parse_reply` has already
    /// checked the monitor's echo against the request, so keeping the code
    /// here would only invite a second, weaker check of the same thing.
    Feature(Value),
    Capabilities(Vec<u8>),
}

impl From<Reply<'_>> for OwnedReply {
    fn from(reply: Reply<'_>) -> Self {
        match reply {
            Reply::Feature { value, .. } => Self::Feature(value),
            Reply::Capabilities { fragment, .. } => Self::Capabilities(fragment.to_vec()),
        }
    }
}

impl<T: DdcTransport> VcpBackend for Ddc<T> {
    fn name(&self) -> String {
        self.transport.name()
    }

    fn get(&mut self, feature: Feature) -> Result<Value, DisplayError> {
        match self.ask(Request::Get(feature))? {
            OwnedReply::Feature(value) => Ok(value),
            // `Request::Get` only ever parses into `Reply::Feature`; anything
            // else is a protocol fault `parse_reply` has already rejected.
            OwnedReply::Capabilities(_) => Err(DisplayError::Protocol {
                name: self.transport.name(),
                source: roadie_ddc::packet::ProtocolError::UnexpectedOpcode { opcode: 0xE3 },
            }),
        }
    }

    fn set(&mut self, feature: Feature, value: u16) -> Result<(), DisplayError> {
        self.tell(Request::Set { feature, value }, self.pacing.after_write)
    }

    fn capabilities(&mut self) -> Result<Capabilities, DisplayError> {
        let mut text = Vec::new();
        let mut truncated = false;
        loop {
            let offset = u16::try_from(text.len()).unwrap_or(u16::MAX);
            let OwnedReply::Capabilities(fragment) = self.ask(Request::Capabilities { offset })?
            else {
                // As above: a capability request cannot parse into a feature
                // reading. Ending the string is the honest response to an
                // impossible one.
                break;
            };
            if fragment.is_empty() {
                break;
            }
            if text.len() + fragment.len() > MAX_CAPABILITIES {
                truncated = true;
                break;
            }
            text.extend_from_slice(&fragment);
        }

        let mut capabilities =
            Capabilities::parse(&text).map_err(|source| DisplayError::Capabilities {
                name: self.transport.name(),
                source,
            })?;
        if truncated {
            capabilities.warnings.push(format!(
                "the capability string was still going after {MAX_CAPABILITIES} bytes, \
                 so the rest of it was not read"
            ));
        }
        Ok(capabilities)
    }

    fn save_settings(&mut self) -> Result<(), DisplayError> {
        self.tell(Request::SaveSettings, self.pacing.after_write)
    }
}

#[cfg(test)]
mod tests;
