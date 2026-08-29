//! The things on the desk that are not HID++ peripherals: monitors reached
//! over the video cable, and Elgato lights reached over the network.
//!
//! These types are declared here rather than borrowed from `roadie-display`
//! and `roadie-keylight` on purpose. The GUI links this crate, and those two
//! carry an I²C `ioctl` layer and a blocking HTTP client respectively —
//! neither of which belongs in a process whose whole contract is that it does
//! no device I/O. So the wire speaks in plain data, and the conversions live
//! in the agent, which is the only side that owns a device.
//!
//! # Why these are asked for rather than observed
//!
//! Every other piece of agent state reaches the GUI through
//! [`Agent::observe`](crate::Agent::observe), which answers with the whole of
//! it whenever any of it changes. These two deliberately do not.
//!
//! A DDC read is a pair of I²C round trips with a mandatory wait between them,
//! and finding Elgato lights means listening to multicast for seconds. Folding
//! either into the state channel would put slow, failure-prone hardware polling
//! behind a channel whose value is that it is cheap enough to leave open — and
//! would make every heartbeat carry a monitor's brightness whether anyone was
//! looking at it or not. So they are ordinary requests, made when a person
//! opens the panel that shows them.

use serde::{Deserialize, Serialize};

/// One monitor, as the agent found it.
///
/// Carries whether it answered, because a monitor that is listed and silent is
/// the interesting case rather than an error: on Linux it usually means the
/// I²C devices belong to the `i2c` group and this user is not in it, which is
/// a thing the person can fix and would never otherwise be told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplaySummary {
    /// Opaque handle, stable for as long as the monitor stays plugged in.
    pub id: String,
    /// The monitor's own name for itself, from its identification block.
    pub name: String,
    /// Whether it answered a read.
    pub reachable: bool,
    /// Why it did not, when it did not.
    pub unreachable_reason: Option<String>,
}

/// A setting on a monitor that the GUI offers.
///
/// Deliberately smaller than the set the command line reaches. Power is absent
/// because a monitor powered off over DDC may stop answering DDC, leaving the
/// button on the bezel as the only way back — a thing to do deliberately at a
/// prompt that says so, not from a panel of everyday knobs. Saving to the
/// monitor's own memory is absent for the same reason: that memory wears out.
///
/// **Append-only.** serde encodes the declaration index.
/// Ordered so a caller can key a map by it. The derives change no encoding —
/// serde writes the declaration index either way — so this is not a wire
/// change, but the declaration order still is: see the append-only note above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DisplayControl {
    /// Backlight level, as a percentage.
    Brightness,
    /// Contrast, in the monitor's own units.
    Contrast,
    /// Speaker volume, where the monitor has speakers.
    Volume,
    /// Which cable the monitor is showing.
    Input,
}

impl DisplayControl {
    /// Every control, in the order a panel should show them.
    ///
    /// Brightness first because it is what almost everyone came for.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Brightness, Self::Contrast, Self::Volume, Self::Input]
    }
}

/// What one control currently reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayReading {
    /// Which control.
    pub control: DisplayControl,
    /// Its value now.
    pub current: u16,
    /// The largest the monitor says it takes.
    ///
    /// From the monitor rather than assumed, because a maximum is per-model:
    /// plenty report 100 for brightness, and plenty do not.
    pub maximum: u16,
}

/// What one monitor answered.
///
/// Only the controls that answered are present. A monitor need not implement
/// every one, and a missing control is an ordinary fact about that model
/// rather than a failure worth reporting as one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplaySettings {
    /// The monitor these came from.
    pub id: String,
    /// Its readings, in [`DisplayControl::all`] order.
    pub readings: Vec<DisplayReading>,
}

/// Why a monitor request could not be answered.
///
/// **Append-only.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayFailure {
    /// No monitor with that handle — usually unplugged since the list was made.
    NotFound,
    /// It is there and did not answer, with what the transport said.
    Unreachable(String),
    /// The write was refused, with the reason to show.
    Refused(String),
}

impl std::fmt::Display for DisplayFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str(
                "that monitor is no longer attached. Unplugging one and plugging it back in \
                 gives it a new handle, so the list is worth refreshing.",
            ),
            Self::Unreachable(why) | Self::Refused(why) => f.write_str(why),
        }
    }
}

/// One Elgato light found on the network.
///
/// Carries whether it answered, for the same reason [`DisplaySummary`] does: a
/// light that announced itself and then would not say what it is doing is
/// worth showing, because "your light is there and not answering" is something
/// a person can act on and its absence from a list is not. Dropping it would
/// also disagree with the command line, which has always kept it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkLightSummary {
    /// Address and port, which is what addresses it and is stable while the
    /// DHCP lease holds.
    pub id: String,
    /// The name it was given in Elgato's own app.
    pub name: String,
    /// Whether it is lit.
    ///
    /// Meaningless unless [`Self::reachable`]: a light that did not answer did
    /// not say.
    pub on: bool,
    /// Brightness, as a percentage. Likewise meaningless unless reachable.
    pub brightness: u16,
    /// Colour temperature in Kelvin — converted from the mireds the light
    /// actually counts in, which run the other way. Likewise.
    pub kelvin: u16,
    /// Whether it answered when asked what it is doing.
    pub reachable: bool,
    /// Why it did not, when it did not.
    pub unreachable_reason: Option<String>,
}

/// A change to a light, with everything that is not being changed left out.
///
/// Several at once because a Key Light's whole state goes in one request, so
/// "on, at forty percent, warm" is one round trip rather than three — and one
/// visible change rather than two intermediate ones on somebody's face.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkLightChange {
    /// Lit or not.
    pub power: Option<bool>,
    /// Brightness as a percentage, clamped to what the light accepts.
    pub brightness_percent: Option<u16>,
    /// Colour temperature in Kelvin, clamped to what the light accepts.
    pub kelvin: Option<u16>,
}

impl NetworkLightChange {
    /// Whether this asks for anything at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.power.is_none() && self.brightness_percent.is_none() && self.kelvin.is_none()
    }
}

/// Why a light request could not be answered.
///
/// **Append-only.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkLightFailure {
    /// No light at that address — asleep, moved, or gone.
    NotFound,
    /// It answered badly, or not in time.
    Unreachable(String),
    /// The change asked for nothing.
    NothingToDo,
}

impl std::fmt::Display for NetworkLightFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str(
                "that light did not answer. Its address comes from a DHCP lease and can \
                 change on its own, so searching again is the fix.",
            ),
            Self::Unreachable(why) => f.write_str(why),
            Self::NothingToDo => f.write_str("that would not change anything."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_is_offered_first() {
        // It is what almost everyone opened the panel for.
        assert_eq!(DisplayControl::all()[0], DisplayControl::Brightness);
    }

    #[test]
    fn the_panel_offers_no_control_that_needs_a_confirmation() {
        // Power off can leave the bezel button as the only way back, and
        // saving wears out the monitor's memory. Both belong at a prompt that
        // says so, which a panel of everyday knobs is not.
        let names = format!("{:?}", DisplayControl::all());
        assert!(!names.contains("Power"), "{names}");
        assert!(!names.contains("Save"), "{names}");
    }

    #[test]
    fn a_change_that_asks_for_nothing_says_so() {
        assert!(NetworkLightChange::default().is_empty());
        assert!(
            !NetworkLightChange {
                power: Some(false),
                ..NetworkLightChange::default()
            }
            .is_empty(),
            "turning a light off is a change like any other"
        );
    }

    #[test]
    fn a_monitor_that_went_away_says_what_to_do_about_it() {
        // The likeliest cause is that it was unplugged and plugged back in,
        // which gives it a new handle — so the useful advice is to refresh
        // rather than to report a fault.
        let said = DisplayFailure::NotFound.to_string();
        assert!(said.contains("refresh"), "{said}");
    }

    #[test]
    fn a_light_that_did_not_answer_is_still_worth_listing() {
        // Its state fields say nothing, which is why `reachable` gates them:
        // a light that did not answer did not say how bright it is, and zero
        // is not the same as unknown.
        let asleep = NetworkLightSummary {
            id: "192.168.1.40:9123".to_owned(),
            name: "Key Light Left".to_owned(),
            on: false,
            brightness: 0,
            kelvin: 0,
            reachable: false,
            unreachable_reason: Some("connection timed out".to_owned()),
        };
        assert!(!asleep.reachable);
        assert!(asleep.unreachable_reason.is_some());
    }

    #[test]
    fn a_light_that_went_away_blames_the_lease_rather_than_the_light() {
        let said = NetworkLightFailure::NotFound.to_string();
        assert!(said.contains("DHCP"), "{said}");
    }
}
