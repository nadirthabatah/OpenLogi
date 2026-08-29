use roadie_ddc::Feature;

use super::{Acknowledged, Risk};

#[test]
fn ordinary_writes_carry_no_risk() {
    // Brightness at both ends of its range, and the input source: the three
    // writes a person actually makes. None of them should ever stop to ask.
    assert_eq!(Risk::of(Feature::Brightness, 0), None);
    assert_eq!(Risk::of(Feature::Brightness, 100), None);
    assert_eq!(Risk::of(Feature::InputSource, 0x11), None);
    assert_eq!(Risk::of(Feature::Volume, 50), None);
    assert_eq!(Risk::of(Feature::Contrast, 75), None);
}

#[test]
fn recoverable_power_states_carry_no_risk() {
    // 0x01 on, 0x02 standby, 0x03 suspend, 0x04 active off. Every one of them
    // leaves the monitor listening, so software can undo it.
    for code in 0x01..=0x04 {
        assert_eq!(
            Risk::of(Feature::PowerMode, code),
            None,
            "power state {code:#04x} is recoverable and should not need confirming"
        );
    }
}

#[test]
fn hard_off_is_refused() {
    // 0x05 is the one that can stop the monitor answering at all.
    assert_eq!(Risk::of(Feature::PowerMode, 0x05), Some(Risk::PowerOff));
}

#[test]
fn unknown_power_states_are_assumed_unrecoverable() {
    // Anything outside the standard table is vendor territory. Being
    // optimistic about it costs someone a walk to the bezel.
    assert_eq!(
        Risk::of(Feature::PowerMode, 0x09),
        Some(Risk::UnknownPowerState(0x09))
    );
    // A value too wide for the wire is not a power state either, and must not
    // fall through the byte conversion into being waved past.
    assert_eq!(
        Risk::of(Feature::PowerMode, 0x0100),
        Some(Risk::UnknownPowerState(0))
    );
}

#[test]
fn acknowledgement_names_what_was_accepted() {
    let acknowledged = Acknowledged::of(Risk::PowerOff);
    assert_eq!(acknowledged.risk(), Risk::PowerOff);
}

#[test]
fn spoken_warnings_are_listenable() {
    // These sentences are read aloud, so the rules that apply to every other
    // output apply here: no box drawing, no bare symbols, no "(s)".
    for risk in [
        Risk::PowerOff,
        Risk::UnknownPowerState(0x09),
        Risk::SaveSettings,
    ] {
        let spoken = risk.spoken();
        assert!(
            !spoken.contains("(s)"),
            "an optional plural is unlistenable: {spoken}"
        );
        assert!(
            spoken.ends_with('.'),
            "a spoken warning is a sentence and needs its full stop: {spoken}"
        );
        assert!(
            !spoken.contains("  "),
            "a doubled space is a line-continuation artefact: {spoken}"
        );
    }
}

#[test]
fn power_off_warning_names_the_way_back() {
    // The whole point of the sentence: someone who cannot see the screen needs
    // to be told that the way back is physical, before the screen goes dark.
    let spoken = Risk::PowerOff.spoken();
    assert!(
        spoken.contains("power button on the monitor"),
        "the warning must name the physical button: {spoken}"
    );
}
