//! Monitors and network lights, on the agent's side of the socket.
//!
//! The GUI does no device I/O — that is the whole of its contract — so the
//! panels that show a monitor's brightness or a Key Light's colour ask the
//! agent, and this is what answers.
//!
//! Everything here is blocking: a DDC exchange is a pair of I²C `ioctl`s with
//! a mandatory wait between them, and both the Elgato HTTP client and the
//! multicast listener block too. So every entry point hands its work to
//! [`tokio::task::spawn_blocking`] rather than stalling the agent's runtime,
//! which is also serving the input hook.
//!
//! Nothing is cached between calls. A monitor unplugged and plugged back in
//! gets a new handle, a light's address comes from a DHCP lease, and a stale
//! handle that writes to the wrong device is a worse failure than an open()
//! per request — which is cheap next to the exchange that follows it.

use std::net::SocketAddr;
use std::time::Duration;

use roadie_ddc::Feature;
use roadie_display::Display;
use roadie_ipc::desk::{
    DisplayControl, DisplayFailure, DisplayReading, DisplaySettings, DisplaySummary,
    NetworkLightChange, NetworkLightFailure, NetworkLightSummary,
};
use roadie_keylight::{KeyLight, Light};

/// How long to listen for Elgato announcements.
///
/// The same window the command line uses. Long enough for a light that is
/// awake to answer, short enough that somebody opening a panel does not think
/// it has hung.
const DISCOVERY_WAIT: Duration = Duration::from_secs(3);

/// The monitor feature behind each control the GUI offers.
const fn feature_of(control: DisplayControl) -> Feature {
    match control {
        DisplayControl::Brightness => Feature::Brightness,
        DisplayControl::Contrast => Feature::Contrast,
        DisplayControl::Volume => Feature::Volume,
        DisplayControl::Input => Feature::InputSource,
    }
}

/// Every monitor attached, and whether each answers.
pub async fn list_displays() -> Vec<DisplaySummary> {
    blocking(
        || {
            let Ok(mut displays) = roadie_display::enumerate() else {
                return Vec::new();
            };
            displays.iter_mut().map(summarize).collect()
        },
        Vec::new,
    )
    .await
}

/// One monitor's summary, including the probe that says whether it answers.
fn summarize(display: &mut Display) -> DisplaySummary {
    // The MCCS version is the cheapest question there is, and a monitor that
    // answers it will answer the rest. Asking for brightness instead would
    // conflate "cannot be reached" with "does not implement brightness".
    let probe = display.get(Feature::McssVersion);
    DisplaySummary {
        id: display.id().as_str().to_owned(),
        name: display.describe(),
        reachable: probe.is_ok(),
        unreachable_reason: probe.err().map(|error| error.to_string()),
    }
}

/// What one monitor's controls read now.
pub async fn read_display(id: String) -> Result<DisplaySettings, DisplayFailure> {
    blocking(
        move || {
            let mut display = find(&id)?;
            let readings = DisplayControl::all()
                .into_iter()
                .filter_map(|control| {
                    // A control the monitor does not implement is left out rather
                    // than reported as a failure. A model without speakers has no
                    // volume, and that is a fact about it, not a fault.
                    let value = display.get(feature_of(control)).ok()?;
                    Some(DisplayReading {
                        control,
                        current: value.current,
                        maximum: value.maximum,
                    })
                })
                .collect();
            Ok(DisplaySettings { id, readings })
        },
        || Err(lost_display()),
    )
    .await
}

/// Change one control, answering with what the monitor then reads.
pub async fn set_display(
    id: String,
    control: DisplayControl,
    value: u16,
) -> Result<DisplayReading, DisplayFailure> {
    blocking(
        move || {
            let mut display = find(&id)?;
            let feature = feature_of(control);
            display
                .set(feature, value)
                .map_err(|error| DisplayFailure::Unreachable(error.to_string()))?;
            // Read back rather than echo. A monitor is free to clamp, round, or
            // ignore, and a panel showing the request instead of the result would
            // be confidently wrong — which is worse than showing nothing.
            let read = display
                .get(feature)
                .map_err(|error| DisplayFailure::Unreachable(error.to_string()))?;
            Ok(DisplayReading {
                control,
                current: read.current,
                maximum: read.maximum,
            })
        },
        || Err(lost_display()),
    )
    .await
}

/// The monitor with that handle, or why not.
fn find(id: &str) -> Result<Display, DisplayFailure> {
    let mut displays = roadie_display::enumerate()
        .map_err(|error| DisplayFailure::Unreachable(error.to_string()))?;
    let found = displays
        .iter()
        .position(|display| display.id().as_str() == id)
        .ok_or(DisplayFailure::NotFound)?;
    Ok(displays.swap_remove(found))
}

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

/// Run blocking device work off the runtime that is also serving the hook.
///
/// `lost` is what to answer if the blocking task panics or is cancelled. It is
/// a closure so that nothing is built for the case that almost never happens,
/// and it is a parameter rather than [`Default`] because the honest answer
/// differs: an empty list is a fine way to say "found none", but an empty
/// reading would be a lie about a monitor that was never asked.
async fn blocking<T, F, L>(work: F, lost: L) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
    L: FnOnce() -> T,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(answer) => answer,
        Err(_) => lost(),
    }
}

/// What a lost blocking task means to a caller waiting on a device.
///
/// Indistinguishable from the device not answering, which is what it is.
fn lost_display() -> DisplayFailure {
    DisplayFailure::Unreachable(
        "the agent stopped talking to that monitor unexpectedly. Trying again is the right \
         next step; if it keeps happening it is worth reporting."
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_control_maps_to_a_real_feature() {
        // The point of the check is that the mapping is total: adding a
        // control to the wire enum without a feature behind it would compile
        // and then do nothing at runtime.
        for control in DisplayControl::all() {
            let feature = feature_of(control);
            assert!(
                Feature::NAMED.contains(&feature),
                "{control:?} maps to {feature:?}, which is not a feature this crate names"
            );
        }
    }

    #[test]
    fn brightness_maps_to_luminance() {
        // Named brightness everywhere a person looks, and luminance in the
        // specification. Worth pinning so a rename in either place is caught.
        assert_eq!(feature_of(DisplayControl::Brightness), Feature::Brightness);
        assert_eq!(feature_of(DisplayControl::Input), Feature::InputSource);
    }

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
