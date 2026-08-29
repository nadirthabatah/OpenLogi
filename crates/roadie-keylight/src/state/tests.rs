use super::{
    BRIGHTNESS, Light, LightError, Lights, Range, TEMPERATURE, kelvin_range, kelvin_to_mired,
    mired_to_kelvin,
};

/// A light in the middle of its range, as one arrives from the device.
fn light() -> Light {
    Light {
        on: 1,
        brightness: 20,
        temperature: 213,
    }
}

#[test]
fn the_wire_shape_is_exactly_what_the_firmware_sends() {
    // Spelled out rather than round-tripped through our own serialiser: the
    // field names and the numeric `on` are the firmware's, and a test that
    // encoded with the code under test would agree with it about a rename.
    let json = r#"{"numberOfLights":1,"lights":[{"on":1,"brightness":20,"temperature":213}]}"#;
    let parsed: Lights = serde_json::from_str(json).expect("the documented shape");
    assert_eq!(parsed.number_of_lights, 1);
    assert_eq!(parsed.first().expect("one light"), light());

    // And back out again in the same shape, because this is what gets PUT.
    let encoded = serde_json::to_string(&parsed).expect("it serialises");
    assert_eq!(encoded, json);
}

#[test]
fn on_stays_a_number_because_the_light_rejects_a_boolean() {
    let encoded =
        serde_json::to_string(&Lights::one(light().set_on(false))).expect("it serialises");
    assert!(
        encoded.contains(r#""on":0"#),
        "the firmware takes 0 and 1, not false and true: {encoded}"
    );
}

#[test]
fn a_light_the_firmware_reports_oddly_is_read_as_on_rather_than_off() {
    // Only 0 and 1 are ever sent. But a lit light reported as dark is the
    // wrong way round for someone who cannot check by looking, so anything
    // that is not zero counts as on.
    assert!(Light { on: 2, ..light() }.is_on());
    assert!(!Light { on: 0, ..light() }.is_on());
}

#[test]
fn brightness_is_clamped_to_what_the_light_actually_accepts() {
    // The floor is the light's own: it will not go below three percent while
    // it is on. Off is a separate field, which is the right model.
    assert_eq!(light().set_brightness(0).brightness, 3);
    assert_eq!(light().set_brightness(1).brightness, 3);
    assert_eq!(light().set_brightness(50).brightness, 50);
    assert_eq!(light().set_brightness(100).brightness, 100);
    assert_eq!(light().set_brightness(9000).brightness, 100);
}

#[test]
fn a_clamp_can_be_reported_rather_than_only_applied() {
    // Silently accepting 0 and setting 3 is how someone ends up believing a
    // light is off when it is at its dimmest.
    assert!(!BRIGHTNESS.holds(0));
    assert!(BRIGHTNESS.holds(3));
    assert!(BRIGHTNESS.holds(100));
    assert!(!BRIGHTNESS.holds(101));
}

#[test]
fn the_temperature_scale_runs_backwards_against_kelvin() {
    // The single most confusable thing about this device. A *larger* mired is
    // a *warmer*, lower-Kelvin light, and getting it the wrong way round
    // gives someone daylight when they asked for candlelight.
    assert!(mired_to_kelvin(TEMPERATURE.low) > mired_to_kelvin(TEMPERATURE.high));
    assert_eq!(mired_to_kelvin(143), 6993);
    assert_eq!(mired_to_kelvin(344), 2907);
}

#[test]
fn the_kelvin_range_matches_the_numbers_elgato_publishes() {
    // Their app says 2900 to 7000. The reciprocal of the mired range gives
    // 2906 and 6993; the difference is rounding, not disagreement, and this
    // test is what says so rather than leaving it to be rediscovered.
    let range = kelvin_range();
    assert_eq!(
        range.low, 2907,
        "the warm end should be about the 2900 K Elgato advertises"
    );
    assert_eq!(
        range.high, 6993,
        "the cold end should be about the 7000 K Elgato advertises"
    );
}

#[test]
fn common_temperatures_land_where_a_photographer_would_expect() {
    // The reciprocal is exact for the first two, which is part of why they
    // are the ones people name. 6500 is not exact — 153.85 mireds — and 154
    // is the nearer step, coming back as 6493 K rather than the 6535 K
    // truncation would give.
    assert_eq!(kelvin_to_mired(4000), 250);
    assert_eq!(kelvin_to_mired(5000), 200);
    assert_eq!(kelvin_to_mired(6500), 154);
    assert_eq!(kelvin_to_mired(3000), 333);
}

#[test]
fn a_temperature_outside_the_range_is_clamped_rather_than_rejected() {
    // Sending an out-of-range value unclamped is worse than clamping: the
    // light rejects the whole request, so an ambitious temperature would
    // silently discard the brightness sent with it.
    assert_eq!(kelvin_to_mired(9000), TEMPERATURE.low);
    assert_eq!(kelvin_to_mired(1000), TEMPERATURE.high);
}

#[test]
fn arithmetic_on_a_devices_own_answer_never_divides_by_zero() {
    // A mired of zero is not a temperature any light reports. Arithmetic that
    // panics on data from a device is arithmetic that eventually panics on
    // data from a device.
    assert_eq!(mired_to_kelvin(0), u16::MAX);
    assert_eq!(kelvin_to_mired(0), TEMPERATURE.high);
}

#[test]
fn a_temperature_survives_the_round_trip_it_is_asked_to_make() {
    // Kelvin in, mireds on the wire, Kelvin back out. Exactness is not
    // available: the mired scale has about two hundred steps across the whole
    // range, and one of them near the cold end is worth about forty Kelvin.
    // 24 K is the worst case across the range with rounding, against 48 with
    // truncation, and this sweep is what holds it there.
    let mut worst = 0;
    for kelvin in kelvin_range().low..=kelvin_range().high {
        let round_tripped = mired_to_kelvin(kelvin_to_mired(kelvin));
        let drift = (i32::from(round_tripped) - i32::from(kelvin)).abs();
        assert!(
            drift <= 24,
            "{kelvin} K came back as {round_tripped} K, which is {drift} off"
        );
        worst = worst.max(drift);
    }
    assert_eq!(
        worst, 24,
        "if the worst case improved, tighten this rather than leaving slack \
         nobody is holding"
    );
}

#[test]
fn a_device_that_lists_no_lights_is_told_apart_from_one_that_did_not_answer() {
    let empty = Lights {
        number_of_lights: 0,
        lights: Vec::new(),
    };
    assert_eq!(empty.first(), Err(LightError::NoLights));
}

#[test]
fn a_range_clamps_from_both_ends_and_holds_its_own_bounds() {
    let range = Range { low: 10, high: 20 };
    assert_eq!(range.clamp(5), 10);
    assert_eq!(range.clamp(15), 15);
    assert_eq!(range.clamp(25), 20);
    assert!(range.holds(10));
    assert!(range.holds(20));
    assert!(!range.holds(9));
    assert!(!range.holds(21));
}
