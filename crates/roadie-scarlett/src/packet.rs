//! Request and response framing.
//!
//! Every exchange is two USB control transfers rather than one: the request
//! goes out on `bRequest` 2, and then the answer is *fetched* on `bRequest` 3.
//! Both carry the same sixteen-byte header, little-endian throughout:
//!
//! | Bytes | Field | Meaning |
//! |---|---|---|
//! | 0–3 | command | which operation |
//! | 4–5 | size | length of the payload after the header |
//! | 6–7 | sequence | a counter the device echoes back |
//! | 8–11 | error | zero on success |
//! | 12–15 | padding | zero, both ways |
//!
//! The sequence number is the part worth understanding, because getting it
//! wrong is silent. It rises by one per request and the device echoes it, so
//! the answer that comes back can be matched to the question that was asked.
//! Fetching the response is a *separate* transfer from sending the request, so
//! nothing but that number distinguishes this answer from the previous one
//! still sitting in the device's buffer — and a host that does not check it
//! reads a stale reply as a fresh one. That is the same trap as the reply
//! checksum in DDC and the echo in VIA, and it is checked here for the same
//! reason.

use thiserror::Error;

/// Length of the header that precedes every payload.
pub const HEADER_LEN: usize = 16;

/// `bRequest` for sending a request.
pub const REQUEST: u8 = 2;

/// `bRequest` for fetching the response to one.
pub const RESPONSE: u8 = 3;

/// First of the two commands that bring a device up.
pub const INIT_1: u32 = 0x0000_0000;

/// Second of them, and the one the device answers oddly: during start-up the
/// request carrying sequence 1 is answered with sequence 0. See
/// [`Request::parse_response`], which is where that exception is applied and
/// where it is explained.
pub const INIT_2: u32 = 0x0000_0002;

/// How many payload bytes [`INIT_2`] answers with.
///
/// A host has to say in advance how much to fetch, so this is not a detail it
/// can discover. Confirmed on a Vocaster Two on 2026-09-03: the reply is 100
/// bytes, sixteen of header and eighty-four of payload.
pub const INIT_2_RESPONSE_LEN: usize = 84;

/// Where the firmware version sits inside [`INIT_2`]'s answer.
const FIRMWARE_OFFSET: usize = 8;

/// The firmware version an interface reports in its [`INIT_2`] answer.
///
/// Worth having rather than skipping, because [`crate::device::Model`] keeps
/// more than one table for some models and picks between them by version —
/// so a host that never reads this would silently address the oldest layout.
///
/// Read at payload offset 8 as a little-endian `u32`. Confirmed against a
/// Vocaster Two, which answered 1749 there while reporting `bcdDevice` 1749
/// on its USB descriptor — two independent statements of the same number,
/// which is what makes the offset believable rather than merely plausible.
///
/// # Errors
///
/// [`ProtocolError::PayloadLength`] if the answer is too short to hold it,
/// which means the reply was truncated rather than that the device is old.
pub fn firmware_version(init_2_payload: &[u8]) -> Result<u32, ProtocolError> {
    init_2_payload
        .get(FIRMWARE_OFFSET..FIRMWARE_OFFSET + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(ProtocolError::PayloadLength {
            expected: FIRMWARE_OFFSET + 4,
            actual: init_2_payload.len(),
        })
}

/// The counter that ties an answer to its question.
///
/// Its own type because it has one rule — it rises by one per request, and it
/// wraps rather than stopping — and because a bare `u16` in a host is easy to
/// forget to advance. Forgetting is not a compile error and not a runtime
/// error either: it is a device that answers the previous question.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sequence(u16);

impl Sequence {
    /// The counter a freshly opened device starts from.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    /// The current value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Take the current value and advance.
    ///
    /// Wrapping rather than saturating: a long-lived session will pass 65535,
    /// and a counter that stopped there would make every later answer look
    /// like a mismatch. Wrapping is what the device does.
    pub const fn next(&mut self) -> u16 {
        let issued = self.0;
        self.0 = self.0.wrapping_add(1);
        issued
    }
}

/// A framed request, ready to be written to the control endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The bytes to send, header and payload together.
    bytes: Vec<u8>,
    /// The sequence this request carries, for checking the answer.
    sequence: u16,
    /// The command this request carries, likewise.
    command: u32,
}

impl Request {
    /// Frame `command` with `payload`, taking the next sequence number.
    ///
    /// # Panics
    ///
    /// Never. A payload longer than `u16::MAX` cannot be framed, and is
    /// rejected as an error rather than truncated — truncation would send a
    /// header promising more than the payload holds, which the device answers
    /// by hanging up rather than by complaining.
    pub fn new(
        command: u32,
        payload: &[u8],
        sequence: &mut Sequence,
    ) -> Result<Self, ProtocolError> {
        let size = u16::try_from(payload.len()).map_err(|_| ProtocolError::PayloadTooLong {
            length: payload.len(),
        })?;
        let issued = sequence.next();
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(&command.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&issued.to_le_bytes());
        // Error and padding are zero on the way out. The device fills the
        // error field on the way back; padding stays zero in both directions
        // and is checked, because a non-zero one means this is not the reply
        // that was expected.
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(payload);
        Ok(Self {
            bytes,
            sequence: issued,
            command,
        })
    }

    /// The bytes to write.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// How many bytes the answer's header plus payload will occupy.
    ///
    /// The host has to say in advance, because fetching the response is its
    /// own transfer with its own length. `expected_payload` is what the caller
    /// asked the device for — a read of four bytes, say — not the length of
    /// what was sent.
    #[must_use]
    pub const fn response_len(expected_payload: usize) -> usize {
        HEADER_LEN + expected_payload
    }

    /// Check `bytes` against this request, and take the payload out of it.
    ///
    /// The check is the point of the method. Anything that does not line up
    /// with the request is refused rather than returned, because every failure
    /// mode here produces plausible-looking bytes: a stale reply is a real
    /// reply to an older question, and a truncated one is a real prefix.
    pub fn parse_response(
        &self,
        bytes: &[u8],
        expected_payload: usize,
    ) -> Result<Response, ProtocolError> {
        let packet = Packet::parse(bytes)?;
        if packet.command != self.command {
            return Err(ProtocolError::CommandMismatch {
                sent: self.command,
                received: packet.command,
            });
        }
        if !self.sequence_matches(packet.sequence) {
            return Err(ProtocolError::SequenceMismatch {
                sent: self.sequence,
                received: packet.sequence,
            });
        }
        if packet.error != 0 {
            return Err(ProtocolError::Device { code: packet.error });
        }
        if packet.padding != 0 {
            return Err(ProtocolError::PaddingNotZero {
                padding: packet.padding,
            });
        }
        if packet.payload.len() != expected_payload {
            return Err(ProtocolError::PayloadLength {
                expected: expected_payload,
                actual: packet.payload.len(),
            });
        }
        Ok(Response {
            payload: packet.payload,
        })
    }

    /// Whether the sequence in an answer belongs to this request.
    ///
    /// Exact, with one exception: during start-up the device answers the
    /// request carrying sequence 1 with sequence 0.
    ///
    /// The exception is tied to the initialisation commands rather than to the
    /// number, and that narrowing is the whole point. Stated as "sequence 1 may
    /// be answered with 0" it collides exactly with the failure this check
    /// exists to catch — the second request of any session carries sequence 1,
    /// and the stale reply still sitting in the device's buffer carries 0, so
    /// the rule would wave through the one case it was written to stop. Tied to
    /// the two commands that only ever run at start-up, it cannot.
    const fn sequence_matches(&self, received: u16) -> bool {
        if received == self.sequence {
            return true;
        }
        let initialising = matches!(self.command, INIT_1 | INIT_2);
        initialising && self.sequence == 1 && received == 0
    }
}

/// A parsed header and its payload, before any request-specific checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// The command echoed back.
    pub command: u32,
    /// The sequence echoed back.
    pub sequence: u16,
    /// The device's error code; zero means success.
    pub error: u32,
    /// The padding field, which should be zero.
    pub padding: u32,
    /// Everything after the header.
    pub payload: Vec<u8>,
}

impl Packet {
    /// Split `bytes` into a header and a payload.
    pub fn parse(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let header: [u8; HEADER_LEN] = bytes
            .get(..HEADER_LEN)
            .and_then(|head| head.try_into().ok())
            .ok_or(ProtocolError::TooShort {
                length: bytes.len(),
            })?;
        // Indexing is over a fixed-size array whose length is HEADER_LEN, so
        // every slice below is in bounds by construction.
        let command = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let size = u16::from_le_bytes([header[4], header[5]]);
        let sequence = u16::from_le_bytes([header[6], header[7]]);
        let error = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        let padding = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        let payload = bytes.get(HEADER_LEN..).unwrap_or_default().to_vec();
        // The header states its own payload length, and a disagreement with
        // what actually arrived means the transfer was cut short. Believing
        // the header would read past the end; believing the buffer would
        // silently accept half an answer.
        if usize::from(size) != payload.len() {
            return Err(ProtocolError::PayloadLength {
                expected: usize::from(size),
                actual: payload.len(),
            });
        }
        Ok(Self {
            command,
            sequence,
            error,
            padding,
            payload,
        })
    }
}

/// The payload of a checked response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    payload: Vec<u8>,
}

impl Response {
    /// The bytes the device sent back.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The payload as one little-endian `u32`, for the many reads that are one.
    pub fn as_u32(&self) -> Result<u32, ProtocolError> {
        self.payload
            .as_slice()
            .try_into()
            .map(u32::from_le_bytes)
            .map_err(|_| ProtocolError::PayloadLength {
                expected: 4,
                actual: self.payload.len(),
            })
    }
}

/// Everything that can be wrong with an exchange.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    /// The answer was shorter than a header.
    #[error(
        "the interface sent back {length} bytes, which is less than the {HEADER_LEN} a reply \
         always starts with. The usual cause is the transfer being cut short rather than the \
         interface being wrong."
    )]
    TooShort {
        /// How many bytes did arrive.
        length: usize,
    },

    /// The request's payload will not fit the length field.
    #[error("a request payload of {length} bytes is longer than this protocol can describe")]
    PayloadTooLong {
        /// The payload's length.
        length: usize,
    },

    /// The payload was not the length it should have been.
    #[error("expected {expected} bytes of payload and received {actual}")]
    PayloadLength {
        /// What was asked for.
        expected: usize,
        /// What arrived.
        actual: usize,
    },

    /// The answer was to a different question.
    #[error(
        "asked the interface for command {sent:#x} and it answered {received:#x}. That is a \
         reply to a different request, so nothing in it should be believed."
    )]
    CommandMismatch {
        /// The command sent.
        sent: u32,
        /// The command echoed back.
        received: u32,
    },

    /// The answer was to an earlier question.
    #[error(
        "asked the interface question {sent} and it answered question {received}. That is an \
         older reply still in its buffer, not an answer to this one."
    )]
    SequenceMismatch {
        /// The sequence sent.
        sent: u16,
        /// The sequence echoed back.
        received: u16,
    },

    /// The device reported a failure.
    #[error("the interface refused the request with error code {code:#x}")]
    Device {
        /// The device's own code.
        code: u32,
    },

    /// The padding field was not zero.
    #[error(
        "a reply arrived with {padding:#x} where the padding should be zero, so it is not a \
         reply this protocol understands"
    )]
    PaddingNotZero {
        /// What was in the padding field.
        padding: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply built the way a device would build one.
    fn reply(command: u32, sequence: u16, error: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&command.to_le_bytes());
        bytes.extend_from_slice(
            &u16::try_from(payload.len())
                .expect("test payloads are short")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&sequence.to_le_bytes());
        bytes.extend_from_slice(&error.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn a_request_is_a_header_then_its_payload() {
        let mut sequence = Sequence::new();
        let request = Request::new(0x0080_0000, &[0xAA, 0xBB], &mut sequence).expect("framed");
        assert_eq!(
            request.bytes(),
            &[
                0x00, 0x00, 0x80, 0x00, // command, little-endian
                0x02, 0x00, // payload length
                0x00, 0x00, // sequence, the first one
                0x00, 0x00, 0x00, 0x00, // error, zero on the way out
                0x00, 0x00, 0x00, 0x00, // padding
                0xAA, 0xBB, // the payload itself
            ]
        );
    }

    #[test]
    fn the_sequence_rises_by_one_per_request() {
        let mut sequence = Sequence::new();
        let first = Request::new(1, &[], &mut sequence).expect("framed");
        let second = Request::new(1, &[], &mut sequence).expect("framed");
        assert_eq!(first.sequence, 0);
        assert_eq!(second.sequence, 1);
    }

    #[test]
    fn the_sequence_wraps_rather_than_stopping() {
        // A long session passes 65535, and a counter that saturated there
        // would make every later answer look like a stale one.
        let mut sequence = Sequence(u16::MAX);
        assert_eq!(sequence.next(), u16::MAX);
        assert_eq!(sequence.get(), 0);
    }

    #[test]
    fn a_matching_reply_yields_its_payload() {
        let mut sequence = Sequence::new();
        let request = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let answer = request
            .parse_response(&reply(0x1001, 0, 0, &[1, 2, 3, 4]), 4)
            .expect("a good reply");
        assert_eq!(answer.payload(), &[1, 2, 3, 4]);
        assert_eq!(answer.as_u32().expect("four bytes"), 0x0403_0201);
    }

    #[test]
    fn an_answer_to_an_earlier_question_is_refused() {
        // The failure this whole type exists for. Fetching the reply is a
        // separate transfer, so a stale one is a real reply that arrives
        // intact — nothing but the sequence says it is the wrong one.
        let mut sequence = Sequence::new();
        let _first = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let second = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let error = second
            .parse_response(&reply(0x1001, 0, 0, &[9, 9, 9, 9]), 4)
            .expect_err("that is the answer to the first request");
        assert_eq!(
            error,
            ProtocolError::SequenceMismatch {
                sent: 1,
                received: 0
            }
        );
    }

    #[test]
    fn the_initialising_exchange_may_be_answered_with_zero() {
        // Start-up answers the request carrying sequence 1 with sequence 0.
        let mut sequence = Sequence::new();
        let _first = Request::new(INIT_1, &[], &mut sequence).expect("framed");
        let second = Request::new(INIT_2, &[], &mut sequence).expect("framed");
        assert!(
            second.parse_response(&reply(INIT_2, 0, 0, &[]), 0).is_ok(),
            "the initialising exchange is answered with sequence zero"
        );
    }

    #[test]
    fn the_exception_does_not_leak_into_ordinary_commands() {
        // The reason the exception names the two start-up commands instead of
        // the number: every session's second request carries sequence 1, and
        // the stale reply left over from its first carries 0. A rule phrased
        // as "1 may be answered with 0" would wave through precisely the case
        // the sequence check exists to catch.
        let mut sequence = Sequence::new();
        let _first = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let second = Request::new(0x1001, &[], &mut sequence).expect("framed");
        assert_eq!(
            second.parse_response(&reply(0x1001, 0, 0, &[]), 0),
            Err(ProtocolError::SequenceMismatch {
                sent: 1,
                received: 0
            }),
            "a stale reply must not be excused by the start-up rule"
        );
    }

    #[test]
    fn an_answer_to_a_different_command_is_refused() {
        let mut sequence = Sequence::new();
        let request = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let error = request
            .parse_response(&reply(0x2001, 0, 0, &[]), 0)
            .expect_err("wrong command");
        assert!(matches!(error, ProtocolError::CommandMismatch { .. }));
    }

    #[test]
    fn a_device_error_is_reported_rather_than_read_past() {
        let mut sequence = Sequence::new();
        let request = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let error = request
            .parse_response(&reply(0x1001, 0, 0x0000_000E, &[]), 0)
            .expect_err("the device refused");
        assert_eq!(error, ProtocolError::Device { code: 0x0E });
    }

    #[test]
    fn a_reply_shorter_than_its_header_is_refused() {
        assert_eq!(
            Packet::parse(&[0, 1, 2]),
            Err(ProtocolError::TooShort { length: 3 })
        );
    }

    #[test]
    fn a_header_promising_more_than_arrived_is_refused() {
        // A truncated transfer is a real prefix of a real answer, so nothing
        // in the bytes themselves gives it away — only the length field does.
        let mut truncated = reply(0x1001, 0, 0, &[1, 2, 3, 4]);
        truncated.truncate(HEADER_LEN + 2);
        assert_eq!(
            Packet::parse(&truncated),
            Err(ProtocolError::PayloadLength {
                expected: 4,
                actual: 2
            })
        );
    }

    #[test]
    fn a_reply_with_rubbish_in_its_padding_is_refused() {
        let mut sequence = Sequence::new();
        let request = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let mut answer = reply(0x1001, 0, 0, &[]);
        answer[12] = 0xFF;
        let error = request
            .parse_response(&answer, 0)
            .expect_err("padding is checked");
        assert!(matches!(error, ProtocolError::PaddingNotZero { .. }));
    }

    #[test]
    fn a_reply_of_the_wrong_length_is_refused_even_when_it_parses() {
        let mut sequence = Sequence::new();
        let request = Request::new(0x1001, &[], &mut sequence).expect("framed");
        let error = request
            .parse_response(&reply(0x1001, 0, 0, &[1, 2]), 4)
            .expect_err("asked for four bytes");
        assert_eq!(
            error,
            ProtocolError::PayloadLength {
                expected: 4,
                actual: 2
            }
        );
    }

    #[test]
    fn the_response_length_a_host_must_ask_for_includes_the_header() {
        assert_eq!(Request::response_len(4), HEADER_LEN + 4);
        assert_eq!(Request::response_len(0), HEADER_LEN);
    }

    /// The bytes are the ones a Vocaster Two actually sent on 2026-09-03,
    /// written out rather than computed, so this checks the offset from
    /// outside the code that reads it.
    #[test]
    fn the_firmware_version_is_read_from_the_start_up_answer() {
        let payload = [
            0x03, 0x00, 0x00, 0x00, // unknown
            0x06, 0xc0, 0x60, 0x00, // unknown
            0xd5, 0x06, 0x00, 0x00, // firmware version, 1749
            0x00, 0x00, 0x10, 0x00, // unknown
        ];
        assert_eq!(firmware_version(&payload), Ok(1749));
    }

    #[test]
    fn a_start_up_answer_too_short_to_hold_a_version_is_refused() {
        // Truncation, not an old device: returning zero here would pick the
        // oldest table on a device whose version was simply never read.
        assert_eq!(
            firmware_version(&[0; 8]),
            Err(ProtocolError::PayloadLength {
                expected: 12,
                actual: 8
            })
        );
    }
}
