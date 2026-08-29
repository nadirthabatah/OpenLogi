//! Feature-code tests.

use super::*;

#[test]
fn every_named_feature_round_trips_through_its_code() {
    for feature in Feature::NAMED {
        assert_eq!(Feature::from_code(feature.code()), feature, "{feature:?}");
    }
}

#[test]
fn every_named_feature_has_a_name() {
    for feature in Feature::NAMED {
        assert!(feature.name().is_some(), "{feature:?} has no name");
    }
}

#[test]
fn an_unnamed_code_stays_unnamed_rather_than_borrowing_a_neighbours_name() {
    // 0xE2 is vendor territory. Naming it would put a confident, wrong word in
    // front of someone reading a monitor's feature list aloud.
    let feature = Feature::from_code(0xE2);

    assert_eq!(feature, Feature::Other(0xE2));
    assert_eq!(feature.name(), None);
    assert_eq!(feature.code(), 0xE2);
}

#[test]
fn the_named_list_has_no_duplicates() {
    let mut codes: Vec<u8> = Feature::NAMED.iter().map(|f| f.code()).collect();
    codes.sort_unstable();
    let before = codes.len();
    codes.dedup();

    assert_eq!(codes.len(), before, "two named features share a code");
}

#[test]
fn only_power_is_flagged_risky() {
    for feature in Feature::NAMED {
        assert_eq!(
            feature.is_risky(),
            feature == Feature::PowerMode,
            "{feature:?}"
        );
    }
}

#[test]
fn a_percentage_is_taken_against_the_monitors_own_maximum() {
    // A monitor whose brightness runs to 255, not 100. Reading 128 as "128
    // percent" is the bug this guards.
    let value = Value {
        current: 128,
        maximum: 255,
    };

    assert_eq!(value.percent(), Some(50));
}

#[test]
fn a_percentage_rounds_to_nearest() {
    assert_eq!(
        Value {
            current: 1,
            maximum: 3,
        }
        .percent(),
        Some(33)
    );
    assert_eq!(
        Value {
            current: 2,
            maximum: 3,
        }
        .percent(),
        Some(67)
    );
}

#[test]
fn a_maximum_of_zero_gives_no_percentage_rather_than_dividing_by_it() {
    let value = Value {
        current: 0,
        maximum: 0,
    };

    assert_eq!(value.percent(), None);
}

#[test]
fn a_current_above_the_maximum_reads_as_full_rather_than_over_full() {
    // Monitors that report this exist. "104 percent" helps nobody.
    let value = Value {
        current: 130,
        maximum: 125,
    };

    assert_eq!(value.percent(), Some(100));
}

#[test]
fn a_percentage_converts_back_against_the_maximum() {
    assert_eq!(Value::from_percent(50, 255), 128);
    assert_eq!(Value::from_percent(0, 255), 0);
    assert_eq!(Value::from_percent(100, 255), 255);
}

#[test]
fn a_percentage_above_a_hundred_is_clamped_before_it_is_scaled() {
    assert_eq!(Value::from_percent(200, 100), 100);
}

#[test]
fn every_named_input_round_trips_through_its_code() {
    for code in 0x01..=0x12_u8 {
        let input = InputSource::from_code(code);
        assert_eq!(input.code(), code);
        assert!(input.name().is_some(), "{code:#04x} has no name");
    }
}

#[test]
fn a_vendor_input_value_is_carried_through_without_being_named() {
    // 0x1b is USB-C on some panels and something else on others, which is
    // exactly why it must not acquire a name here.
    let input = InputSource::from_code(0x1B);

    assert_eq!(input, InputSource::Other(0x1B));
    assert_eq!(input.name(), None);
    assert_eq!(input.code(), 0x1B);
}

#[test]
fn an_input_name_parses_however_it_was_spelled_or_spoken() {
    for text in ["hdmi2", "HDMI-2", "hdmi 2", "HDMI_2", "Hdmi  2"] {
        assert_eq!(InputSource::parse(text), Some(InputSource::Hdmi2), "{text}");
    }
    for text in [
        "dp",
        "DP1",
        "displayport",
        "DisplayPort 1",
        "display port 1",
    ] {
        assert_eq!(
            InputSource::parse(text),
            Some(InputSource::DisplayPort1),
            "{text}"
        );
    }
}

#[test]
fn a_bare_connector_name_means_its_first_connector() {
    assert_eq!(InputSource::parse("hdmi"), Some(InputSource::Hdmi1));
    assert_eq!(InputSource::parse("dvi"), Some(InputSource::Dvi1));
    assert_eq!(InputSource::parse("vga"), Some(InputSource::Vga1));
}

#[test]
fn an_unknown_input_name_is_rejected_rather_than_guessed_at() {
    for text in ["", "usbc", "thunderbolt", "hdmi9", "     "] {
        assert_eq!(InputSource::parse(text), None, "{text}");
    }
}

#[test]
fn the_longest_input_name_still_fits_the_buffer_that_parses_it() {
    // `parse` squashes into a fixed buffer, so the bound has to be at least
    // the longest name it must accept. Shrink the buffer and this is the test
    // that notices, rather than a user finding DisplayPort unselectable.
    assert_eq!(
        InputSource::parse("display port 1"),
        Some(InputSource::DisplayPort1)
    );
    assert_eq!(
        InputSource::parse("DisplayPort-2"),
        Some(InputSource::DisplayPort2)
    );
}

#[test]
fn an_absurdly_long_input_name_is_rejected_rather_than_overrunning_the_buffer() {
    assert_eq!(InputSource::parse(&"hdmi2".repeat(10)), None);
    assert_eq!(InputSource::parse(&"a".repeat(1000)), None);
}

#[test]
fn every_named_power_mode_round_trips_through_its_code() {
    for code in 0x01..=0x05_u8 {
        let mode = PowerMode::from_code(code);
        assert_eq!(mode.code(), code);
        assert!(mode.name().is_some(), "{code:#04x} has no name");
    }
}

#[test]
fn hard_off_is_the_only_standard_mode_software_cannot_undo() {
    for code in 0x01..=0x05_u8 {
        let mode = PowerMode::from_code(code);
        assert_eq!(
            mode.is_recoverable(),
            mode != PowerMode::Off,
            "{mode:?} ({code:#04x})"
        );
    }
}

#[test]
fn an_unknown_power_mode_is_assumed_unrecoverable() {
    // Optimism is the wrong default here: the cost of being wrong is a
    // monitor that only a physical button brings back.
    assert!(!PowerMode::from_code(0x09).is_recoverable());
    assert_eq!(PowerMode::from_code(0x09).name(), None);
}
