//! What a Key Light is doing, and what it will accept.
//!
//! The wire shape is small enough to state completely:
//!
//! ```json
//! {"numberOfLights":1,"lights":[{"on":1,"brightness":20,"temperature":213}]}
//! ```
//!
//! Three fields, and every one of them has an edge worth knowing about.

use serde::{Deserialize, Serialize};

/// An inclusive range the light accepts, in its own units.
///
/// Carried as a type rather than as two loose numbers because every one of
/// these is a clamp waiting to be forgotten, and the interesting operations —
/// clamping, and asking whether a value was changed on the way in — belong
/// with the bounds rather than at each call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    /// The lowest value the light accepts.
    pub low: u16,
    /// The highest value the light accepts.
    pub high: u16,
}

impl Range {
    /// `value`, brought inside the range.
    #[must_use]
    pub const fn clamp(self, value: u16) -> u16 {
        if value < self.low {
            self.low
        } else if value > self.high {
            self.high
        } else {
            value
        }
    }

    /// Whether `value` is already inside the range.
    ///
    /// Separate from [`Self::clamp`] so a caller can *say* that it clamped.
    /// Silently accepting 0 and setting 3 is how someone ends up believing a
    /// light is off when it is at its dimmest — which, for a light aimed at a
    /// face, is a difference they cannot see and everyone else can.
    #[must_use]
    pub const fn holds(self, value: u16) -> bool {
        value >= self.low && value <= self.high
    }
}

/// Brightness, as a percentage.
///
/// The floor is 3 rather than 0, and that is the light's own limit rather than
/// a choice here: a Key Light will not go below three percent while it is on.
/// Off is a separate field, which is the right model — a light at three
/// percent is on, and looks it.
pub const BRIGHTNESS: Range = Range { low: 3, high: 100 };

/// Colour temperature, in mireds, as the light counts them.
///
/// Reversed against Kelvin: 143 mireds is the coldest the light goes and 344
/// the warmest. Written as the light's own units rather than converted at the
/// edges of this constant, so that the one place the reversal happens is
/// [`kelvin_to_mired`].
pub const TEMPERATURE: Range = Range {
    low: 143,
    high: 344,
};

/// The coldest and warmest the light goes, in Kelvin, for saying out loud.
///
/// Derived from [`TEMPERATURE`] rather than written down twice, and checked
/// against Elgato's published figures in this module's tests. Their app says
/// 2900 to 7000; the reciprocal of the mired range gives 2907 and 6993, and
/// the difference is the coarseness of the mired scale rather than
/// disagreement about what the light does.
#[must_use]
pub fn kelvin_range() -> Range {
    Range {
        low: mired_to_kelvin(TEMPERATURE.high),
        high: mired_to_kelvin(TEMPERATURE.low),
    }
}

/// The scale factor between the two units: one million reciprocal kelvin.
const MEGAKELVIN: u32 = 1_000_000;

/// Kelvin from mireds, rounded to nearest.
///
/// Rounded rather than truncated, and it is worth a sentence why. The mired
/// scale has about two hundred steps across the light's whole range, and they
/// are not evenly spaced in Kelvin: one step near the warm end is about ten
/// Kelvin, and one near the cold end is about forty. Truncating biases every
/// conversion the same direction, so the error compounds across a round trip;
/// rounding halves the worst case, from 48 K to 24 K across the range, and
/// puts the endpoints at 2907 K and 6993 K against the 2900 and 7000 Elgato
/// advertises.
///
/// Saturates rather than dividing by zero. A mired of zero is not a
/// temperature the light can report, but arithmetic that panics on data from
/// a device is arithmetic that will eventually panic on data from a device.
#[must_use]
pub fn mired_to_kelvin(mired: u16) -> u16 {
    if mired == 0 {
        return u16::MAX;
    }
    let mired = u32::from(mired);
    // Saturating rather than casting: below about sixteen mireds the
    // reciprocal is larger than a u16 holds, and `try_from` states that bound
    // instead of a cast asserting it in a comment.
    u16::try_from((MEGAKELVIN + mired / 2) / mired).unwrap_or(u16::MAX)
}

/// Mireds from Kelvin, rounded to nearest and clamped to what the light
/// accepts.
///
/// Rounded for the reason given on [`mired_to_kelvin`].
///
/// The clamp is deliberate and is not a silent one: [`TEMPERATURE`] is public
/// and [`Range::holds`] exists so a caller can tell someone their 8000 K
/// became 6993 K. Sending an out-of-range value unclamped is worse — the
/// light rejects the whole request, so an ambitious temperature would also
/// silently discard the brightness sent with it.
#[must_use]
pub fn kelvin_to_mired(kelvin: u16) -> u16 {
    if kelvin == 0 {
        return TEMPERATURE.high;
    }
    let kelvin = u32::from(kelvin);
    let mired = u16::try_from((MEGAKELVIN + kelvin / 2) / kelvin).unwrap_or(u16::MAX);
    TEMPERATURE.clamp(mired)
}

/// One light's state, exactly as it appears on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Light {
    /// 1 when the light is on, 0 when it is off.
    ///
    /// Kept as the number the firmware sends rather than converted to a
    /// `bool` at the serde boundary, because this type *is* the wire shape and
    /// a `bool` would serialise as `true` — which the light rejects.
    /// [`Light::is_on`] and [`Light::set_on`] are the interface.
    pub on: u8,
    /// Brightness as a percentage; see [`BRIGHTNESS`].
    pub brightness: u16,
    /// Colour temperature in mireds; see [`TEMPERATURE`].
    pub temperature: u16,
}

impl Light {
    /// Whether the light is on.
    ///
    /// Anything other than zero counts as on. The firmware only ever sends 0
    /// or 1, but treating an unexpected number as off would mean a light that
    /// is visibly lit gets reported as dark, which is exactly the wrong way
    /// round for someone who cannot check by looking.
    #[must_use]
    pub const fn is_on(self) -> bool {
        self.on != 0
    }

    /// The same light, switched.
    #[must_use]
    pub const fn set_on(mut self, on: bool) -> Self {
        self.on = on as u8;
        self
    }

    /// The same light at `percent` brightness, clamped to what it accepts.
    #[must_use]
    pub const fn set_brightness(mut self, percent: u16) -> Self {
        self.brightness = BRIGHTNESS.clamp(percent);
        self
    }

    /// The same light at `kelvin`, clamped to what it accepts.
    #[must_use]
    pub fn set_kelvin(mut self, kelvin: u16) -> Self {
        self.temperature = kelvin_to_mired(kelvin);
        self
    }

    /// This light's colour temperature in Kelvin.
    #[must_use]
    pub fn kelvin(self) -> u16 {
        mired_to_kelvin(self.temperature)
    }
}

/// Every light on one device.
///
/// A Key Light is one light and a Light Strip is one light, but the firmware
/// models a list and says how long it is, so this does too. Assuming one would
/// work on every unit sold today and break on the first that is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lights {
    /// How many lights the device carries.
    ///
    /// Defaulted on the way in because the field is not always sent: the
    /// Key Light Neo on this project's desk answers a USB write with only
    /// the `lights` list, though the same firmware includes the count when
    /// read. Writes always send it, and a reply's missing count defaults to
    /// zero rather than failing the parse — the list itself is the answer.
    #[serde(rename = "numberOfLights", default)]
    pub number_of_lights: u16,
    /// The lights themselves.
    pub lights: Vec<Light>,
}

impl Lights {
    /// One light's worth of state, ready to `PUT`.
    #[must_use]
    pub fn one(light: Light) -> Self {
        Self {
            number_of_lights: 1,
            lights: vec![light],
        }
    }

    /// The first light, which on every unit sold today is the only one.
    ///
    /// # Errors
    ///
    /// Returns [`LightError::NoLights`] when the device reported none, which
    /// is a device answering the right endpoint with nothing in it rather than
    /// a transport failure — and is therefore worth telling apart.
    pub fn first(&self) -> Result<Light, LightError> {
        self.lights.first().copied().ok_or(LightError::NoLights)
    }
}

/// What a light's own answer can be wrong about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LightError {
    /// The device answered, and listed no lights at all.
    #[error(
        "the light answered but reported no lights, which is a firmware fault rather than \
             a connection problem"
    )]
    NoLights,
}

/// A refusal, as the firmware phrases one.
///
/// The Key Light Neo answers a request it will not honour with this shape
/// instead of a state — observed on this project's desk when a brightness
/// beyond its USB power budget was asked for:
///
/// ```json
/// {"errors":[{"message":"Invalid parameters","code":-1}]}
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReply {
    /// The refusals, in the firmware's words.
    pub errors: Vec<DeviceError>,
}

/// One refusal inside an [`ErrorReply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceError {
    /// The firmware's own words for what was wrong.
    #[serde(default)]
    pub message: String,
    /// The firmware's error code, for a bug report.
    #[serde(default)]
    pub code: i32,
}

impl ErrorReply {
    /// The refusals as one spoken sentence fragment.
    #[must_use]
    pub fn describe(&self) -> String {
        if self.errors.is_empty() {
            return "an error it did not explain".to_owned();
        }
        self.errors
            .iter()
            .map(|error| error.message.trim())
            .filter(|message| !message.is_empty())
            .collect::<Vec<_>>()
            .join(", and ")
    }
}

#[cfg(test)]
mod tests;
