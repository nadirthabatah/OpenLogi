//! The bytes are spelled out literally here rather than computed from the
//! code under test. A test that builds its expectation with [`Event::byte`]
//! would agree with [`decode`] however wrong both were; these numbers come
//! from captures of the device published by other drivers, and are the only
//! independent check this crate has short of hardware.

use super::*;
use std::collections::HashMap;

/// Every button, pressed. The low byte is the control code and the press
/// bits are zero, so a press byte is the bare control code.
#[test]
fn every_button_press_decodes_to_the_byte_the_device_sends() {
    let expected = [
        (0x00, Button::Tall),
        (0x01, Button::Side),
        (0x02, Button::Top),
        (0x03, Button::Short),
        (0x0a, Button::Scroll),
        (0x10, Button::Up),
        (0x11, Button::Down),
        (0x12, Button::Left),
        (0x13, Button::Right),
        (0x22, Button::C1),
        (0x23, Button::C2),
        (0x2a, Button::Tour),
        (0x37, Button::Knob),
        (0x38, Button::Dial),
    ];
    for (byte, button) in expected {
        assert_eq!(
            decode(byte).expect("a published press byte decodes"),
            Event::Button {
                button,
                action: ButtonAction::Pressed
            },
            "byte {byte:#04x}"
        );
    }
}

/// Every button, released. A release sets bit 7 and nothing else.
#[test]
fn every_button_release_decodes_to_the_byte_the_device_sends() {
    let expected = [
        (0x80, Button::Tall),
        (0x81, Button::Side),
        (0x82, Button::Top),
        (0x83, Button::Short),
        (0x8a, Button::Scroll),
        (0x90, Button::Up),
        (0x91, Button::Down),
        (0x92, Button::Left),
        (0x93, Button::Right),
        (0xa2, Button::C1),
        (0xa3, Button::C2),
        (0xaa, Button::Tour),
        (0xb7, Button::Knob),
        (0xb8, Button::Dial),
    ];
    for (byte, button) in expected {
        assert_eq!(
            decode(byte).expect("a published release byte decodes"),
            Event::Button {
                button,
                action: ButtonAction::Released
            },
            "byte {byte:#04x}"
        );
    }
}

/// The three wheels, both directions. Clockwise sets bit 6.
#[test]
fn every_wheel_turn_decodes_to_the_byte_the_device_sends() {
    let expected = [
        (0x44, Wheel::Knob, Turn::Clockwise),
        (0x04, Wheel::Knob, Turn::CounterClockwise),
        (0x49, Wheel::Scroll, Turn::Clockwise),
        (0x09, Wheel::Scroll, Turn::CounterClockwise),
        (0x4f, Wheel::Dial, Turn::Clockwise),
        (0x0f, Wheel::Dial, Turn::CounterClockwise),
    ];
    for (byte, wheel, direction) in expected {
        assert_eq!(
            decode(byte).expect("a published turn byte decodes"),
            Event::Turn {
                wheel,
                direction,
                phase: TurnPhase::Moving
            },
            "byte {byte:#04x}"
        );
    }
}

/// The end-of-turn markers, which an earlier version of this crate refused
/// as impossible. Each is its direction's byte with the high bit set, and
/// every one of these is named in a published driver's constants — the
/// `_STOP` family — with one of them also printed from live hardware by a
/// second driver. Refusing them would have produced an error at the end of
/// every single turn.
#[test]
fn every_wheel_reports_the_end_of_a_turn() {
    let expected = [
        (0xc4, Wheel::Knob, Turn::Clockwise),
        (0x84, Wheel::Knob, Turn::CounterClockwise),
        (0xc9, Wheel::Scroll, Turn::Clockwise),
        (0x89, Wheel::Scroll, Turn::CounterClockwise),
        (0xcf, Wheel::Dial, Turn::Clockwise),
        (0x8f, Wheel::Dial, Turn::CounterClockwise),
    ];
    for (byte, wheel, direction) in expected {
        assert_eq!(
            decode(byte).expect("a published stop byte decodes"),
            Event::Turn {
                wheel,
                direction,
                phase: TurnPhase::Ended
            },
            "byte {byte:#04x}"
        );
    }
}

/// The end marker keeps the direction of the turn it ends, rather than
/// being one shared "stopped" byte. That is what lets a caller holding a
/// modifier down for a clockwise turn release the right one.
#[test]
fn the_end_of_a_turn_remembers_which_way_it_was_going() {
    let clockwise = decode(0xc4).expect("knob clockwise stop decodes");
    let anticlockwise = decode(0x84).expect("knob counter-clockwise stop decodes");
    assert_ne!(clockwise, anticlockwise);
    assert_eq!(clockwise.describe(), "knob stopped turning clockwise");
    assert_eq!(
        anticlockwise.describe(),
        "knob stopped turning counter-clockwise"
    );
}

/// Pressing a wheel is a different control from turning it, and the two
/// carry different codes. Getting this wrong would make every knob press
/// read as a turn, which is the single most likely way to mis-transcribe
/// this protocol.
#[test]
fn pressing_a_wheel_is_not_turning_it() {
    assert_eq!(Button::Knob.code(), 0x37);
    assert_eq!(Wheel::Knob.code(), 0x04);
    assert_eq!(Button::Scroll.code(), 0x0a);
    assert_eq!(Wheel::Scroll.code(), 0x09);
    assert_eq!(Button::Dial.code(), 0x38);
    assert_eq!(Wheel::Dial.code(), 0x0f);
}

/// The disputed byte. One published source records the knob press as
/// `0x77`, which would make it the only control that sets a turn bit while
/// being pressed. This build rejects it by name rather than delivering a
/// keystroke on a byte it cannot explain. If hardware ever produces `0x77`,
/// this is the test that has to change, and the error message is what will
/// have said so.
#[test]
fn the_disputed_knob_byte_is_refused_rather_than_guessed_at() {
    let error = decode(0x77).expect_err("0x77 is not a byte this build accepts");
    assert_eq!(
        error,
        ProtocolError::ImpossibleAction {
            control: "pressing the knob",
            action: "a turn",
            byte: 0x77,
        }
    );
    assert_eq!(
        error.to_string(),
        "byte 0x77 names pressing the knob, but its action bits say a turn, \
         and no control does both"
    );
}

/// A wheel has no impossible action: both of its bits are meaningful and
/// independent, so all four combinations decode. This is the assertion that
/// replaced one claiming the opposite, and it is here so that a future
/// change reintroducing the refusal has to argue with a test.
#[test]
fn no_byte_belonging_to_a_wheel_is_ever_refused() {
    for wheel_code in [0x04u8, 0x09, 0x0f] {
        for action in [0x00u8, 0x40, 0x80, 0xc0] {
            let byte = wheel_code | action;
            decode(byte).unwrap_or_else(|error| {
                panic!("byte {byte:#04x} belongs to a wheel but was refused: {error}")
            });
        }
    }
}

/// A control code no model in this build carries. `0x3f` is the largest
/// six-bit value and belongs to nothing.
#[test]
fn an_unknown_control_names_the_byte_it_could_not_explain() {
    let error = decode(0x3f).expect_err("0x3f names no control");
    assert_eq!(
        error,
        ProtocolError::UnknownControl {
            control: 0x3f,
            byte: 0x3f,
        }
    );
}

/// The error has to carry the whole byte and not only the masked control,
/// because a bug report is useless without what actually arrived.
#[test]
fn an_unknown_control_reports_the_whole_byte_not_just_the_masked_half() {
    let error = decode(0xbf).expect_err("0xbf names no control either");
    assert_eq!(
        error,
        ProtocolError::UnknownControl {
            control: 0x3f,
            byte: 0xbf,
        }
    );
}

/// Two controls sharing a byte would make one of them undeliverable, and
/// nothing else in this crate would notice. Checked across every event the
/// type system can build.
#[test]
fn no_two_events_are_sent_as_the_same_byte() {
    let mut seen: HashMap<u8, Event> = HashMap::new();
    for event in all_events() {
        if let Some(other) = seen.insert(event.byte(), event) {
            panic!(
                "{:?} and {:?} both encode to {:#04x}",
                other,
                event,
                event.byte()
            );
        }
    }
    assert_eq!(seen.len(), 14 * 2 + 3 * 2 * 2);
}

/// Encoding and decoding are inverses for everything the device can send.
#[test]
fn every_event_survives_a_round_trip() {
    for event in all_events() {
        assert_eq!(
            decode(event.byte()).expect("an event this crate encodes decodes"),
            event
        );
    }
}

/// Bytes that decode at all are exactly the ones an event encodes to.
/// Sweeping all 256 catches a decoder that accepts more than it should —
/// the failure mode where a mis-masked branch quietly swallows corruption.
#[test]
fn no_byte_outside_the_known_set_is_accepted() {
    let known: Vec<u8> = all_events().map(Event::byte).collect();
    for byte in 0u8..=255 {
        let decoded = decode(byte);
        assert_eq!(
            decoded.is_ok(),
            known.contains(&byte),
            "byte {byte:#04x} decoded to {decoded:?}"
        );
    }
}

/// What a screen reader is handed. No symbols, no abbreviations that only
/// work on a screen.
#[test]
fn an_event_describes_itself_in_words() {
    assert_eq!(
        decode(0x01)
            .expect("side press decodes")
            .describe()
            .as_str(),
        "side button pressed"
    );
    assert_eq!(
        decode(0xaa)
            .expect("tour release decodes")
            .describe()
            .as_str(),
        "tour button released"
    );
    assert_eq!(
        decode(0x44).expect("knob turn decodes").describe().as_str(),
        "knob turned clockwise"
    );
    assert_eq!(
        decode(0x0f).expect("dial turn decodes").describe().as_str(),
        "dial turned counter-clockwise"
    );
}

/// Pressing a wheel and turning it are told apart by the verb, not by the
/// noun, so both readings have to be checked side by side. Getting this
/// wrong is not a wrong answer but an unlistenable one.
#[test]
fn pressing_a_wheel_and_turning_it_read_differently() {
    assert_eq!(
        decode(0x0a).expect("scroll press decodes").describe(),
        "scroll wheel pressed"
    );
    assert_eq!(
        decode(0x49).expect("scroll turn decodes").describe(),
        "scroll wheel turned clockwise"
    );
    assert_eq!(
        decode(0x37).expect("knob press decodes").describe(),
        "knob pressed"
    );
    assert_eq!(
        decode(0x04).expect("knob turn decodes").describe(),
        "knob turned counter-clockwise"
    );
}

/// The bug this guards: a control name that already contained a verb
/// produced "scroll wheel press pressed", which is what a screen reader
/// would have said out loud. No name may end in a word the action is about
/// to repeat.
#[test]
fn no_description_says_its_verb_twice() {
    for event in all_events() {
        let described = event.describe();
        assert!(
            !described.contains("press pressed")
                && !described.contains("press released")
                && !described.contains("turned turned"),
            "{described}"
        );
        for verb in ["pressed", "released", "turned"] {
            assert_eq!(
                described.matches(verb).count(),
                usize::from(described.contains(verb)),
                "{described} says {verb} more than once"
            );
        }
    }
}

/// The setup message is a fixed length the device rejects if it is wrong,
/// and its first and last bytes frame it.
#[test]
fn the_setup_message_is_framed_as_the_device_expects() {
    assert_eq!(SETUP_MESSAGE.len(), 94);
    assert_eq!(SETUP_MESSAGE[0], 0xb5);
    assert_eq!(SETUP_MESSAGE[93], 0xfe);
}

/// Every event the device can produce, built from the type system rather
/// than from a list that could drift out of step with the enums.
fn all_events() -> impl Iterator<Item = Event> {
    const BUTTONS: [Button; 14] = [
        Button::Tall,
        Button::Side,
        Button::Top,
        Button::Short,
        Button::Scroll,
        Button::Up,
        Button::Down,
        Button::Left,
        Button::Right,
        Button::C1,
        Button::C2,
        Button::Tour,
        Button::Knob,
        Button::Dial,
    ];
    const WHEELS: [Wheel; 3] = [Wheel::Knob, Wheel::Scroll, Wheel::Dial];

    let buttons = BUTTONS.into_iter().flat_map(|button| {
        [ButtonAction::Pressed, ButtonAction::Released]
            .into_iter()
            .map(move |action| Event::Button { button, action })
    });
    let wheels = WHEELS.into_iter().flat_map(|wheel| {
        [Turn::Clockwise, Turn::CounterClockwise]
            .into_iter()
            .flat_map(move |direction| {
                [TurnPhase::Moving, TurnPhase::Ended]
                    .into_iter()
                    .map(move |phase| Event::Turn {
                        wheel,
                        direction,
                        phase,
                    })
            })
    });
    buttons.chain(wheels)
}
