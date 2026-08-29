//! Framing tests.
//!
//! The byte sequences below are written out in full rather than computed from
//! the code under test. A test that builds its expectation with the same
//! function it is checking proves only that the function agrees with itself,
//! and the checksum rule is exactly the kind of constant that would sail
//! through such a test while being wrong on every real monitor.
//!
//! `51 82 01 10 AC` — read brightness — is the sequence that appears in DDC/CI
//! documentation and in `ddcutil`'s own traces, so it is an outside check on
//! the request framing and the request seed together.

use super::*;
use crate::vcp::{Feature, InputSource, PowerMode};

#[test]
fn reading_brightness_is_the_documented_byte_sequence() {
    let frame = Request::Get(Feature::Brightness).frame();

    assert_eq!(frame.as_bytes(), [0x51, 0x82, 0x01, 0x10, 0xAC]);
}

#[test]
fn a_request_does_not_carry_the_display_address_it_is_checksummed_with() {
    let frame = Request::Get(Feature::Brightness).frame();

    // 0x6E is in the checksum and nowhere else: the I2C layer puts it on the
    // wire, so writing it here too would send it twice.
    assert!(!frame.as_bytes().contains(&DISPLAY_ADDRESS));
}

#[test]
fn writing_a_feature_frames_the_value_big_endian() {
    let frame = Request::Set {
        feature: Feature::Brightness,
        value: 50,
    }
    .frame();

    assert_eq!(frame.as_bytes(), [0x51, 0x84, 0x03, 0x10, 0x00, 0x32, 0x9A]);
}

#[test]
fn a_value_above_255_survives_framing() {
    let frame = Request::Set {
        feature: Feature::Other(0xE2),
        value: 0x1234,
    }
    .frame();

    assert_eq!(&frame.as_bytes()[2..6], [0x03, 0xE2, 0x12, 0x34]);
}

#[test]
fn switching_input_frames_the_input_code() {
    let frame = Request::Set {
        feature: Feature::InputSource,
        value: u16::from(InputSource::Hdmi2.code()),
    }
    .frame();

    assert_eq!(frame.as_bytes(), [0x51, 0x84, 0x03, 0x60, 0x00, 0x12, 0xCA]);
}

#[test]
fn saving_settings_is_a_one_byte_payload() {
    let frame = Request::SaveSettings.frame();

    assert_eq!(frame.as_bytes(), [0x51, 0x81, 0x0C, 0xB2]);
}

#[test]
fn a_capability_request_frames_its_offset() {
    let frame = Request::Capabilities { offset: 32 }.frame();

    assert_eq!(frame.as_bytes(), [0x51, 0x83, 0xF3, 0x00, 0x20, 0x6F]);
}

#[test]
fn a_brightness_reply_carries_current_and_maximum() {
    let bytes = [
        0x6E, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0xF2,
    ];

    let reply = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap();

    assert_eq!(
        reply,
        Reply::Feature {
            feature: Feature::Brightness,
            momentary: false,
            value: Value {
                current: 50,
                maximum: 100,
            },
        }
    );
}

#[test]
fn a_reply_checksummed_like_a_request_is_rejected() {
    // The same brightness reply, checksummed with 0x6e — the request seed —
    // instead of 0x50. A host that used one seed for both directions would
    // accept this, and would then reject every reply a real monitor sends.
    let mut bytes = [
        0x6E, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0xF2,
    ];
    bytes[10] ^= VIRTUAL_HOST_ADDRESS ^ DISPLAY_ADDRESS;

    let error = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap_err();

    assert!(matches!(error, ProtocolError::Checksum { .. }), "{error:?}");
}

#[test]
fn a_reply_naming_a_different_feature_is_rejected() {
    // Contrast's answer arriving where brightness's was expected: what a read
    // that outran the monitor actually looks like.
    let bytes = [
        0x6E, 0x88, 0x02, 0x00, 0x12, 0x00, 0x00, 0x64, 0x00, 0x50, 0x92,
    ];

    let error = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap_err();

    assert_eq!(
        error,
        ProtocolError::WrongFeature {
            asked: 0x10,
            answered: 0x12,
        }
    );
}

#[test]
fn an_unsupported_feature_says_so_rather_than_reading_zero() {
    let bytes = [
        0x6E, 0x88, 0x02, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA5,
    ];

    let error = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap_err();

    assert_eq!(error, ProtocolError::Unsupported { feature: 0x10 });
}

#[test]
fn a_null_message_is_its_own_error() {
    let bytes = [0x6E, 0x80, 0xBE];

    let error = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap_err();

    // Distinct from a fault: the caller's answer is to wait, not to give up.
    assert_eq!(error, ProtocolError::Null);
}

#[test]
fn a_reply_from_the_wrong_address_is_rejected() {
    let bytes = [0x6C, 0x80, 0xBC];

    let error = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap_err();

    assert_eq!(error, ProtocolError::NotFromDisplay { address: 0x6C });
}

#[test]
fn a_length_byte_without_its_high_bit_is_rejected() {
    let bytes = [
        0x6E, 0x08, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0x72,
    ];

    let error = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap_err();

    assert_eq!(error, ProtocolError::MalformedLength { byte: 0x08 });
}

#[test]
fn a_reply_claiming_more_than_it_carries_is_rejected() {
    let bytes = [0x6E, 0x88, 0x02, 0x00, 0x10, 0xF2];

    let error = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap_err();

    assert_eq!(
        error,
        ProtocolError::Truncated {
            declared: 8,
            available: 3,
        }
    );
}

#[test]
fn a_reply_too_short_to_have_framing_is_rejected() {
    for len in 0..3_usize {
        let bytes = [0x6E, 0x80, 0xBE];

        let error = Request::Get(Feature::Brightness)
            .parse_reply(&bytes[..len])
            .unwrap_err();

        assert_eq!(error, ProtocolError::TooShort { len }, "at length {len}");
    }
}

#[test]
fn a_capability_fragment_carries_its_offset_and_bytes() {
    let bytes = [
        0x6E, 0x91, 0xE3, 0x00, 0x00, 0x28, 0x70, 0x72, 0x6F, 0x74, 0x28, 0x6D, 0x6F, 0x6E, 0x69,
        0x74, 0x6F, 0x72, 0x29, 0x10,
    ];

    let reply = Request::Capabilities { offset: 0 }
        .parse_reply(&bytes)
        .unwrap();

    assert_eq!(
        reply,
        Reply::Capabilities {
            offset: 0,
            fragment: b"(prot(monitor)",
        }
    );
}

#[test]
fn the_last_capability_fragment_is_empty() {
    let bytes = [0x6E, 0x83, 0xE3, 0x00, 0x2A, 0x74];

    let reply = Request::Capabilities { offset: 42 }
        .parse_reply(&bytes)
        .unwrap();

    assert_eq!(
        reply,
        Reply::Capabilities {
            offset: 42,
            fragment: b"",
        }
    );
}

#[test]
fn a_capability_fragment_for_the_wrong_offset_is_rejected() {
    let bytes = [0x6E, 0x83, 0xE3, 0x00, 0x2A, 0x74];

    let error = Request::Capabilities { offset: 0 }
        .parse_reply(&bytes)
        .unwrap_err();

    assert_eq!(
        error,
        ProtocolError::WrongOffset {
            asked: 0,
            answered: 42,
        }
    );
}

#[test]
fn a_feature_reply_to_a_capability_request_is_rejected() {
    let bytes = [
        0x6E, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0xF2,
    ];

    let error = Request::Capabilities { offset: 0 }
        .parse_reply(&bytes)
        .unwrap_err();

    assert_eq!(error, ProtocolError::UnexpectedOpcode { opcode: 0x02 });
}

#[test]
fn writes_are_not_answered_and_parsing_one_says_so() {
    let bytes = [
        0x6E, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0xF2,
    ];

    for request in [
        Request::Set {
            feature: Feature::Brightness,
            value: 50,
        },
        Request::SaveSettings,
    ] {
        assert!(!request.expects_reply(), "{request:?}");
        assert_eq!(
            request.parse_reply(&bytes).unwrap_err(),
            ProtocolError::NotAnswered,
            "{request:?}"
        );
    }
}

#[test]
fn reads_are_answered() {
    assert!(Request::Get(Feature::Brightness).expects_reply());
    assert!(Request::Capabilities { offset: 0 }.expects_reply());
}

#[test]
fn a_momentary_control_is_reported_as_one() {
    // Type byte 0x01 rather than 0x00: the monitor calls this a control that
    // acts when written and has no resting value.
    let mut bytes = [
        0x6E, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0xF2,
    ];
    bytes[5] = 0x01;
    bytes[10] ^= 0x01;

    let reply = Request::Get(Feature::Brightness)
        .parse_reply(&bytes)
        .unwrap();

    assert!(
        matches!(
            reply,
            Reply::Feature {
                momentary: true,
                ..
            }
        ),
        "{reply:?}"
    );
}

#[test]
fn every_request_waits_at_least_the_specifications_floor() {
    // Named individually rather than checked as "greater than zero": a
    // rounding of these numbers down to something convenient is exactly the
    // change that would make monitors seem flaky, and it should fail here.
    assert_eq!(Request::Get(Feature::Brightness).settle().as_millis(), 40);
    assert_eq!(
        Request::Set {
            feature: Feature::Brightness,
            value: 1,
        }
        .settle()
        .as_millis(),
        50
    );
    assert_eq!(Request::Capabilities { offset: 0 }.settle().as_millis(), 50);
    assert_eq!(Request::SaveSettings.settle().as_millis(), 200);
}

#[test]
fn the_power_feature_is_framed_from_its_mode() {
    let frame = Request::Set {
        feature: Feature::PowerMode,
        value: u16::from(PowerMode::ActiveOff.code()),
    }
    .frame();

    assert_eq!(&frame.as_bytes()[2..6], [0x03, 0xD6, 0x00, 0x04]);
}
