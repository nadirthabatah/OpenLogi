//! Framing a key image into the packets a Stream Deck accepts.
//!
//! Scope is deliberately narrow: this module chops an **already-encoded**
//! image blob into HID output reports. Producing that blob — scaling to the
//! model's [`size_px`](crate::model::KeyScreens::size_px), applying its
//! [`rotation`](crate::model::KeyScreens::rotation), and encoding to its
//! [`format`](crate::model::KeyScreens::format) — needs an imaging library
//! and belongs to a host layer, not to a pure protocol crate.
//!
//! Only [`Generation::Gen2`] framing is implemented. Gen 1 hardware uses a
//! different, larger packet with its own bitmap paging, and this crate
//! refuses it explicitly rather than emitting packets that would be wrong;
//! see [`ProtocolError::ImageFramingUnsupported`].

use crate::ProtocolError;
use crate::model::{Generation, Model};

/// Total size of one [`Generation::Gen2`] image output report, report ID and
/// header included.
pub const GEN2_PACKET_LEN: usize = 1024;

/// Size of the [`Generation::Gen2`] image packet header.
const GEN2_HEADER_LEN: usize = 8;

/// Image bytes carried by one [`Generation::Gen2`] packet.
const GEN2_PAYLOAD_LEN: usize = GEN2_PACKET_LEN - GEN2_HEADER_LEN;

/// Split an encoded key image into the output reports that upload it.
///
/// Every returned packet is exactly [`GEN2_PACKET_LEN`] bytes, zero-padded,
/// with the report ID as byte 0 — the shape a platform HID write expects.
/// The packets must be written in order; the last one carries the flag that
/// tells the device to display what it has received.
///
/// An empty `image` still yields one terminating packet, which is how a key
/// is cleared.
///
/// # Errors
///
/// - [`ProtocolError::ScreenlessModel`] if the model has no key screens.
/// - [`ProtocolError::KeyOutOfRange`] if `key` is not a key on this model.
/// - [`ProtocolError::ImageFramingUnsupported`] for [`Generation::Gen1`].
pub fn key_image_packets(
    model: &Model,
    key: u16,
    image: &[u8],
) -> Result<Vec<Vec<u8>>, ProtocolError> {
    if model.screens.is_none() {
        return Err(ProtocolError::ScreenlessModel { model: model.name });
    }
    if key >= model.key_count() {
        return Err(ProtocolError::KeyOutOfRange {
            index: key,
            count: model.key_count(),
        });
    }
    if model.generation == Generation::Gen1 {
        return Err(ProtocolError::ImageFramingUnsupported { model: model.name });
    }
    // Catalogued key counts are far below u8::MAX, and `key` was just bounds
    // checked against one, so the wire field cannot truncate.
    let key_byte = u8::try_from(key).map_err(|_| ProtocolError::KeyOutOfRange {
        index: key,
        count: model.key_count(),
    })?;

    let mut packets = Vec::new();
    let mut remaining = image;
    let mut page: u16 = 0;
    loop {
        let take = remaining.len().min(GEN2_PAYLOAD_LEN);
        let (chunk, rest) = remaining.split_at(take);
        let last = rest.is_empty();
        // `take` is capped at GEN2_PAYLOAD_LEN, which fits a u16 many times
        // over, so this length field cannot truncate.
        let length = u16::try_from(take).unwrap_or(u16::MAX);

        let mut packet = Vec::with_capacity(GEN2_PACKET_LEN);
        packet.extend_from_slice(&[
            0x02,
            0x07,
            key_byte,
            u8::from(last),
            length.to_le_bytes()[0],
            length.to_le_bytes()[1],
            page.to_le_bytes()[0],
            page.to_le_bytes()[1],
        ]);
        packet.extend_from_slice(chunk);
        packet.resize(GEN2_PACKET_LEN, 0);
        packets.push(packet);

        if last {
            break;
        }
        remaining = rest;
        page = page.saturating_add(1);
    }
    Ok(packets)
}

#[cfg(test)]
mod tests {
    use super::{GEN2_PACKET_LEN, key_image_packets};
    use crate::ProtocolError;
    use crate::model::{ELGATO_VENDOR_ID, Model, identify};

    fn mk2() -> &'static Model {
        identify(ELGATO_VENDOR_ID, 0x0080).expect("the MK.2 is catalogued")
    }

    /// Read a packet's declared payload length back out of its header.
    fn declared_len(packet: &[u8]) -> usize {
        usize::from(u16::from_le_bytes([packet[4], packet[5]]))
    }

    /// Read a packet's page number back out of its header.
    fn page(packet: &[u8]) -> u16 {
        u16::from_le_bytes([packet[6], packet[7]])
    }

    #[test]
    fn a_small_image_fits_one_terminating_packet() {
        let packets = key_image_packets(mk2(), 0, &[0xaa; 100]).expect("a valid upload");
        assert_eq!(packets.len(), 1);
        let packet = &packets[0];
        assert_eq!(packet.len(), GEN2_PACKET_LEN);
        assert_eq!(&packet[..4], &[0x02, 0x07, 0, 1], "key 0, last packet");
        assert_eq!(declared_len(packet), 100);
        assert_eq!(page(packet), 0);
        assert_eq!(&packet[8..108], &[0xaa; 100]);
        assert!(packet[108..].iter().all(|&b| b == 0), "padded with zeroes");
    }

    #[test]
    fn an_empty_image_still_yields_one_packet_so_a_key_can_be_cleared() {
        let packets = key_image_packets(mk2(), 4, &[]).expect("a valid upload");
        assert_eq!(packets.len(), 1);
        assert_eq!(&packets[0][..4], &[0x02, 0x07, 4, 1]);
        assert_eq!(declared_len(&packets[0]), 0);
    }

    #[test]
    fn a_large_image_is_paged_and_only_the_last_packet_is_flagged() {
        // Two full payloads plus a remainder.
        let image = vec![0x5a; 1016 * 2 + 7];
        let packets = key_image_packets(mk2(), 2, &image).expect("a valid upload");
        assert_eq!(packets.len(), 3);
        for (index, packet) in packets.iter().enumerate() {
            assert_eq!(packet.len(), GEN2_PACKET_LEN);
            assert_eq!(packet[2], 2, "every packet names the same key");
            assert_eq!(usize::from(page(packet)), index, "pages count up from 0");
        }
        assert_eq!(packets[0][3], 0);
        assert_eq!(packets[1][3], 0);
        assert_eq!(packets[2][3], 1, "only the final packet is flagged last");
        assert_eq!(declared_len(&packets[0]), 1016);
        assert_eq!(declared_len(&packets[1]), 1016);
        assert_eq!(declared_len(&packets[2]), 7);
    }

    #[test]
    fn an_exactly_full_payload_does_not_emit_a_trailing_empty_packet() {
        let packets = key_image_packets(mk2(), 0, &vec![1; 1016]).expect("a valid upload");
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0][3], 1);
        assert_eq!(declared_len(&packets[0]), 1016);
    }

    #[test]
    fn the_payload_reassembles_to_the_original_image() {
        let image: Vec<u8> = (0..5000u32)
            .map(|byte| u8::try_from(byte % 251).expect("a remainder below 251 fits a byte"))
            .collect();
        let packets = key_image_packets(mk2(), 1, &image).expect("a valid upload");
        let reassembled: Vec<u8> = packets
            .iter()
            .flat_map(|packet| packet[8..8 + declared_len(packet)].to_vec())
            .collect();
        assert_eq!(reassembled, image);
    }

    #[test]
    fn a_key_the_model_does_not_have_is_refused() {
        let error = key_image_packets(mk2(), 15, &[0]).expect_err("the MK.2 has 15 keys, 0..=14");
        assert!(matches!(
            error,
            ProtocolError::KeyOutOfRange {
                index: 15,
                count: 15
            }
        ));
    }

    #[test]
    fn a_screenless_model_is_refused_before_anything_is_framed() {
        let pedal = identify(ELGATO_VENDOR_ID, 0x0086).expect("the Pedal is catalogued");
        let error = key_image_packets(pedal, 0, &[0]).expect_err("the Pedal has no screens");
        assert!(matches!(error, ProtocolError::ScreenlessModel { .. }));
    }

    #[test]
    fn gen1_framing_is_refused_rather_than_guessed_at() {
        let original = identify(ELGATO_VENDOR_ID, 0x0060).expect("the original is catalogued");
        let error = key_image_packets(original, 0, &[0]).expect_err("gen 1 framing differs");
        assert!(matches!(
            error,
            ProtocolError::ImageFramingUnsupported { .. }
        ));
    }
}
