//! What a light says it is.
//!
//! `GET /elgato/accessory-info` answers with the model, the firmware, a serial
//! number and the name someone gave it in Elgato's app. That last one is the
//! useful part: a desk with two Key Lights has them named "left" and "right"
//! by whoever set them up, and those are the words that person will use.

use serde::{Deserialize, Serialize};

/// A light's identity, as it reports it.
///
/// Every field is optional because the firmware's answer has grown over the
/// years and older units send less of it. A missing field is a device that did
/// not say, which is different from a device that said nothing — and modelling
/// it as an empty string would lose that.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessoryInfo {
    /// The model, as Elgato names it: "Elgato Key Light Air", and so on.
    #[serde(rename = "productName")]
    pub product_name: Option<String>,
    /// The name someone gave this light in Elgato's app.
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    /// The serial number, which is what tells two identical lights apart.
    #[serde(rename = "serialNumber")]
    pub serial_number: Option<String>,
    /// The firmware version, for a bug report.
    #[serde(rename = "firmwareVersion")]
    pub firmware_version: Option<String>,
    /// What this unit can do. Every light so far says `["lights"]`.
    #[serde(default)]
    pub features: Vec<String>,
    /// How the light is being powered, and what that limits.
    ///
    /// Sent by the Key Light Neo, whose brightness ceiling depends on its
    /// power source — 400-lumen territory on a computer's USB-A port, full
    /// output only on a mains supply. Absent on the mains-powered family.
    #[serde(rename = "power-info")]
    pub power_info: Option<PowerInfo>,
}

/// What a light says about its power source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerInfo {
    /// A firmware-defined mode number; meaning unmapped so far.
    #[serde(rename = "operationMode")]
    pub operation_mode: Option<u16>,
    /// The highest brightness percentage the current power source allows.
    ///
    /// A request above it is *refused* by the firmware, not clamped — the
    /// Neo on this project's desk answered 50 percent with "Invalid
    /// parameters" while reporting a 40 percent ceiling — so whoever asks
    /// too much needs this number to know what to ask instead.
    #[serde(rename = "maximumBrightness")]
    pub maximum_brightness: Option<u16>,
}

impl AccessoryInfo {
    /// The name to print or speak.
    ///
    /// The name someone gave the light wins over the model, because a desk
    /// with two of them has them called "key left" and "key right" and those
    /// are the words that person will use. The model is the fallback, and the
    /// last resort says plainly that the light did not give a name rather than
    /// inventing one.
    #[must_use]
    pub fn describe(&self) -> String {
        match (self.display_name.as_deref(), self.product_name.as_deref()) {
            (Some(given), _) if !given.trim().is_empty() => given.trim().to_owned(),
            (_, Some(model)) if !model.trim().is_empty() => model.trim().to_owned(),
            _ => "an unnamed Elgato light".to_owned(),
        }
    }

    /// Whether this device controls lights at all.
    ///
    /// Elgato's mDNS service covers more than the light family, so a device
    /// answering on it is not necessarily one of these. A unit that does not
    /// list the feature is left alone rather than being sent commands it has
    /// no meaning for.
    #[must_use]
    pub fn controls_lights(&self) -> bool {
        // An older unit that lists nothing at all is assumed to be a light:
        // the field arrived after the first models, and refusing those would
        // drop working hardware to satisfy a newer field.
        self.features.is_empty() || self.features.iter().any(|feature| feature == "lights")
    }
}

#[cfg(test)]
mod tests;
