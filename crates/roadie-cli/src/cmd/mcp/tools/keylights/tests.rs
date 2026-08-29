use std::net::{IpAddr, Ipv4Addr};

use roadie_keylight::state::TEMPERATURE;
use roadie_keylight::{KeyLight, Light};
use serde_json::json;

use super::{change, choose, ensure_changed};

fn light() -> Light {
    Light {
        on: 1,
        brightness: 20,
        temperature: 213,
    }
}

fn found(name: &str, last_octet: u8) -> KeyLight {
    KeyLight::at(IpAddr::V4(Ipv4Addr::new(192, 168, 1, last_octet))).named(name)
}

#[test]
fn one_light_needs_no_naming() {
    let lights = [found("Key Light Left", 40)];
    assert_eq!(
        choose(&lights, None).expect("the only light").name(),
        "Key Light Left"
    );
}

#[test]
fn several_lights_will_not_be_guessed_between() {
    // On a desk with a key and a fill light, picking the wrong one is
    // immediately visible to everyone except the person who asked.
    let lights = [found("Key Light Left", 40), found("Key Light Right", 41)];
    let error = choose(&lights, None).expect_err("two lights and no choice made");
    assert!(error.contains("list_network_lights"), "{error}");
}

#[test]
fn a_light_can_be_named_or_addressed() {
    let lights = [found("Key Light Left", 40), found("Fill", 41)];
    assert_eq!(
        choose(&lights, Some(&json!("fill"))).expect("one").name(),
        "Fill"
    );
    assert_eq!(
        choose(&lights, Some(&json!("192.168.1.40")))
            .expect("one")
            .name(),
        "Key Light Left"
    );
}

#[test]
fn an_ambiguous_name_lists_the_candidates() {
    let lights = [found("Key Light Left", 40), found("Key Light Right", 41)];
    let error = choose(&lights, Some(&json!("key light"))).expect_err("two match");
    assert!(error.contains("Key Light Left"), "{error}");
    assert!(error.contains("Key Light Right"), "{error}");
}

#[test]
fn several_changes_are_applied_in_one_go() {
    // One PUT, which is what the light expects. Three separate calls would be
    // three round trips and two intermediate states on someone's face.
    let after = change(
        light(),
        &json!({"power": "on", "brightness_percent": 40, "temperature_kelvin": 4000}),
    )
    .expect("all three are valid");
    assert!(after.is_on());
    assert_eq!(after.brightness, 40);
    assert_eq!(after.temperature, 250);
}

#[test]
fn a_change_that_asks_for_nothing_is_left_identical() {
    // The caller turns this into the error; what matters here is that no
    // field is quietly invented when none was given.
    assert_eq!(change(light(), &json!({})).expect("nothing to do"), light());
}

#[test]
fn values_outside_the_lights_range_are_clamped_rather_than_refused() {
    // Refusing would be worse: the light rejects an out-of-range request
    // whole, so an ambitious temperature would silently discard the
    // brightness sent alongside it.
    let after = change(
        light(),
        &json!({"brightness_percent": 0, "temperature_kelvin": 9000}),
    )
    .expect("clamped, not refused");
    assert_eq!(after.brightness, 3, "the light's own floor while it is on");
    assert_eq!(after.temperature, TEMPERATURE.low, "its coldest");
}

#[test]
fn a_value_too_wide_for_the_wire_is_clamped_rather_than_wrapping() {
    // A number from a model is a number from anywhere: it can be anything.
    // 65536 is the value that tells the two behaviours apart — truncated to
    // sixteen bits it becomes 0 and then clamps to the light's dimmest, which
    // is the opposite of what was asked for. 70000 would not have caught it:
    // it truncates to 4464, which clamps to 100 like the correct answer does.
    let after = change(light(), &json!({"brightness_percent": 65536})).expect("clamped");
    assert_eq!(after.brightness, 100);
    let after = change(light(), &json!({"temperature_kelvin": 65536})).expect("clamped");
    assert_eq!(
        after.temperature, TEMPERATURE.low,
        "a huge kelvin is the coldest the light goes, not the warmest"
    );
}

#[test]
fn a_call_that_would_change_nothing_is_refused() {
    // Reported success having changed nothing is the failure nobody notices.
    let error = ensure_changed(light(), light()).expect_err("nothing would change");
    assert!(error.contains("at least one of"), "{error}");

    // Setting a light to what it already is asks for nothing either, and
    // saying so beats a cheerful report that nothing happened.
    let same = change(light(), &json!({"brightness_percent": 20})).expect("valid");
    assert!(ensure_changed(light(), same).is_err());

    ensure_changed(light(), light().set_brightness(40)).expect("that is a change");
}

#[test]
fn a_power_value_that_is_not_on_or_off_is_refused() {
    let error = change(light(), &json!({"power": "bright"})).expect_err("not a power value");
    assert!(error.contains("on"), "{error}");
    assert!(error.contains("off"), "{error}");
}

#[test]
fn a_non_numeric_brightness_is_refused_rather_than_ignored() {
    // Ignoring it would report success having changed nothing, which is the
    // worst of the three possible outcomes.
    let error = change(light(), &json!({"brightness_percent": "forty"})).expect_err("not a number");
    assert!(error.contains("must be a number"), "{error}");
}
