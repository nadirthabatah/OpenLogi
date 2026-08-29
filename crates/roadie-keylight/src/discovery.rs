//! Finding the lights, without anyone having to know an IP address.
//!
//! Key Lights announce themselves over multicast DNS, under `_elg._tcp.local.`
//! This is not a convenience: a light's address comes from the router's DHCP
//! lease and changes on its own, so an address written down in a config file
//! is an address that will be wrong one morning. Discovery is how the light
//! stays findable without anybody maintaining that.
//!
//! # What is testable here and what is not
//!
//! The daemon loop needs a network with lights on it, and the machines this is
//! built on have neither. So the two pieces of judgement it contains are
//! separated out and tested on their own: [`instance_name`], which turns a
//! DNS-SD full name into the name a person gave the light, and
//! [`pick_address`], which chooses among the several addresses one light
//! announces. What is left in the loop is plumbing.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

use crate::SERVICE;
use crate::net::KeyLight;

/// How long to listen before answering.
///
/// Long enough for a light on a quiet network to answer twice, short enough
/// that someone waiting to hear a list does not think the command has hung.
/// Lights answer in well under a second on a normal network; the rest of this
/// is margin for a busy one.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(3);

/// Every Key Light that answers within `wait`.
///
/// Sorted by name, so the list reads the same twice running — which matters
/// more than usual when someone is counting positions in it by ear, and
/// multicast answers arrive in whatever order the network delivers them.
///
/// # Errors
///
/// Returns [`DiscoveryError`] if the multicast responder could not be started
/// or the browse could not begin. An empty list is not an error: a desk with
/// no Key Lights on it is the ordinary case.
pub fn discover(wait: Duration) -> Result<Vec<KeyLight>, DiscoveryError> {
    let daemon = ServiceDaemon::new().map_err(|error| DiscoveryError::NoResponder {
        reason: error.to_string(),
    })?;
    let receiver = daemon
        .browse(SERVICE)
        .map_err(|error| DiscoveryError::NoBrowse {
            reason: error.to_string(),
        })?;

    // Keyed by full name so a light that answers on several interfaces, or
    // answers twice, is one entry. The map is ordered, which is what makes
    // the returned list stable.
    let mut found: BTreeMap<String, KeyLight> = BTreeMap::new();
    let deadline = Instant::now() + wait;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = receiver.recv_timeout(remaining) else {
            break;
        };
        let ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };
        let addresses: Vec<IpAddr> = service
            .addresses
            .iter()
            .map(mdns_sd::ScopedIp::to_ip_addr)
            .collect();
        let Some(address) = pick_address(&addresses) else {
            tracing::debug!(
                service = %service.fullname,
                "a light answered with no usable address"
            );
            continue;
        };
        let name = instance_name(&service.fullname);
        found.insert(
            service.fullname.clone(),
            KeyLight::at_port(address, service.port).named(name),
        );
    }

    // The daemon owns sockets and a thread; shutting it down is not optional
    // just because the process is about to exit anyway, since a long-running
    // caller would otherwise leak one per call.
    let _ = daemon.shutdown();

    let mut lights: Vec<KeyLight> = found.into_values().collect();
    lights.sort_by(|a, b| a.name().cmp(b.name()));
    Ok(lights)
}

/// The name a person gave the light, from its DNS-SD full name.
///
/// A full name is `Instance Name._elg._tcp.local.`, and the instance half is
/// what someone typed into Elgato's app. DNS-SD escapes it: a literal dot
/// becomes `\.`, a backslash `\\`, and anything awkward `\032` and friends in
/// decimal. Leaving those in reaches a screen reader as "backslash zero three
/// two", which is the whole reason this function exists rather than a
/// `split('.')`.
#[must_use]
pub fn instance_name(fullname: &str) -> String {
    let instance = fullname
        .strip_suffix(SERVICE)
        .and_then(|instance| instance.strip_suffix('.'))
        .unwrap_or(fullname);

    let mut out = String::with_capacity(instance.len());
    let mut characters = instance.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        // An escape. Either three decimal digits naming a byte, or the next
        // character taken literally.
        let rest: String = characters.clone().take(3).collect();
        if rest.len() == 3
            && rest.chars().all(|digit| digit.is_ascii_digit())
            && let Ok(byte) = rest.parse::<u8>()
        {
            out.push(char::from(byte));
            for _ in 0..3 {
                characters.next();
            }
            continue;
        }
        if let Some(literal) = characters.next() {
            out.push(literal);
        }
    }
    out.trim().to_owned()
}

/// The address to actually talk to, from the several a light announces.
///
/// IPv4 first, and not out of habit. A Key Light announces link-local IPv6
/// alongside its IPv4 address, and a link-local address without its scope
/// identifier does not route — so preferring IPv6 would produce a light that
/// is discovered and then unreachable, which is worse than not finding it.
/// Unspecified and loopback addresses are skipped outright: neither can be a
/// light on the network.
#[must_use]
pub fn pick_address(addresses: &[IpAddr]) -> Option<IpAddr> {
    let usable = |address: &&IpAddr| match address {
        IpAddr::V4(v4) => !v4.is_unspecified() && !v4.is_loopback(),
        IpAddr::V6(v6) => !v6.is_unspecified() && !v6.is_loopback() && !is_link_local(v6),
    };
    addresses
        .iter()
        .filter(usable)
        .find(|address| address.is_ipv4())
        .or_else(|| addresses.iter().find(usable))
        .copied()
}

/// Whether an IPv6 address is link-local, which cannot be used without the
/// interface it was seen on.
///
/// Hand-written because `Ipv6Addr::is_unicast_link_local` is not stable, and
/// the definition is one comparison: the `fe80::/10` prefix.
fn is_link_local(address: &std::net::Ipv6Addr) -> bool {
    address.segments()[0] & 0xFFC0 == 0xFE80
}

/// Why looking for lights did not work.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiscoveryError {
    /// The multicast responder could not be started.
    #[error("could not start looking for lights on the network: {reason}")]
    NoResponder {
        /// What went wrong.
        reason: String,
    },
    /// The responder started and the browse could not.
    #[error("could not ask the network for Elgato lights: {reason}")]
    NoBrowse {
        /// What went wrong.
        reason: String,
    },
}

#[cfg(test)]
mod tests;
