use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::{instance_name, pick_address};

#[test]
fn a_plain_name_comes_through_unchanged() {
    assert_eq!(
        instance_name("Elgato Key Light Air 1234._elg._tcp.local."),
        "Elgato Key Light Air 1234"
    );
}

#[test]
fn an_escaped_dot_is_a_dot_and_not_a_separator() {
    // A light called "Key Light v1.2" is an ordinary thing to call one, and
    // splitting on the first dot would name it "Key Light v1".
    assert_eq!(
        instance_name(r"Key Light v1\.2._elg._tcp.local."),
        "Key Light v1.2"
    );
}

#[test]
fn a_decimal_escape_becomes_the_character_it_names() {
    // Left in, this reaches a screen reader as "backslash zero three two",
    // which is the whole reason the unescaping exists.
    assert_eq!(
        instance_name(r"Key\032Light\032Left._elg._tcp.local."),
        "Key Light Left"
    );
}

#[test]
fn an_escaped_backslash_is_one_backslash() {
    assert_eq!(instance_name(r"Key\\Light._elg._tcp.local."), r"Key\Light");
}

#[test]
fn a_trailing_escape_with_nothing_after_it_does_not_panic() {
    // Malformed input from the network. Forgiving it is the right trade: a
    // crash here takes out the whole discovery, and every other light with it.
    assert_eq!(instance_name(r"Key Light\"), "Key Light");
}

#[test]
fn a_short_decimal_escape_is_taken_literally_rather_than_guessed_at() {
    // Two digits is not a decimal escape. Taking the next character literally
    // is what DNS-SD says to do, and inventing a byte from a partial number
    // would rename someone's light.
    assert_eq!(instance_name(r"Key\03._elg._tcp.local."), "Key03");
}

#[test]
fn a_name_that_is_not_under_this_service_is_left_whole() {
    // Defensive: the browse only asks for one service, but a name that did
    // not end the expected way must not be silently truncated.
    assert_eq!(
        instance_name("something-else._http._tcp.local."),
        "something-else._http._tcp.local."
    );
}

#[test]
fn a_link_local_ipv6_is_never_chosen() {
    // A Key Light announces one alongside its IPv4 address. Without its scope
    // identifier it does not route, so choosing it would give a light that is
    // discovered and then unreachable — worse than not finding it.
    let addresses = [
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 40)),
    ];
    assert_eq!(
        pick_address(&addresses),
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 40)))
    );
}

#[test]
fn ipv4_is_preferred_over_a_routable_ipv6() {
    // Distinct from the test above, and the distinction is the point: there
    // the IPv6 address was excluded as unusable, so the IPv4 one would have
    // been chosen by any rule at all. Here both are usable and the order is
    // the only thing deciding, which is what makes this the test of the
    // preference rather than of the filter. Announced IPv6 first, so a rule
    // that simply took the first usable address fails.
    let addresses = [
        IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 40)),
    ];
    assert_eq!(
        pick_address(&addresses),
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 40)))
    );
}

#[test]
fn a_routable_ipv6_is_used_when_there_is_no_ipv4() {
    let addresses = [IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1))];
    assert_eq!(pick_address(&addresses), Some(addresses[0]));
}

#[test]
fn a_light_announcing_only_unusable_addresses_is_skipped() {
    // Nothing here can be talked to, and reporting a light that cannot be
    // reached would put an entry in the list that only ever fails.
    let addresses = [
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
    ];
    assert_eq!(pick_address(&addresses), None);
}

#[test]
fn a_light_that_announced_nothing_is_skipped() {
    assert_eq!(pick_address(&[]), None);
}
