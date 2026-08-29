use roadie_ddc::vcp::PowerMode;
use roadie_ddc::{Feature, Value};
use roadie_display::{DisplayError, Risk};
use serde_json::json;

use super::{feature_named, meaning, power_named, reading_json, setting_name, value_for};

#[test]
fn every_advertised_setting_name_round_trips() {
    // The enum in the tool schema and the parser behind it are two lists that
    // must not drift. A name the schema offers and the parser rejects is a
    // tool that fails only when a model does exactly what it was told.
    for setting in [
        "brightness",
        "contrast",
        "volume",
        "mute",
        "input_source",
        "power",
    ] {
        let feature = feature_named(setting).expect("the schema advertises this name");
        assert_eq!(
            setting_name(feature),
            setting,
            "{setting} must come back out the way it went in"
        );
    }
}

#[test]
fn an_unknown_setting_lists_the_ones_that_work() {
    let error = feature_named("loudness").expect_err("there is no such setting");
    assert!(error.contains("brightness"), "{error}");
    assert!(error.contains("input_source"), "{error}");
}

#[test]
fn every_advertised_power_name_parses() {
    // Same drift risk, and this list is the one that matters: "screen_off" and
    // "off" are different words for deliberately different things, and getting
    // them the wrong way round leaves someone with a dark screen and a walk to
    // the bezel.
    assert_eq!(power_named("on"), Some(PowerMode::On));
    assert_eq!(power_named("standby"), Some(PowerMode::Standby));
    assert_eq!(power_named("suspend"), Some(PowerMode::Suspend));
    assert_eq!(power_named("screen_off"), Some(PowerMode::ActiveOff));
    assert_eq!(power_named("off"), Some(PowerMode::Off));
    assert_eq!(power_named("dark"), None);
}

#[test]
fn powering_off_is_the_only_setting_that_needs_confirming() {
    // Every other write can be undone by writing it again. Treating more of
    // them as dangerous would train a caller to pass the confirmation without
    // reading it, which is how a confirmation stops working.
    assert_eq!(
        Risk::of(Feature::PowerMode, u16::from(PowerMode::Off.code())),
        Some(Risk::PowerOff)
    );
    assert_eq!(
        Risk::of(Feature::PowerMode, u16::from(PowerMode::ActiveOff.code())),
        None
    );
    assert_eq!(Risk::of(Feature::Brightness, 100), None);
    assert_eq!(Risk::of(Feature::Volume, 0), None);
}

#[test]
fn a_value_is_accepted_as_a_number_a_name_or_a_percentage() {
    let before = Some(Value {
        current: 40,
        maximum: 80,
    });
    assert_eq!(
        value_for(Feature::Brightness, &json!(40), before),
        Ok(40),
        "a plain number goes through untouched"
    );
    assert_eq!(
        value_for(Feature::InputSource, &json!("hdmi2"), before),
        Ok(0x12)
    );
    assert_eq!(
        value_for(Feature::PowerMode, &json!("standby"), before),
        Ok(0x02)
    );
    assert_eq!(value_for(Feature::Mute, &json!("unmute"), before), Ok(0x02));
    // Half of this monitor's own maximum, which is 80 rather than 100.
    assert_eq!(
        value_for(Feature::Brightness, &json!("50%"), before),
        Ok(40)
    );
}

#[test]
fn a_percentage_with_nothing_to_scale_it_against_is_refused() {
    let error = value_for(Feature::Brightness, &json!("50%"), None)
        .expect_err("there is no maximum to take a percentage of");
    assert!(
        error.contains("read_display_settings"),
        "the error has to say what to call instead: {error}"
    );
}

#[test]
fn a_value_too_wide_for_the_wire_is_refused_rather_than_truncated() {
    let error = value_for(Feature::Brightness, &json!(70000), None)
        .expect_err("a monitor setting is sixteen bits");
    assert!(error.contains("too large"), "{error}");
}

#[test]
fn a_reading_carries_the_maximum_as_well_as_the_value() {
    // The percentage alone is not enough to write a value back with, because
    // the maximum is not always 100.
    let reading = reading_json(
        Feature::Brightness,
        Ok(Value {
            current: 40,
            maximum: 80,
        }),
    );
    assert_eq!(reading["setting"], "brightness");
    assert_eq!(reading["value"], 40);
    assert_eq!(reading["maximum"], 80);
    assert_eq!(reading["percent"], 50);
}

#[test]
fn a_feature_the_monitor_lacks_is_reported_rather_than_dropped() {
    // "This panel has no speakers" is a useful answer to "turn the volume
    // down". An absent key would leave that to be guessed at.
    let reading = reading_json(
        Feature::Volume,
        Err(DisplayError::Unsupported { platform: "linux" }),
    );
    assert_eq!(reading["setting"], "volume");
    assert!(
        reading["unavailable"].is_string(),
        "the reason has to survive into the result: {reading}"
    );
}

#[test]
fn a_coded_value_is_given_its_meaning() {
    assert_eq!(
        meaning(
            Feature::InputSource,
            Value {
                current: 0x11,
                maximum: 0x12
            }
        )
        .as_deref(),
        Some("HDMI 1")
    );
    assert_eq!(
        meaning(
            Feature::PowerMode,
            Value {
                current: 0x04,
                maximum: 0x05
            }
        )
        .as_deref(),
        Some("screen off")
    );
    // Above the standard table is vendor territory, and a wrong name would be
    // worse than none.
    assert_eq!(
        meaning(
            Feature::InputSource,
            Value {
                current: 0x1B,
                maximum: 0x1C
            }
        ),
        None
    );
    // Brightness is a magnitude, not a code, so it has no meaning to give.
    assert_eq!(
        meaning(
            Feature::Brightness,
            Value {
                current: 50,
                maximum: 100
            }
        ),
        None
    );
}
