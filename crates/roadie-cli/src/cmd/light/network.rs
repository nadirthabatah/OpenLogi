//! The half of `roadie light` that is on the network rather than on the desk.
//!
//! Elgato Key Lights are reached over Wi-Fi. They belong under `roadie light`
//! rather than a command of their own, because the question someone asks is
//! "what lights do I have" and an answer that covered only the ones on USB
//! would be a worse answer for being confidently incomplete.
//!
//! The rendering functions here take values rather than devices, so the
//! strings that ship are the strings the tests sweep — a network device is
//! even less available to a test than a USB one.

use std::time::Duration;

use roadie_keylight::state::{BRIGHTNESS, kelvin_range};
use roadie_keylight::{KeyLight, Light};

/// One discovered light, and whatever it said about itself.
///
/// The state is optional because discovery and reachability are different
/// questions: a light can announce itself over multicast and then refuse the
/// HTTP request, most often because it went to sleep between the two. Keeping
/// the entry with no state is the same choice the monitor code makes — a light
/// that is there and not answering is worth saying, and dropping it would
/// answer "where is my key light" with nothing.
#[derive(Debug, Clone)]
pub struct Found {
    /// The light, ready to be written to.
    pub light: KeyLight,
    /// What it is doing, or why that could not be read.
    pub state: Result<Light, String>,
}

impl Found {
    /// The name the light announced, or its address if it announced none.
    #[must_use]
    pub fn name(&self) -> &str {
        self.light.name()
    }
}

/// Every Key Light on the network, with its current state.
///
/// A failure to look is not a failure of the command. `roadie light` exists
/// mainly for the Litra on the desk, and a machine with no multicast — a
/// container, a locked-down network — must still get its USB lights listed.
/// So the reason is traced and the list comes back empty.
pub fn find(wait: Duration) -> Vec<Found> {
    let lights = match roadie_keylight::discover(wait) {
        Ok(lights) => lights,
        Err(error) => {
            tracing::debug!(%error, "could not look for lights on the network");
            return Vec::new();
        }
    };
    lights.iter().map(read).collect()
}

/// Ask one discovered light what it is doing.
fn read(light: &KeyLight) -> Found {
    Found {
        light: light.clone(),
        state: light.read().map_err(|error| error.to_string()),
    }
}

/// What one network light's line says.
///
/// Kelvin rather than mireds, because everything else on this desk speaks
/// Kelvin and the light's own units run backwards against it. Brightness as a
/// percentage, which is what the light itself uses.
#[must_use]
pub fn describe(found: &Found) -> String {
    match &found.state {
        Ok(light) if light.is_on() => format!(
            "  {} is on at {} percent, {} kelvin.\n",
            found.name(),
            light.brightness,
            light.kelvin()
        ),
        Ok(_) => format!("  {} is off.\n", found.name()),
        Err(why) => format!(
            "  {} was found at {} but did not answer: {why}\n",
            found.name(),
            found.light.address()
        ),
    }
}

/// What to say about the ranges a Key Light accepts.
///
/// Said once, under the list, rather than after every light: they are the same
/// for every unit in the family, and repeating them per light is three extra
/// sentences to sit through for one fact.
#[must_use]
pub fn ranges() -> String {
    let kelvin = kelvin_range();
    format!(
        "Elgato lights take a brightness from {} to {} percent, and a colour \
         temperature from {} to {} kelvin.\n",
        BRIGHTNESS.low, BRIGHTNESS.high, kelvin.low, kelvin.high
    )
}

#[cfg(test)]
mod tests;
