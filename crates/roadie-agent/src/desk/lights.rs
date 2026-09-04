//! Elgato lights, reached over the local network.
//!
//! Blocking throughout: both the HTTP client and the multicast listener block,
//! and discovery alone costs [`DISCOVERY_WAIT`].

use std::net::SocketAddr;
use std::time::Duration;

use roadie_ipc::desk::{NetworkLightChange, NetworkLightFailure, NetworkLightSummary};
use roadie_keylight::{KeyLight, Light};

use super::blocking;

/// How long to listen for Elgato announcements.
///
/// The same window the command line uses. Long enough for a light that is
/// awake to answer, short enough that somebody opening a panel does not think
/// it has hung.
const DISCOVERY_WAIT: Duration = Duration::from_secs(3);

/// Elgato lights answering on the local network.
pub async fn list_network_lights() -> Vec<NetworkLightSummary> {
    blocking(
        || {
            roadie_keylight::discover(DISCOVERY_WAIT)
                .unwrap_or_default()
                .iter()
                .map(|light| match light.read() {
                    Ok(state) => summarize_light(light, state),
                    // Kept rather than dropped. It announced itself, so it is
                    // there; saying so is something a person can act on, and a
                    // light that silently vanishes from the list is not.
                    Err(error) => unreachable_light(light, &error.to_string()),
                })
                .collect()
        },
        Vec::new,
    )
    .await
}

/// One light's summary, from a reading already taken.
fn summarize_light(light: &KeyLight, state: Light) -> NetworkLightSummary {
    NetworkLightSummary {
        id: SocketAddr::new(light.address(), light.port()).to_string(),
        name: light.name().to_owned(),
        on: state.is_on(),
        brightness: state.brightness,
        kelvin: state.kelvin(),
        reachable: true,
        unreachable_reason: None,
    }
}

/// A light that announced itself and then would not say what it is doing.
///
/// The state fields are left at zero and mean nothing; `reachable` is what a
/// reader must consult first, which is why it exists rather than a sentinel
/// value in `brightness`.
fn unreachable_light(light: &KeyLight, why: &str) -> NetworkLightSummary {
    NetworkLightSummary {
        id: SocketAddr::new(light.address(), light.port()).to_string(),
        name: light.name().to_owned(),
        on: false,
        brightness: 0,
        kelvin: 0,
        reachable: false,
        unreachable_reason: Some(why.to_owned()),
    }
}

/// Change one light, answering with what it then reads.
pub async fn set_network_light(
    id: String,
    change: NetworkLightChange,
) -> Result<NetworkLightSummary, NetworkLightFailure> {
    if change.is_empty() {
        return Err(NetworkLightFailure::NothingToDo);
    }
    blocking(
        move || {
            let address: SocketAddr = id.parse().map_err(|_| NetworkLightFailure::NotFound)?;
            let light = KeyLight::at_port(address.ip(), address.port());
            let before = light
                .read()
                .map_err(|error| NetworkLightFailure::Unreachable(error.to_string()))?;

            // Applied in one write because the light's whole state goes in one
            // request. Three separate writes would be three round trips and two
            // intermediate states visible on somebody's face.
            let mut after = before;
            if let Some(on) = change.power {
                after = after.set_on(on);
            }
            if let Some(percent) = change.brightness_percent {
                after = after.set_brightness(percent);
            }
            if let Some(kelvin) = change.kelvin {
                after = after.set_kelvin(kelvin);
            }

            let written = light
                .write(after)
                .map_err(|error| NetworkLightFailure::Unreachable(error.to_string()))?;
            Ok(summarize_light(&light, written))
        },
        || {
            Err(NetworkLightFailure::Unreachable(
                "the agent stopped talking to that light unexpectedly.".to_owned(),
            ))
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_change_asking_for_nothing_never_reaches_the_network() {
        // Checked before the address is even parsed, so an empty change costs
        // no round trip. The unparseable id proves the early return: reaching
        // the parse would give NotFound instead.
        let answer =
            set_network_light("not an address".into(), NetworkLightChange::default()).await;
        assert_eq!(answer, Err(NetworkLightFailure::NothingToDo));
    }

    #[tokio::test]
    async fn an_id_that_is_not_an_address_is_a_light_that_is_not_there() {
        let answer = set_network_light(
            "kitchen".into(),
            NetworkLightChange {
                power: Some(true),
                ..NetworkLightChange::default()
            },
        )
        .await;
        assert_eq!(answer, Err(NetworkLightFailure::NotFound));
    }
}
