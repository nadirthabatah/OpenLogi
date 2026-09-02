//! Which TourBox is attached, and what it carries.
//!
//! Adding a model that speaks the protocol in [`crate::event`] is an entry
//! in [`MODELS`] and nothing else — the deliberate "no code" path, the same
//! one `roadie-streamdeck` takes for a new Stream Deck generation.

use crate::event::{Button, Wheel};

/// The USB vendor ID a TourBox Elite reports.
///
/// Read off a TourBox Elite rather than taken from a list: the number is not
/// a registered USB-IF vendor ID, so there is no authority to check it
/// against other than the hardware.
pub const TOURBOX_VENDOR_ID: u16 = 0xc251;

/// The USB vendor ID of the Silicon Labs CP210x serial bridge.
///
/// The TourBox Neo reaches the host through one of these rather than through
/// a vendor-specific USB identity. That is why there is no Neo entry in
/// [`MODELS`]: every CP210x-based device in the world shares this vendor ID
/// and a handful of product IDs, so matching on it would claim a great many
/// unrelated serial adapters were TourBoxes. Identifying a Neo needs a
/// protocol handshake, not a number.
pub const CP210X_VENDOR_ID: u16 = 0x10c4;

/// A TourBox model and the controls it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// USB product ID.
    pub product_id: u16,
    /// Marketing name, as it should be read aloud.
    pub name: &'static str,
    /// Buttons this model has.
    pub buttons: &'static [Button],
    /// Wheels, knobs and dials this model has.
    pub wheels: &'static [Wheel],
    /// Whether the model can produce haptic feedback.
    pub haptics: bool,
}

impl Model {
    /// How many separately addressable controls the model has.
    #[must_use]
    pub const fn control_count(&self) -> usize {
        self.buttons.len() + self.wheels.len()
    }
}

/// Every button on a full-size TourBox.
const FULL_BUTTONS: &[Button] = &[
    Button::Tall,
    Button::Side,
    Button::Top,
    Button::Short,
    Button::Scroll,
    Button::Up,
    Button::Down,
    Button::Left,
    Button::Right,
    Button::C1,
    Button::C2,
    Button::Tour,
    Button::Knob,
    Button::Dial,
];

/// Every wheel on a full-size TourBox.
const FULL_WHEELS: &[Wheel] = &[Wheel::Knob, Wheel::Scroll, Wheel::Dial];

/// The models this build recognises by their USB identity.
///
/// Short on purpose. A TourBox is identified here only when its vendor and
/// product IDs are known to belong to a TourBox and nothing else; see
/// [`CP210X_VENDOR_ID`] for the family that cannot be.
pub const MODELS: &[Model] = &[Model {
    product_id: 0x2005,
    name: "TourBox Elite",
    buttons: FULL_BUTTONS,
    wheels: FULL_WHEELS,
    haptics: true,
}];

/// The model behind a USB vendor and product ID, if this build knows it.
#[must_use]
pub fn identify(vendor_id: u16, product_id: u16) -> Option<&'static Model> {
    if vendor_id != TOURBOX_VENDOR_ID {
        return None;
    }
    MODELS.iter().find(|model| model.product_id == product_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identity actually read off the hardware on 2026-08-31.
    #[test]
    fn a_tourbox_elite_is_named_by_its_usb_identity() {
        let model = identify(0xc251, 0x2005).expect("the Elite is in the table");
        assert_eq!(model.name, "TourBox Elite");
        assert!(model.haptics);
    }

    /// The whole point of the vendor check: a CP210x bridge is not a TourBox
    /// just because a TourBox Neo happens to contain one.
    #[test]
    fn a_serial_bridge_is_never_mistaken_for_a_tourbox() {
        assert_eq!(identify(CP210X_VENDOR_ID, 0x2005), None);
        assert_eq!(identify(CP210X_VENDOR_ID, 0xea60), None);
    }

    /// A product ID from the right vendor that this build has never heard of
    /// is unknown, not "probably the Elite".
    #[test]
    fn unknown_hardware_is_never_guessed_at() {
        assert_eq!(identify(TOURBOX_VENDOR_ID, 0x9999), None);
    }

    /// A full-size TourBox has fourteen buttons and three wheels. Pinned
    /// because `control_count` is what the survey reads aloud.
    #[test]
    fn a_full_size_tourbox_carries_seventeen_controls() {
        let model = identify(0xc251, 0x2005).expect("the Elite is in the table");
        assert_eq!(model.buttons.len(), 14);
        assert_eq!(model.wheels.len(), 3);
        assert_eq!(model.control_count(), 17);
    }

    /// A model that named the same control twice would report a count no
    /// desk agrees with.
    #[test]
    fn no_model_names_a_control_twice() {
        for model in MODELS {
            let mut buttons = model.buttons.to_vec();
            buttons.sort_unstable_by_key(|button| button.code());
            let before = buttons.len();
            buttons.dedup_by_key(|button| button.code());
            assert_eq!(buttons.len(), before, "{} repeats a button", model.name);

            let mut wheels = model.wheels.to_vec();
            wheels.sort_unstable_by_key(|wheel| wheel.code());
            let before = wheels.len();
            wheels.dedup_by_key(|wheel| wheel.code());
            assert_eq!(wheels.len(), before, "{} repeats a wheel", model.name);
        }
    }
}
