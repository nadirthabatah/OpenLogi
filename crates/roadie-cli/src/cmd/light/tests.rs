use std::net::{IpAddr, Ipv4Addr};

use roadie_core::device::{DeviceKind, RawDeviceAddress, StandaloneDevice};
use roadie_keylight::{KeyLight, Light};

use super::network::Found;
use super::{Lamp, nothing_found, select_lamp};
use crate::spoken::{assert_agrees, assert_listenable};

fn litra(name: &str) -> StandaloneDevice {
    StandaloneDevice {
        address: RawDeviceAddress {
            vendor_id: 0x046d,
            product_id: 0xc900,
            usage_page: 0xff43,
            usage_id: 0x0202,
            identity: "serial:test".into(),
        },
        display_name: name.into(),
        manufacturer: Some("Logi".into()),
        serial_number: Some("test".into()),
        unit_id: [0; 4],
        kind: DeviceKind::Light,
        online: true,
        capabilities: None,
        light_capabilities: None,
        driver_id: "litra".into(),
        registry_model_id: None,
    }
}

fn key(name: &str, last_octet: u8) -> Found {
    Found {
        light: KeyLight::at(IpAddr::V4(Ipv4Addr::new(192, 168, 1, last_octet))).named(name),
        state: Ok(Light {
            on: 1,
            brightness: 20,
            temperature: 213,
        }),
    }
}

#[test]
fn one_light_of_either_kind_needs_no_choosing() {
    assert!(matches!(
        select_lamp(&[litra("Litra Glow")], &[], None).expect("one light"),
        Lamp::Litra(_)
    ));
    assert!(matches!(
        select_lamp(&[], &[key("Key Light Left", 40)], None).expect("one light"),
        Lamp::Network(_)
    ));
}

#[test]
fn one_light_on_each_transport_is_still_two_lights() {
    // The bug this guards: treating the two families as separate lists would
    // make each look like "the only light", and a command with no --device
    // would silently pick whichever list was consulted first.
    let error = select_lamp(&[litra("Litra Glow")], &[key("Key Light Left", 40)], None)
        .expect_err("two lights and no choice made");
    let message = error.to_string();
    assert!(message.contains("2 lights are found"), "{message}");
    assert_listenable(&message, "roadie light");
    assert_agrees(&message, "roadie light");
}

#[test]
fn a_query_reaches_across_both_transports() {
    let litra = [litra("Litra Glow")];
    let network = [key("Key Light Left", 40)];
    assert_eq!(
        select_lamp(&litra, &network, Some("glow"))
            .expect("one match")
            .name(),
        "Litra Glow"
    );
    assert_eq!(
        select_lamp(&litra, &network, Some("key light"))
            .expect("one match")
            .name(),
        "Key Light Left"
    );
}

#[test]
fn a_network_light_can_be_picked_by_its_address() {
    // Two lights named the same in Elgato's app is an ordinary thing to end
    // up with, and the address is the only thing that then tells them apart.
    let network = [key("Key Light", 40), key("Key Light", 41)];
    assert_eq!(
        select_lamp(&[], &network, Some("192.168.1.41"))
            .expect("one match")
            .name(),
        "Key Light"
    );
}

#[test]
fn an_ambiguous_query_names_the_candidates_rather_than_only_counting_them() {
    // The next thing someone does is choose between them, and "use more of
    // the name" without the names is an instruction they cannot follow.
    let network = [key("Key Light Left", 40), key("Key Light Right", 41)];
    let error = select_lamp(&[], &network, Some("key light")).expect_err("two match");
    let message = error.to_string();
    assert!(message.contains("Key Light Left"), "{message}");
    assert!(message.contains("Key Light Right"), "{message}");
    assert_listenable(&message, "roadie light");
}

#[test]
fn a_query_that_matches_nothing_says_where_to_look() {
    let error =
        select_lamp(&[litra("Litra Glow")], &[], Some("beam")).expect_err("nothing matches");
    let message = error.to_string();
    assert!(message.contains("roadie light list"), "{message}");
    assert_listenable(&message, "roadie light");
}

#[test]
fn no_lights_at_all_says_what_was_looked_for() {
    let error = select_lamp(&[], &[], None).expect_err("nothing is attached");
    assert!(error.to_string().contains("roadie light list"), "{error}");

    // And the listing's own version says which places were searched, because
    // "no lights found" after --no-network would otherwise read as a
    // statement about the network too.
    let searched = nothing_found(false);
    assert!(searched.contains("on USB or on the network"), "{searched}");
    let declined = nothing_found(true);
    assert!(declined.contains("was not searched"), "{declined}");

    for text in [searched, declined] {
        assert_listenable(&text, "roadie light");
        assert_agrees(&text, "roadie light");
    }
}
