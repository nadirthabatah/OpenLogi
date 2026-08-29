use std::net::{IpAddr, Ipv4Addr};

use roadie_keylight::{KeyLight, Light};

use super::{Found, describe, ranges};
use crate::spoken::{assert_agrees, assert_listenable};

fn light() -> Light {
    Light {
        on: 1,
        brightness: 20,
        temperature: 213,
    }
}

fn found(state: Result<Light, String>) -> Found {
    Found {
        light: KeyLight::at(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 40))).named("Key Light Left"),
        state,
    }
}

#[test]
fn every_rendered_string_is_listenable() {
    let strings = [
        describe(&found(Ok(light()))),
        describe(&found(Ok(light().set_on(false)))),
        describe(&found(Err("timed out".to_owned()))),
        ranges(),
    ];
    for text in &strings {
        assert_listenable(text, "roadie light, network");
        assert_agrees(text, "roadie light, network");
    }
}

#[test]
fn a_light_that_is_on_says_its_brightness_and_its_colour() {
    // Kelvin, not mireds. Everything else on this desk speaks Kelvin, and the
    // light's own units run backwards against it.
    assert_eq!(
        describe(&found(Ok(light()))),
        "  Key Light Left is on at 20 percent, 4695 kelvin.\n"
    );
}

#[test]
fn a_light_that_is_off_says_only_that() {
    // Its brightness and colour are still set, and saying them would suggest
    // it was lit.
    assert_eq!(
        describe(&found(Ok(light().set_on(false)))),
        "  Key Light Left is off.\n"
    );
}

#[test]
fn a_light_that_was_found_and_did_not_answer_is_still_listed() {
    // Discovery and reachability are different questions, and a light that
    // went to sleep between the two is the ordinary case. Dropping it would
    // answer "where is my key light" with nothing.
    let text = describe(&found(Err("timed out".to_owned())));
    assert!(text.contains("Key Light Left"), "{text}");
    assert!(text.contains("did not answer"), "{text}");
    assert!(
        text.contains("192.168.1.40"),
        "and says where it was, which is what someone pings: {text}"
    );
}

#[test]
fn the_ranges_are_the_ones_the_light_actually_accepts() {
    // Read once from the crate rather than written here, so a change to the
    // protocol's own limits cannot leave this sentence stale.
    let text = ranges();
    assert!(text.contains("3 to 100 percent"), "{text}");
    assert!(text.contains("2907 to 6993 kelvin"), "{text}");
}
