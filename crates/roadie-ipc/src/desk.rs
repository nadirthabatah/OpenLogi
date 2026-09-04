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

/// One Stream Deck attached over USB.
///
/// Carries `reachable` for the same reason the monitor and light summaries do,
/// though the failure it records is a different one: a Stream Deck is almost
/// always *listed* — enumeration only reads HID descriptors — and then refuses
/// to open, because another program already holds it exclusively. Elgato's own
/// Stream Deck app does that, and so does Logitech's device manager. Saying
/// "found it, could not open it, here is what the transport said" is the only
/// version of that a person can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDeckSummary {
    /// Serial number where the device gives one, which survives a replug.
    pub id: String,
    /// What the operating system calls it.
    pub name: String,
    /// The model name off the box, from the registry.
    pub model: String,
    /// How many keys it has.
    pub keys: u16,
    /// How many rotary dials, which only the Stream Deck Plus has.
    pub dials: u8,
    /// Whether it could actually be opened.
    pub reachable: bool,
    /// Why it could not, when it could not.
    pub unreachable_reason: Option<String>,
}

/// A change to a Stream Deck.
///
/// # Why no brightness is reported back
///
/// A Stream Deck cannot be asked what its brightness is — the protocol has a
/// write and no matching read. So unlike every other write on this wire, the
/// answer to setting it cannot be what the device then reads; it is only
/// confirmation that the write was accepted. The panel that drives this owns
/// the number it last sent and must not pretend it is a read-back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDeckChange {
    /// Screen brightness as a percentage, which the device takes from 0 to 100.
    pub brightness_percent: Option<u8>,
    /// Clear every key back to blank, the way it looks at power-on.
    pub reset: bool,
}

impl StreamDeckChange {
    /// Whether this asks for anything at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.brightness_percent.is_none() && !self.reset
    }
}

/// Why a Stream Deck request could not be answered.
///
/// **Append-only.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamDeckFailure {
    /// No deck with that serial — unplugged since the list was made.
    NotFound,
    /// It is there and would not open, with what the transport said.
    Unreachable(String),
    /// The value was outside what the device takes.
    Refused(String),
    /// The change asked for nothing.
    NothingToDo,
}

impl std::fmt::Display for StreamDeckFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str(
                "that Stream Deck is no longer attached. Searching again will find it if it \
                 came back.",
            ),
            Self::Unreachable(why) | Self::Refused(why) => f.write_str(why),
            Self::NothingToDo => f.write_str("that would not change anything."),
        }
    }
}

/// One audio interface, with what every input on it is doing.
///
/// The whole snapshot rather than a handle to ask further questions through,
/// because the device answers it in one pass and a list assembled from several
/// round trips can disagree with itself halfway down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInterfaceSummary {
    /// Serial number, which is what survives a replug.
    pub id: String,
    /// The model name off the box.
    pub name: String,
    /// The firmware version it reported, which selects its settings table.
    pub firmware: u32,
    /// Whether it is still presenting its registration disk, where the model
    /// has that switch. Not a fault and not a thing to fix — everything works
    /// with it on — but worth showing, because people expect it to matter.
    pub mass_storage: Option<bool>,
    /// One entry per input, counted the way the box labels them.
    pub inputs: Vec<AudioInputSettings>,
    /// Whether it answered.
    pub reachable: bool,
    /// Why it did not, when it did not.
    pub unreachable_reason: Option<String>,
}

/// What one input on an interface is doing.
///
/// Every setting is optional because models differ in which they have, and a
/// `None` says this input has no such control rather than that the read
/// failed — the same distinction [`DisplaySettings`] draws by omitting a
/// control the monitor does not implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInputSettings {
    /// Which input, counted from one the way the box labels them.
    pub input: u16,
    /// Preamp gain, in the interface's own units.
    ///
    /// Deliberately without a maximum beside it, unlike [`DisplayReading`]:
    /// a monitor reports its own ceiling and these interfaces do not. This
    /// desk's Vocaster stores and reads back every byte to 255 without
    /// complaint, so any ceiling shown here would be invented — and a slider
    /// drawn against an invented maximum is confidently wrong at both ends.
    pub gain: Option<u8>,
    /// Whether the input is muted.
    pub muted: Option<bool>,
    /// Whether 48 volt phantom power is on for it.
    pub phantom: Option<bool>,
}

/// A change to one input, with everything not being changed left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInputChange {
    /// New preamp gain.
    pub gain: Option<u8>,
    /// Mute or unmute.
    pub muted: Option<bool>,
    /// Switch 48 volt phantom power on or off.
    pub phantom: Option<bool>,
    /// Whether whoever asked was shown what switching phantom power **on**
    /// costs, and accepted it.
    ///
    /// A separate flag rather than a token, because the proof that carries the
    /// risk cannot cross a wire: it is built at the call site from the risk
    /// itself, precisely so a flag passed from far away cannot conjure one.
    /// What crosses here is the answer to a question; the agent re-derives the
    /// risk for *this* input and builds the acknowledgement beside the write.
    /// Ignored for every other field, and ignored for switching phantom off —
    /// that direction is how somebody makes the interface safe again.
    pub phantom_acknowledged: bool,
}

impl AudioInputChange {
    /// Whether this asks for anything at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.gain.is_none() && self.muted.is_none() && self.phantom.is_none()
    }
}

/// Why an audio-interface request could not be answered.
///
/// **Append-only.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFailure {
    /// No interface with that serial — unplugged since the list was made.
    NotFound,
    /// It is there and did not answer, with what the transport said.
    Unreachable(String),
    /// The change asked for nothing.
    NothingToDo,
    /// Phantom power was asked for without the warning being accepted, and
    /// this is the warning, written to be read aloud.
    NeedsAcknowledgement(String),
    /// The write was refused, with the reason to show.
    Refused(String),
}

impl std::fmt::Display for AudioFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str(
                "that audio interface is no longer attached. Searching again will find it if \
                 it came back.",
            ),
            Self::Unreachable(why) | Self::NeedsAcknowledgement(why) | Self::Refused(why) => {
                f.write_str(why)
            }
            Self::NothingToDo => f.write_str("that would not change anything."),
        }
    }
}

/// One controller — a TourBox — on a serial port.
///
/// It has no settings to read or write: the device streams what its buttons
/// and wheels do, and what those *mean* is this app's config rather than
/// anything stored on the device. So this is identity only, and the panel that
/// shows it says so rather than offering knobs that would do nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerSummary {
    /// The serial port it is on, which is how it is addressed.
    pub id: String,
    /// The model name off the box.
    pub name: String,
    /// How many buttons it has.
    pub buttons: u16,
    /// How many wheels, knobs and dials.
    pub wheels: u16,
    /// Whether it can produce haptic feedback.
    pub haptics: bool,
    /// Serial number where it gives one.
    pub serial_number: Option<String>,
}

/// One VIA-speaking macro pad or keyboard.
///
/// These boards are self-describing — there is no model table behind this,
/// because VIA firmware answers for itself how many layers it has and what
/// every key does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroPadSummary {
    /// Serial number where the board gives one, else its USB identity.
    pub id: String,
    /// What the operating system calls it.
    pub name: String,
    /// USB vendor identifier.
    pub vendor_id: u16,
    /// USB product identifier.
    pub product_id: u16,
    /// Which VIA protocol revision it speaks.
    pub protocol: u16,
    /// How many keymap layers it carries.
    pub layers: u8,
    /// Whether it answered its handshake.
    pub reachable: bool,
    /// Why it did not, when it did not.
    pub unreachable_reason: Option<String>,
}

/// Why a macro-pad request could not be answered.
///
/// **Append-only.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroPadFailure {
    /// No board with that identity — unplugged since the list was made.
    NotFound,
    /// It is there and would not answer, with what the transport said.
    Unreachable(String),
    /// The write was refused, with the reason to show.
    Refused(String),
}

impl std::fmt::Display for MacroPadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str(
                "that keyboard is no longer attached. Searching again will find it if it came \
                 back.",
            ),
            Self::Unreachable(why) | Self::Refused(why) => f.write_str(why),
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

    #[test]
    fn a_stream_deck_reset_is_a_change_even_though_it_carries_no_value() {
        // `reset` is the one field that means something by being true rather
        // than by holding a number, so an emptiness check written only against
        // the Options would call a reset request empty and refuse it.
        assert!(StreamDeckChange::default().is_empty());
        assert!(
            !StreamDeckChange {
                reset: true,
                ..StreamDeckChange::default()
            }
            .is_empty()
        );
        assert!(
            !StreamDeckChange {
                brightness_percent: Some(0),
                ..StreamDeckChange::default()
            }
            .is_empty(),
            "switching the screens off is a change like any other"
        );
    }

    #[test]
    fn acknowledging_phantom_power_is_not_by_itself_a_change() {
        // The flag answers a question about a write; it is not a write. A
        // change carrying nothing but the acknowledgement must still be
        // refused as empty, or a stray confirmation would look like work.
        assert!(
            AudioInputChange {
                phantom_acknowledged: true,
                ..AudioInputChange::default()
            }
            .is_empty()
        );
        assert!(
            !AudioInputChange {
                phantom: Some(false),
                ..AudioInputChange::default()
            }
            .is_empty(),
            "switching phantom power off is a change like any other"
        );
    }

    #[test]
    fn an_unmuted_input_with_no_gain_control_is_not_the_same_as_gain_zero() {
        // `None` says this model has no such control; zero says it has one and
        // it is turned all the way down. A panel that drew them the same would
        // show a dead slider on an input that never had one.
        let settings = AudioInputSettings {
            input: 2,
            gain: None,
            muted: Some(false),
            phantom: None,
        };
        assert!(settings.gain.is_none());
        assert_eq!(settings.muted, Some(false));
    }

    #[test]
    fn the_phantom_warning_is_what_the_refusal_says() {
        // The sentence has to survive the wire intact: it is the whole reason
        // the refusal exists, and a caller that had to invent its own wording
        // would drift from what the command line reads out.
        let said = AudioFailure::NeedsAcknowledgement(
            "This switches 48 volt phantom power on for input pair 1.".to_owned(),
        )
        .to_string();
        assert!(said.contains("48 volt"), "{said}");
    }

    #[test]
    fn a_board_that_did_not_answer_its_handshake_is_still_listed() {
        // Same rule as the light and the deck: a board on the usage page that
        // then would not speak VIA is a thing a person can act on, and its
        // silent absence from the list is not.
        let quiet = MacroPadSummary {
            id: "5343:0080".to_owned(),
            name: "SmartCloud".to_owned(),
            vendor_id: 0x5343,
            product_id: 0x0080,
            protocol: 0,
            layers: 0,
            reachable: false,
            unreachable_reason: Some("did not answer as a VIA device".to_owned()),
        };
        assert!(!quiet.reachable);
        assert_eq!(
            quiet.protocol, 0,
            "nothing was learned, so nothing is shown"
        );
    }
}
