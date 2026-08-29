//! A monitor made of software, for the days there is no monitor.
//!
//! [`Panel`] is a [`DdcTransport`], not a [`VcpBackend`](crate::VcpBackend),
//! and that choice is the point. A double that answered at the VCP level would
//! let the CLI and the MCP tools be driven with no hardware while proving
//! nothing about the layer that actually breaks: framing, checksums, the
//! echoed feature code, the capability fragment loop, the retry policy. A
//! double that answers *packets* exercises all of it, and leaves only the wire
//! itself untested — which is the one part no software double could cover
//! anyway.
//!
//! The reply framing here is written out from the DDC/CI specification rather
//! than built with `roadie-ddc`'s own helpers, deliberately. A double that
//! encodes with the code under test agrees with it by construction, including
//! where both are wrong. The literal `0x6E`, `0x80`, `0x50` and `0x02` below
//! are the specification's numbers, and if `roadie-ddc` ever stops agreeing
//! with them these tests are what says so.
//!
//! It is `pub`, not `#[cfg(test)]`: the CLI and the MCP server want it too, so
//! that `roadie display` can be driven end to end on a machine with no panel
//! attached, the way `roadie-agent-mock` serves the GUI.

use std::collections::{BTreeMap, VecDeque};

use roadie_ddc::packet::Frame;

use crate::backend::{DdcTransport, DisplayError};

/// The display's source address, the first byte of every reply.
const DISPLAY_ADDRESS: u8 = 0x6E;
/// The high bit every length byte carries.
const LENGTH_FLAG: u8 = 0x80;
/// The "virtual host address" a reply's checksum is seeded with. Not the
/// host's `0x51` and not the display's `0x6E`: a third number the
/// specification names for this one purpose.
const VIRTUAL_HOST_ADDRESS: u8 = 0x50;
/// Host opcode: read a feature.
const OP_GET: u8 = 0x01;
/// Host opcode: write a feature.
const OP_SET: u8 = 0x03;
/// Host opcode: commit settings to the monitor's memory.
const OP_SAVE: u8 = 0x0C;
/// Host opcode: read part of the capability string.
const OP_CAPABILITIES: u8 = 0xF3;
/// Monitor opcode: a feature reading.
const OP_GET_REPLY: u8 = 0x02;
/// Monitor opcode: a capability fragment.
const OP_CAPABILITIES_REPLY: u8 = 0xE3;
/// The most capability bytes one fragment carries.
const FRAGMENT: usize = 32;

/// Something to make a panel do instead of answering properly.
///
/// Each fault is consumed by one exchange, so a queue of them scripts a
/// sequence: two hesitations and then an answer, say, which is what a real
/// monitor waking from standby does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Send a null message: the reply a monitor sends when it is not ready.
    NotReady,
    /// Send the *previous* reply again.
    ///
    /// This is the hazard the whole protocol layer is built around. DDC has no
    /// sequence numbers, so a reply read too early is the answer to the last
    /// question, and every reading after it is shifted by one. Reproducing it
    /// here is how the echo check gets tested rather than merely written.
    Stale,
    /// Send a well-formed reply with the wrong checksum.
    BadChecksum,
    /// Send nothing at all, as a monitor with DDC/CI switched off does.
    Silent,
}

/// A scripted monitor.
///
/// Built with [`Panel::new`] and then taught what it knows: features with
/// [`Panel::with_feature`], a capability string with
/// [`Panel::with_capabilities`], and misbehaviour with [`Panel::with_fault`].
#[derive(Debug, Clone)]
pub struct Panel {
    name: String,
    /// Feature code to (current, maximum).
    features: BTreeMap<u8, (u16, u16)>,
    capabilities: Vec<u8>,
    faults: VecDeque<Fault>,
    /// The reply the next `receive` will hand back.
    pending: Option<Vec<u8>>,
    /// The reply before that, for [`Fault::Stale`].
    previous: Option<Vec<u8>>,
    /// How many times this panel has been told to commit its settings.
    ///
    /// A save is answered by nothing, so a count here is the only way to see
    /// that one happened — and the only way to check the more important
    /// property, that one did *not*.
    saves: usize,
    /// Every request this panel was sent, framing and all.
    ///
    /// Kept so a test can assert what went on the wire, not merely what came
    /// back off it — a write is answered by nothing, so the request bytes are
    /// the only evidence it happened.
    seen: Vec<Vec<u8>>,
}

impl Panel {
    /// A panel that knows nothing yet, called `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            features: BTreeMap::new(),
            capabilities: Vec::new(),
            faults: VecDeque::new(),
            pending: None,
            previous: None,
            saves: 0,
            seen: Vec::new(),
        }
    }

    /// Teach it a feature, with a current and a maximum value.
    #[must_use]
    pub fn with_feature(mut self, code: u8, current: u16, maximum: u16) -> Self {
        self.features.insert(code, (current, maximum));
        self
    }

    /// Give it a capability string to hand out in fragments.
    #[must_use]
    pub fn with_capabilities(mut self, text: &str) -> Self {
        self.capabilities = text.as_bytes().to_vec();
        self
    }

    /// Queue one misbehaviour, consumed by the next exchange.
    #[must_use]
    pub fn with_fault(mut self, fault: Fault) -> Self {
        self.faults.push_back(fault);
        self
    }

    /// Queue one misbehaviour on a panel already in use.
    ///
    /// [`Panel::with_fault`] is for building; this is for the middle of a run,
    /// where the interesting faults are — a monitor that answers three
    /// questions and then hesitates on the fourth is the ordinary case.
    pub fn queue(&mut self, fault: Fault) {
        self.faults.push_back(fault);
    }

    /// What this panel currently believes a feature is set to.
    ///
    /// The point of a write test: a `Set` is answered by nothing, so the only
    /// way to see that it landed is to ask the panel afterwards.
    #[must_use]
    pub fn feature(&self, code: u8) -> Option<(u16, u16)> {
        self.features.get(&code).copied()
    }

    /// How many times this panel has been told to commit its settings.
    #[must_use]
    pub fn saves(&self) -> usize {
        self.saves
    }

    /// Every request this panel has been sent, oldest first.
    #[must_use]
    pub fn seen(&self) -> &[Vec<u8>] {
        &self.seen
    }

    /// Wrap the payload in a reply's framing.
    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(payload.len() + 3);
        bytes.push(DISPLAY_ADDRESS);
        // The length field is 7 bits; no reply this panel builds comes close
        // to overflowing it, and a fragment is capped at 32 bytes above.
        let declared = u8::try_from(payload.len()).unwrap_or(0);
        bytes.push(LENGTH_FLAG | declared);
        bytes.extend_from_slice(payload);
        let sum = bytes
            .iter()
            .fold(VIRTUAL_HOST_ADDRESS, |sum, byte| sum ^ byte);
        bytes.push(sum);
        bytes
    }

    /// The reply to a `Get`.
    fn feature_reply(&self, code: u8) -> Vec<u8> {
        let Some(&(current, maximum)) = self.features.get(&code) else {
            // Result byte 0x01: the monitor has no such feature. The remaining
            // bytes are still sent; a monitor does not shorten the message.
            return Self::frame(&[OP_GET_REPLY, 0x01, code, 0, 0, 0, 0, 0]);
        };
        let [max_high, max_low] = maximum.to_be_bytes();
        let [cur_high, cur_low] = current.to_be_bytes();
        Self::frame(&[
            OP_GET_REPLY,
            0x00,
            code,
            0x00,
            max_high,
            max_low,
            cur_high,
            cur_low,
        ])
    }

    /// The reply to a capability read at `offset`.
    fn capability_reply(&self, offset: u16) -> Vec<u8> {
        let start = usize::from(offset).min(self.capabilities.len());
        let end = (start + FRAGMENT).min(self.capabilities.len());
        let [high, low] = offset.to_be_bytes();
        let mut payload = vec![OP_CAPABILITIES_REPLY, high, low];
        payload.extend_from_slice(&self.capabilities[start..end]);
        Self::frame(&payload)
    }
}

impl DdcTransport for Panel {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn send(&mut self, frame: &Frame) -> Result<(), DisplayError> {
        let bytes = frame.as_bytes();
        self.seen.push(bytes.to_vec());

        // Source, length, then the payload: the panel reads the request the
        // same way a monitor does, by position.
        let payload = bytes.get(2..).unwrap_or_default();
        let reply = match payload.first().copied() {
            Some(OP_GET) => payload.get(1).map(|&code| self.feature_reply(code)),
            Some(OP_CAPABILITIES) => {
                let offset = match (payload.get(1), payload.get(2)) {
                    (Some(&high), Some(&low)) => u16::from_be_bytes([high, low]),
                    _ => 0,
                };
                Some(self.capability_reply(offset))
            }
            Some(OP_SET) => {
                if let (Some(&code), Some(&high), Some(&low)) =
                    (payload.get(1), payload.get(2), payload.get(3))
                {
                    let value = u16::from_be_bytes([high, low]);
                    let maximum = self.features.get(&code).map_or(value, |&(_, max)| max);
                    self.features.insert(code, (value, maximum));
                }
                None
            }
            Some(OP_SAVE) => {
                self.saves += 1;
                None
            }
            // Anything this panel does not recognise is answered by nothing
            // too: a real monitor stays quiet rather than complaining about an
            // opcode it has never heard of.
            _ => None,
        };

        self.previous = self.pending.take();
        self.pending = reply;
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, DisplayError> {
        let reply = match self.faults.pop_front() {
            // A null message: framing, no payload. Its own checksum still has
            // to be right, or the layer above would call it corruption
            // instead of hesitation and never learn to wait.
            Some(Fault::NotReady) => Some(Self::frame(&[])),
            Some(Fault::Stale) => self.previous.clone().or_else(|| Some(Self::frame(&[]))),
            Some(Fault::BadChecksum) => self.pending.clone().map(|mut bytes| {
                if let Some(last) = bytes.last_mut() {
                    *last = last.wrapping_add(1);
                }
                bytes
            }),
            Some(Fault::Silent) => None,
            None => self.pending.clone(),
        };

        let Some(reply) = reply else {
            return Ok(0);
        };
        let len = reply.len().min(buffer.len());
        buffer[..len].copy_from_slice(&reply[..len]);
        Ok(len)
    }
}

#[cfg(test)]
mod tests;
