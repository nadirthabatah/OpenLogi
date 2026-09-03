use super::{
    FRAME_BODY, FRAME_LEN, FrameError, Reassembly, frames, is_neo, read_request, write_request,
};

/// The layout is a wire contract, so it is spelled out in literal bytes
/// rather than recomputed from the code under test.
#[test]
fn a_short_message_is_one_frame_with_the_documented_layout() {
    let sent = frames(b"GET /elgato/lights").expect("one frame");
    assert_eq!(sent.len(), 1);
    let frame = sent[0];
    assert_eq!(frame.len(), FRAME_LEN);
    assert_eq!(frame[0], 0x02, "start marker");
    assert_eq!(frame[1], 0, "frame index");
    assert_eq!(frame[2], 1, "frame count");
    assert_eq!(frame[3], 0x03, "header marker");
    assert_eq!(frame[4], 18, "body length, low byte");
    assert_eq!(frame[5], 0, "body length, high byte");
    assert_eq!(&frame[6..24], b"GET /elgato/lights");
    assert_eq!(frame[24], 0x03, "closing marker");
    assert!(frame[25..].iter().all(|&byte| byte == 0), "zero padding");
}

#[test]
fn an_empty_message_is_still_one_well_formed_frame() {
    let sent = frames(b"").expect("one frame");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0][2], 1, "an empty message is one frame, not zero");
    assert_eq!(sent[0][4..6], [0, 0], "with no body");
    assert_eq!(sent[0][6], 0x03, "and the closing marker right after");
}

#[test]
fn a_long_message_spans_frames_and_reassembles_to_itself() {
    // Long enough for three frames, with content that catches reordering.
    let message: Vec<u8> = (0..(FRAME_BODY * 2 + 100))
        .map(|i| u8::try_from(i % 251).unwrap_or(0))
        .collect();
    let sent = frames(&message).expect("three frames");
    assert_eq!(sent.len(), 3);
    assert!(
        sent.iter().enumerate().all(|(i, f)| usize::from(f[1]) == i),
        "indexes count up"
    );
    assert!(
        sent.iter().all(|f| f[2] == 3),
        "every frame carries the count"
    );

    let mut reassembly = Reassembly::new();
    assert_eq!(reassembly.accept(&sent[0]).expect("accepted"), None);
    assert_eq!(reassembly.accept(&sent[1]).expect("accepted"), None);
    let whole = reassembly
        .accept(&sent[2])
        .expect("accepted")
        .expect("complete");
    assert_eq!(whole, message);
}

/// HID reports owe nobody an ordering guarantee, and a frame index exists
/// precisely so arrival order does not matter.
#[test]
fn frames_arriving_out_of_order_still_reassemble_in_order() {
    let message: Vec<u8> = (0..(FRAME_BODY + 10))
        .map(|i| u8::try_from(i % 256).unwrap_or(0))
        .collect();
    let sent = frames(&message).expect("two frames");
    let mut reassembly = Reassembly::new();
    assert_eq!(reassembly.accept(&sent[1]).expect("accepted"), None);
    let whole = reassembly
        .accept(&sent[0])
        .expect("accepted")
        .expect("complete");
    assert_eq!(whole, message);
}

#[test]
fn a_duplicated_frame_is_kept_once() {
    let message: Vec<u8> = (0..(FRAME_BODY + 10))
        .map(|i| u8::try_from(i % 256).unwrap_or(0))
        .collect();
    let sent = frames(&message).expect("two frames");
    let mut reassembly = Reassembly::new();
    assert_eq!(reassembly.accept(&sent[0]).expect("accepted"), None);
    assert_eq!(reassembly.accept(&sent[0]).expect("accepted"), None);
    let whole = reassembly
        .accept(&sent[1])
        .expect("accepted")
        .expect("complete");
    assert_eq!(whole, message);
}

#[test]
fn a_completed_reassembly_is_ready_for_the_next_message() {
    let mut reassembly = Reassembly::new();
    let first = frames(b"one").expect("frames");
    let second = frames(b"two").expect("frames");
    assert_eq!(
        reassembly.accept(&first[0]).expect("accepted"),
        Some(b"one".to_vec())
    );
    assert_eq!(
        reassembly.accept(&second[0]).expect("accepted"),
        Some(b"two".to_vec())
    );
}

#[test]
fn malformed_frames_are_named_not_guessed_at() {
    let mut reassembly = Reassembly::new();
    assert_eq!(
        reassembly.accept(&[0x02, 0, 1]),
        Err(FrameError::Short { got: 3 })
    );
    let mut wrong_start = frames(b"x").expect("frames")[0];
    wrong_start[0] = 0x55;
    assert_eq!(
        reassembly.accept(&wrong_start),
        Err(FrameError::BadStart { found: 0x55 })
    );
    // A length claim larger than the frame would read past the buffer.
    let mut overrun = frames(b"x").expect("frames")[0];
    overrun[4..6].copy_from_slice(&2000_u16.to_le_bytes());
    assert_eq!(
        reassembly.accept(&overrun),
        Err(FrameError::LengthOverrun {
            claimed: 2000,
            carried: FRAME_LEN - 6,
        })
    );
}

/// The byte after the counters is not judged on the way in. Requests carry
/// 0x03 there, and the Key Light Neo on this project's desk answers with
/// 0x00 — a frame refused over it would refuse the actual hardware.
#[test]
fn a_reply_whose_type_byte_is_zero_is_still_read() {
    let mut frame = frames(b"{}").expect("frames")[0];
    frame[3] = 0x00;
    let mut reassembly = Reassembly::new();
    assert_eq!(
        reassembly.accept(&frame).expect("accepted"),
        Some(b"{}".to_vec())
    );
}

/// A reply interleaved with another message's frames is refused rather than
/// stitched into a franken-message.
#[test]
fn a_mid_message_change_of_frame_count_is_refused() {
    let long: Vec<u8> = vec![7; FRAME_BODY + 1];
    let sent = frames(&long).expect("two frames");
    let mut reassembly = Reassembly::new();
    assert_eq!(reassembly.accept(&sent[0]).expect("accepted"), None);
    let stray = frames(b"other").expect("frames");
    assert_eq!(
        reassembly.accept(&stray[0]),
        Err(FrameError::TotalChanged { had: 2, found: 1 })
    );
}

#[test]
fn a_message_too_long_for_the_format_is_refused_with_its_size() {
    let huge = vec![0_u8; FRAME_BODY * 256];
    assert_eq!(
        frames(&huge),
        Err(FrameError::MessageTooLong {
            bytes: FRAME_BODY * 256
        })
    );
}

#[test]
fn the_request_lines_read_as_the_firmware_expects() {
    assert_eq!(read_request("/elgato/lights"), b"GET /elgato/lights");
    assert_eq!(
        write_request("/elgato/lights", r#"{"lights":[{"on":1}]}"#),
        br#"PUT /elgato/lights {"lights":[{"on":1}]}"#
    );
}

/// The identity is a wire contract: the ids are spelled out, not imported.
#[test]
fn the_neo_is_recognised_by_its_full_identity_and_nothing_close_to_it() {
    assert!(is_neo(0x0fd9, 0x00a0, 0x000c, 0x0001));
    assert!(!is_neo(0x0fd8, 0x00a0, 0x000c, 0x0001), "wrong vendor");
    assert!(!is_neo(0x0fd9, 0x00a1, 0x000c, 0x0001), "wrong product");
    assert!(!is_neo(0x0fd9, 0x00a0, 0x000d, 0x0001), "wrong usage page");
    assert!(!is_neo(0x0fd9, 0x00a0, 0x000c, 0x0002), "wrong usage");
}
