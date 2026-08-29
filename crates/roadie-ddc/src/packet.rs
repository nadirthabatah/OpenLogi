//! Framing: how a request becomes bytes, and how bytes become a reply.
//!
//! # The checksum is the part that catches people out
//!
//! A request and a reply are checksummed by the same XOR, over different
//! bytes, and nothing on the wire announces which rule applies. A request is
//! XOR'd over every byte of the message *including the display's own bus
//! address*, `0x6E` — a byte the caller never writes, because the I²C adapter
//! puts it on the wire. A reply is XOR'd over every byte the monitor sent,
//! seeded with `0x50`: a host address that exists nowhere in the transaction
//! and is there purely because the specification says so.
//!
//! Get that seed wrong and every reply fails its checksum, which reads exactly
//! like a monitor that does not speak DDC — so a wrong constant here costs a
//! long afternoon of blaming the hardware. Both rules are spelled out in
//! [`Request::frame`] and [`checksum`], and both are tested against captured
//! byte sequences rather than against themselves.
//!
//! # A reply that answers the wrong question
//!
//! Every `Get VCP Feature` reply echoes the opcode it is answering, and that
//! echo is not decoration. DDC has no sequence numbers and no framing between
//! transactions: if a monitor is slow and the host reads before the reply
//! lands, the read can return the *previous* answer, and every read after it
//! is shifted by one. Brightness would be set from a contrast reading, and
//! nothing would report an error.
//!
//! So the echo is checked, always. [`Request::parse_reply`] takes the request
//! that was sent and rejects an answer that names a different feature, which
//! is why parsing goes through the request rather than standing alone.

use core::time::Duration;

use crate::vcp::{Feature, Value};

/// The I²C address a monitor answers DDC/CI on.
///
/// `0x50` on the same bus is the EDID EEPROM, which is a different thing
/// entirely: it is read-only identification, and it answers even on monitors
/// that have DDC/CI switched off in their menu. Finding an EDID at `0x50` says
/// a display is there; it says nothing about whether it will take orders.
pub const I2C_ADDRESS: u8 = 0x37;

/// The display's 8-bit write address. Never transmitted by the caller — the
/// I²C layer emits it — but folded into every request checksum.
const DISPLAY_ADDRESS: u8 = 0x6E;

/// The host's source address, the first byte of every request.
const HOST_ADDRESS: u8 = 0x51;

/// The "virtual host address" a reply's checksum is seeded with. It is not the
/// host's address (`0x51`) and not the display's (`0x6E`); it is a third
/// number the specification names for this one purpose.
const VIRTUAL_HOST_ADDRESS: u8 = 0x50;

/// The high bit set on every length byte.
const LENGTH_FLAG: u8 = 0x80;

/// The largest payload a length byte can describe, the field being 7 bits.
/// Real messages stay far under it — capability fragments are the longest at
/// 32 data bytes — but the field's range is the honest bound.
pub const MAX_PAYLOAD: usize = 0x7F;

/// The largest reply that can arrive: source, length, payload, checksum.
pub const MAX_REPLY: usize = MAX_PAYLOAD + 3;

/// The largest request this crate builds: `Set VCP Feature`, at four payload
/// bytes plus source, length, and checksum.
const MAX_REQUEST: usize = 7;

// Payload opcodes, host side.
const OP_GET: u8 = 0x01;
const OP_SET: u8 = 0x03;
const OP_SAVE: u8 = 0x0C;
const OP_CAPABILITIES: u8 = 0xF3;

// Payload opcodes, monitor side.
const OP_GET_REPLY: u8 = 0x02;
const OP_CAPABILITIES_REPLY: u8 = 0xE3;

/// The XOR checksum over `bytes`, seeded with `seed`.
///
/// The seed is the whole difference between the two directions: a request
/// seeds with the display's bus address, a reply with the virtual host
/// address. Splitting it out makes the asymmetry a parameter rather than two
/// near-identical loops that could drift apart.
#[must_use]
pub fn checksum(seed: u8, bytes: &[u8]) -> u8 {
    bytes.iter().fold(seed, |sum, byte| sum ^ byte)
}

/// A request, ready to hand to an I²C write.
///
/// Short enough to live on the stack, which keeps this crate free of `alloc`
/// and therefore usable everywhere the wasm portability job checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    bytes: [u8; MAX_REQUEST],
    len: usize,
}

impl Frame {
    /// The bytes to write, in order, to [`I2C_ADDRESS`].
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl AsRef<[u8]> for Frame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

/// Something to ask a monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Read one feature's current and maximum value.
    Get(Feature),
    /// Write one feature. The monitor sends nothing back, so the only way to
    /// know a write took is to read the feature again.
    Set {
        /// The feature to write.
        feature: Feature,
        /// The value to write, in the feature's own units.
        value: u16,
    },
    /// Commit the current settings to the monitor's non-volatile memory.
    ///
    /// Deliberate only. That memory has a finite number of erase cycles, and a
    /// caller that saved after every brightness nudge would spend them on
    /// nothing: a plain `Set` already survives until the monitor loses power.
    SaveSettings,
    /// Read a slice of the capability string, starting at `offset`.
    ///
    /// The string arrives in fragments and the caller loops, advancing the
    /// offset by however many bytes came back, until a reply carries none.
    Capabilities {
        /// Byte offset into the capability string.
        offset: u16,
    },
}

impl Request {
    /// The payload bytes, without framing.
    ///
    /// The length comes back as a `u8` because it is about to become part of a
    /// length byte. Carrying it as a `usize` and narrowing later would put a
    /// fallible conversion in the middle of a function that cannot fail.
    fn payload(self) -> ([u8; 4], u8) {
        match self {
            Self::Get(feature) => ([OP_GET, feature.code(), 0, 0], 2),
            Self::Set { feature, value } => {
                let [high, low] = value.to_be_bytes();
                ([OP_SET, feature.code(), high, low], 4)
            }
            Self::SaveSettings => ([OP_SAVE, 0, 0, 0], 1),
            Self::Capabilities { offset } => {
                let [high, low] = offset.to_be_bytes();
                ([OP_CAPABILITIES, high, low, 0], 3)
            }
        }
    }

    /// The bytes to write to the bus.
    ///
    /// The display's address is XOR'd into the checksum but not written: the
    /// I²C layer emits it as part of addressing the device, so writing it here
    /// too would put it on the wire twice.
    #[must_use]
    pub fn frame(self) -> Frame {
        let (payload, declared) = self.payload();
        let len = usize::from(declared);
        let mut bytes = [0_u8; MAX_REQUEST];
        bytes[0] = HOST_ADDRESS;
        bytes[1] = LENGTH_FLAG | declared;
        bytes[2..2 + len].copy_from_slice(&payload[..len]);
        bytes[2 + len] = checksum(DISPLAY_ADDRESS, &bytes[..2 + len]);
        Frame {
            bytes,
            len: len + 3,
        }
    }

    /// Whether this request is answered at all.
    ///
    /// `Set` and `SaveSettings` are not. Reading after one of them returns
    /// whatever was left in the monitor's buffer, which is the shifted-reply
    /// hazard [`Self::parse_reply`] guards against — so the right move is not
    /// to read.
    #[must_use]
    pub const fn expects_reply(self) -> bool {
        matches!(self, Self::Get(_) | Self::Capabilities { .. })
    }

    /// How long to wait after writing this request before doing anything else.
    ///
    /// These are the specification's floors, not comfortable margins. A
    /// monitor that is slower than its own spec answers a rushed read with
    /// truncated bytes rather than an error, so a host that finds a panel
    /// flaky should raise these before suspecting the framing.
    #[must_use]
    pub const fn settle(self) -> Duration {
        match self {
            // The interval the spec requires between any two messages.
            Self::Get(_) => Duration::from_millis(40),
            // A write needs longer than a read, and the monitor says nothing
            // when it is done, so this delay is the only thing separating one
            // write from the next. A capability read wants the same 50: it is
            // the longer of the two floors, and the same number for two
            // different reasons rather than one shared one.
            Self::Set { .. } | Self::Capabilities { .. } => Duration::from_millis(50),
            // Writing non-volatile memory is slow in a way nothing else here
            // is, and interrupting it is how a monitor's settings get corrupted.
            Self::SaveSettings => Duration::from_millis(200),
        }
    }

    /// Parse the monitor's answer to *this* request.
    ///
    /// Parsing hangs off the request rather than standing alone so that the
    /// echoed opcode can be checked against what was actually asked. See the
    /// module docs: an unchecked echo is how a slow monitor turns one late
    /// reply into every subsequent reading being wrong.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] for a malformed, mis-checksummed, empty, or
    /// mismatched reply, and for a feature the monitor says it does not have.
    pub fn parse_reply(self, bytes: &[u8]) -> Result<Reply<'_>, ProtocolError> {
        let payload = unwrap_frame(bytes)?;
        match (self, payload) {
            (_, []) => Err(ProtocolError::Null),
            (Self::Get(asked), [OP_GET_REPLY, result, answered, kind, mh, ml, sh, sl]) => {
                if *result == 0x01 {
                    return Err(ProtocolError::Unsupported {
                        feature: asked.code(),
                    });
                }
                if *result != 0x00 {
                    return Err(ProtocolError::Failed { result: *result });
                }
                if *answered != asked.code() {
                    return Err(ProtocolError::WrongFeature {
                        asked: asked.code(),
                        answered: *answered,
                    });
                }
                Ok(Reply::Feature {
                    feature: asked,
                    momentary: *kind == 0x01,
                    value: Value {
                        current: u16::from_be_bytes([*sh, *sl]),
                        maximum: u16::from_be_bytes([*mh, *ml]),
                    },
                })
            }
            (
                Self::Capabilities { offset: asked },
                [OP_CAPABILITIES_REPLY, oh, ol, fragment @ ..],
            ) => {
                let answered = u16::from_be_bytes([*oh, *ol]);
                if answered != asked {
                    return Err(ProtocolError::WrongOffset { asked, answered });
                }
                Ok(Reply::Capabilities {
                    offset: answered,
                    fragment,
                })
            }
            (Self::Get(_) | Self::Capabilities { .. }, [op, ..]) => {
                Err(ProtocolError::UnexpectedOpcode { opcode: *op })
            }
            (Self::Set { .. } | Self::SaveSettings, _) => Err(ProtocolError::NotAnswered),
        }
    }
}

/// Strip framing from a reply and hand back its payload.
fn unwrap_frame(bytes: &[u8]) -> Result<&[u8], ProtocolError> {
    // Source, length, checksum: the shortest possible reply is a null message
    // with no payload at all, which monitors send when they have nothing to
    // say yet.
    let [source, length, rest @ ..] = bytes else {
        return Err(ProtocolError::TooShort { len: bytes.len() });
    };
    if *source != DISPLAY_ADDRESS {
        return Err(ProtocolError::NotFromDisplay { address: *source });
    }
    if length & LENGTH_FLAG == 0 {
        return Err(ProtocolError::MalformedLength { byte: *length });
    }
    let declared = usize::from(length & !LENGTH_FLAG);
    // `rest` still carries the checksum, so the payload is one byte shorter.
    let Some(available) = rest.len().checked_sub(1) else {
        return Err(ProtocolError::TooShort { len: bytes.len() });
    };
    if declared > available {
        return Err(ProtocolError::Truncated {
            declared,
            available,
        });
    }
    let (payload, tail) = rest.split_at(declared);
    let found = tail[0];
    let expected = checksum(VIRTUAL_HOST_ADDRESS, &bytes[..declared + 2]);
    if found != expected {
        return Err(ProtocolError::Checksum { expected, found });
    }
    Ok(payload)
}

/// What a monitor answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply<'a> {
    /// One feature's reading.
    Feature {
        /// The feature that was asked for, having been checked against the
        /// monitor's echo.
        feature: Feature,
        /// Whether the monitor calls this a momentary control — one that acts
        /// when written and has no resting value worth reading back, like
        /// "degauss". Continuous-versus-discrete is a different question, and
        /// the capability string rather than this byte is what answers it.
        momentary: bool,
        /// Current and maximum, in the feature's own units.
        value: Value,
    },
    /// A slice of the capability string.
    Capabilities {
        /// The offset the monitor says this fragment starts at, having been
        /// checked against the offset that was asked for.
        offset: u16,
        /// The fragment. Empty means the string has ended.
        fragment: &'a [u8],
    },
}

/// Why a reply could not be believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// Fewer bytes than a reply's framing needs.
    #[error("a DDC reply needs at least 3 bytes; got {len}")]
    TooShort {
        /// How many bytes arrived.
        len: usize,
    },
    /// The first byte was not the display's address.
    #[error("a DDC reply must come from 0x6e; this one came from {address:#04x}")]
    NotFromDisplay {
        /// The address the reply claimed to come from.
        address: u8,
    },
    /// The length byte was missing its high bit.
    #[error("a DDC length byte must have its high bit set; got {byte:#04x}")]
    MalformedLength {
        /// The length byte as received.
        byte: u8,
    },
    /// The reply claimed more payload than arrived.
    #[error("the reply claims {declared} payload bytes but only {available} arrived")]
    Truncated {
        /// The length the monitor declared.
        declared: usize,
        /// How many payload bytes actually arrived.
        available: usize,
    },
    /// The checksum did not match.
    ///
    /// Worth reading twice before blaming the monitor: the seed differs
    /// between requests and replies, and a host that uses the wrong one fails
    /// every reply identically.
    #[error("checksum mismatch: expected {expected:#04x}, got {found:#04x}")]
    Checksum {
        /// The checksum the bytes imply.
        expected: u8,
        /// The checksum the monitor sent.
        found: u8,
    },
    /// The monitor sent a null message: no payload at all.
    ///
    /// Not a fault. It is how a monitor says "not ready", and the answer is to
    /// wait and read again rather than to give up on the device.
    #[error("the monitor sent a null message; it is not ready to answer yet")]
    Null,
    /// The monitor answered a different feature than the one asked for.
    ///
    /// Almost always a timing problem rather than a broken monitor: a reply
    /// read too early returns the answer to the previous question.
    #[error("asked for feature {asked:#04x} but the monitor answered {answered:#04x}")]
    WrongFeature {
        /// The feature that was requested.
        asked: u8,
        /// The feature the monitor's reply named.
        answered: u8,
    },
    /// A capability fragment arrived for a different offset than the one
    /// asked for. The same timing hazard as [`Self::WrongFeature`].
    #[error("asked for capabilities at offset {asked} but got offset {answered}")]
    WrongOffset {
        /// The offset that was requested.
        asked: u16,
        /// The offset the monitor's reply named.
        answered: u16,
    },
    /// The monitor does not implement this feature.
    ///
    /// Expected, not exceptional: a monitor's capability string is the list of
    /// what it has, and asking outside that list is how a host discovers the
    /// edges of a panel that reports its capabilities badly.
    #[error("this monitor does not support feature {feature:#04x}")]
    Unsupported {
        /// The feature that was asked for.
        feature: u8,
    },
    /// The monitor reported a failure code this crate does not recognise.
    #[error("the monitor reported result code {result:#04x}")]
    Failed {
        /// The result byte from the reply.
        result: u8,
    },
    /// The reply's opcode was not the one this request is answered with.
    #[error("unexpected reply opcode {opcode:#04x}")]
    UnexpectedOpcode {
        /// The opcode the reply carried.
        opcode: u8,
    },
    /// A reply was parsed for a request that monitors do not answer.
    #[error("this request is not answered; reading after it returns stale bytes")]
    NotAnswered,
}

#[cfg(test)]
mod tests;
