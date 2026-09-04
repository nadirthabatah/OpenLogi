//! Monitors, reached over the video cable with DDC/CI.
//!
//! Blocking throughout: a DDC exchange is a pair of I²C `ioctl`s with a
//! mandatory wait between them.

use roadie_ddc::Feature;
use roadie_display::Display;
use roadie_ipc::desk::{
    DisplayControl, DisplayFailure, DisplayReading, DisplaySettings, DisplaySummary,
};

use super::blocking;

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
}
