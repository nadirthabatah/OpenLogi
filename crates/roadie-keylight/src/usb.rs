//! The USB framing a Key Light Neo speaks, and the identity that names one.
//!
//! A Key Light Neo is the first light in the family with a USB data port,
//! and over it the firmware speaks *the same application protocol* as the
//! Wi-Fi lights: the request line `GET /elgato/lights` answers with the JSON
//! that [`crate::state::Lights`] already models, and
//! `PUT /elgato/lights <json>` takes the same shape back. What USB adds is
//! only an envelope — the device is a USB HID, and messages are chunked into
//! 512-byte reports.
//!
//! The frame layout, transcribed from the one published reverse engineering
//! of this transport and confirmed against the light on this project's desk:
//!
//! byte 0 is the start marker `0x02`; byte 1 is the frame's index within its
//! message; byte 2 is how many frames the message has; byte 3 is the marker
//! `0x03`; bytes 4 and 5 are the body length as a little-endian sixteen-bit
//! number; the body follows; one more `0x03` closes it; and the rest of the
//! 512 bytes is zero padding. A frame therefore carries at most 505 body
//! bytes, and a message longer than that spans frames.
//!
//! Everything here is pure — bytes in, bytes out — so the framing is tested
//! without a light, and the crate's wasm portability claim is undisturbed.

use thiserror::Error;

/// Elgato's USB vendor id.
pub const VENDOR_ID: u16 = 0x0fd9;

/// The Key Light Neo's USB product id.
pub const NEO_PRODUCT_ID: u16 = 0x00a0;

/// The HID usage page the Neo's endpoint reports: the consumer page.
pub const USAGE_PAGE: u16 = 0x000c;

/// The HID usage the Neo's endpoint reports: consumer control.
pub const USAGE_ID: u16 = 0x0001;

/// Whether a HID collection is a Key Light Neo's control endpoint.
#[must_use]
pub const fn is_neo(vendor_id: u16, product_id: u16, usage_page: u16, usage_id: u16) -> bool {
    vendor_id == VENDOR_ID
        && product_id == NEO_PRODUCT_ID
        && usage_page == USAGE_PAGE
        && usage_id == USAGE_ID
}

/// Every frame is exactly this long, padded with zeros.
pub const FRAME_LEN: usize = 512;

/// How much body one frame carries: the length minus the six header bytes
/// and the closing marker.
pub const FRAME_BODY: usize = FRAME_LEN - HEADER_LEN - 1;

/// The fixed header: start marker, index, total, marker, and two length bytes.
const HEADER_LEN: usize = 6;

/// The marker every frame starts with.
const FRAME_START: u8 = 0x02;

/// The byte after the frame counters, and the one that closes the body.
///
/// Probably a message type rather than a marker: requests carry `0x03`
/// there and are accepted, but the light on this project's desk answers
/// with `0x00` in that position, so inbound frames are not judged by it.
/// The published reverse engineering ignores the byte on replies too, which
/// is consistent with it meaning something nobody has mapped yet.
const BODY_MARK: u8 = 0x03;

/// A message split into the frames that carry it.
///
/// # Errors
///
/// [`FrameError::MessageTooLong`] when the message needs more than 255
/// frames, which the one-byte frame count cannot express. At 505 bytes per
/// frame that is over 128 kilobytes — no real request comes anywhere near
/// it, but a limit the format imposes is stated rather than truncated into.
pub fn frames(message: &[u8]) -> Result<Vec<[u8; FRAME_LEN]>, FrameError> {
    let chunks: Vec<&[u8]> = if message.is_empty() {
        vec![&[]]
    } else {
        message.chunks(FRAME_BODY).collect()
    };
    let total = u8::try_from(chunks.len()).map_err(|_| FrameError::MessageTooLong {
        bytes: message.len(),
    })?;
    Ok(chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let mut frame = [0_u8; FRAME_LEN];
            frame[0] = FRAME_START;
            // The chunk count above bounds the index to a u8.
            #[expect(clippy::cast_possible_truncation, reason = "bounded by `total` above")]
            let index = index as u8;
            frame[1] = index;
            frame[2] = total;
            frame[3] = BODY_MARK;
            // Chunks are at most FRAME_BODY long, which fits sixteen bits.
            #[expect(clippy::cast_possible_truncation, reason = "FRAME_BODY < u16::MAX")]
            let length = chunk.len() as u16;
            frame[4..6].copy_from_slice(&length.to_le_bytes());
            frame[HEADER_LEN..HEADER_LEN + chunk.len()].copy_from_slice(chunk);
            frame[HEADER_LEN + chunk.len()] = BODY_MARK;
            frame
        })
        .collect())
}

/// Why a frame from the device could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FrameError {
    /// The message would need more frames than the format can count.
    #[error("a {bytes}-byte message does not fit in 255 frames")]
    MessageTooLong {
        /// How long the message was.
        bytes: usize,
    },
    /// Fewer bytes than a header arrived.
    #[error("the light sent {got} bytes; a frame carries at least {HEADER_LEN}")]
    Short {
        /// How many bytes arrived.
        got: usize,
    },
    /// The first byte was not the start marker.
    #[error("the light sent a frame starting {found:#04x} instead of {FRAME_START:#04x}")]
    BadStart {
        /// The byte that arrived instead.
        found: u8,
    },
    /// The header promises more body than the frame holds.
    #[error("the light claims {claimed} body bytes in a frame that carries {carried}")]
    LengthOverrun {
        /// The length field's claim.
        claimed: usize,
        /// How much body the frame could actually hold.
        carried: usize,
    },
    /// A frame names a different message shape than the ones before it.
    ///
    /// The frame count is repeated in every frame of a message, so two
    /// frames disagreeing means a reply was interleaved with another or a
    /// frame was lost — either way the message cannot be trusted.
    #[error("the light changed its frame count from {had} to {found} mid-message")]
    TotalChanged {
        /// The count the earlier frames carried.
        had: u8,
        /// The count this frame carries.
        found: u8,
    },
}

/// The frames of one message, collected as they arrive.
///
/// Frames carry their own index and count, so arrival order is recorded
/// rather than assumed — and a duplicate index keeps the first copy, which
/// makes replays harmless.
#[derive(Debug, Default)]
pub struct Reassembly {
    total: Option<u8>,
    chunks: Vec<(u8, Vec<u8>)>,
}

impl Reassembly {
    /// An empty reassembly, waiting for its first frame.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take one frame, and the whole message once this one completes it.
    ///
    /// # Errors
    ///
    /// Any [`FrameError`] the frame earns. An erroring frame is not
    /// accumulated, and the caller decides whether to abandon the message.
    pub fn accept(&mut self, frame: &[u8]) -> Result<Option<Vec<u8>>, FrameError> {
        if frame.len() < HEADER_LEN {
            return Err(FrameError::Short { got: frame.len() });
        }
        if frame[0] != FRAME_START {
            return Err(FrameError::BadStart { found: frame[0] });
        }
        // frame[3] is deliberately not judged; see [`BODY_MARK`]. The light
        // on this project's desk answers with 0x00 there, and refusing it
        // would refuse working hardware over a byte nobody has mapped.
        let index = frame[1];
        let total = frame[2];
        if let Some(had) = self.total
            && had != total
        {
            return Err(FrameError::TotalChanged { had, found: total });
        }
        let claimed = usize::from(u16::from_le_bytes([frame[4], frame[5]]));
        let carried = frame.len() - HEADER_LEN;
        if claimed > carried {
            return Err(FrameError::LengthOverrun { claimed, carried });
        }
        self.total = Some(total);
        if !self.chunks.iter().any(|&(had, _)| had == index) {
            self.chunks
                .push((index, frame[HEADER_LEN..HEADER_LEN + claimed].to_vec()));
        }
        let total = usize::from(total);
        if total > 0 && self.chunks.len() >= total {
            self.chunks.sort_by_key(|&(index, _)| index);
            let message = self
                .chunks
                .iter()
                .flat_map(|(_, chunk)| chunk.iter().copied())
                .collect();
            self.chunks.clear();
            self.total = None;
            return Ok(Some(message));
        }
        Ok(None)
    }
}

/// The request line that reads a path: `GET /elgato/lights`.
#[must_use]
pub fn read_request(path: &str) -> Vec<u8> {
    format!("GET {path}").into_bytes()
}

/// The request line that writes a path: `PUT /elgato/lights <json>`.
#[must_use]
pub fn write_request(path: &str, body: &str) -> Vec<u8> {
    format!("PUT {path} {body}").into_bytes()
}

#[cfg(test)]
mod tests;
