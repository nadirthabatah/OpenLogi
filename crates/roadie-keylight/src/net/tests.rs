use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::{KeyLight, NetError};
use crate::{INFO_PATH, LIGHTS_PATH, PORT};

fn v4() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 40))
}

#[test]
fn a_light_is_addressed_on_the_port_the_firmware_serves() {
    // Fixed by the firmware and the same across the family, so a wrong number
    // here would look exactly like every light on the desk being unplugged.
    assert_eq!(PORT, 9123);
    let light = KeyLight::at(v4());
    assert_eq!(
        light.url(LIGHTS_PATH),
        "http://192.168.1.40:9123/elgato/lights"
    );
    assert_eq!(
        light.url(INFO_PATH),
        "http://192.168.1.40:9123/elgato/accessory-info"
    );
}

#[test]
fn an_ipv6_address_is_bracketed_the_way_a_url_needs() {
    // Forgetting the brackets gives a URL parse error rather than a
    // connection failure, which sends someone looking at their network
    // instead of at this line.
    let light = KeyLight::at(IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)));
    assert_eq!(
        light.url(LIGHTS_PATH),
        "http://[fe80::1]:9123/elgato/lights"
    );
}

#[test]
fn a_light_behind_a_forwarded_port_is_addressable() {
    let light = KeyLight::at_port(v4(), 19123);
    assert_eq!(
        light.url(LIGHTS_PATH),
        "http://192.168.1.40:19123/elgato/lights"
    );
}

#[test]
fn a_light_is_known_by_its_address_until_it_gives_a_name() {
    let light = KeyLight::at(v4());
    assert_eq!(light.name(), "192.168.1.40");
    assert_eq!(light.named("Key Light Left").name(), "Key Light Left");
}

#[test]
fn a_failure_says_which_light_and_what_was_being_done() {
    // The message is the whole of what someone gets when a light is
    // unplugged, and "connection refused" on its own names neither.
    let error = KeyLight::at(v4())
        .named("Key Light Left")
        .failed("changing its settings", &"connection refused");
    let message = error.to_string();
    assert!(message.contains("Key Light Left"), "{message}");
    assert!(message.contains("changing its settings"), "{message}");
    assert!(message.contains("connection refused"), "{message}");
}

#[test]
fn every_failure_names_the_light() {
    // Whichever of the three it is. A desk can have several, and a message
    // that does not say which one is a message that cannot be acted on.
    let errors = [
        NetError::Unreachable {
            light: "Key Light Left".to_owned(),
            doing: "reading its state".to_owned(),
            reason: "timed out".to_owned(),
        },
        NetError::Malformed {
            light: "Key Light Left".to_owned(),
            what: "its state".to_owned(),
            reason: "expected value".to_owned(),
        },
        NetError::Refused {
            light: "Key Light Left".to_owned(),
            reason: "no lights".to_owned(),
        },
    ];
    for error in errors {
        assert!(
            error.to_string().starts_with("Key Light Left"),
            "the light names itself first: {error}"
        );
    }
}
