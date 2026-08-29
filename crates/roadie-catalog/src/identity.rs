//! What the operating system says a plugged-in thing is.

/// Which numbering scheme a device's two ids come from.
///
/// They are not one namespace, and this crate's own rule is that vocabularies
/// are converted at the boundary rather than crossed by raw value. A USB
/// vendor id is administered by the USB Implementers Forum. A display's
/// manufacturer code is a PNP id, administered by the UEFI Forum and packed
/// into two bytes of its EDID, and the same number names a different company
/// in each scheme. So the source travels with the numbers instead of being
/// inferred from whatever else happens to be filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IdSource {
    /// USB's vendor and product ids, as the host reports them.
    #[default]
    Usb,
    /// A display's PNP manufacturer code and product code, from its EDID.
    Edid,
}

/// A peripheral's identity, exactly as the OS reports it.
///
/// Every field beyond the two ids is optional because every one of them is
/// optional in reality: plenty of devices report no manufacturer string, and
/// many report no serial number at all. Modelling that as `Option` rather than
/// an empty string keeps "the device did not say" distinguishable from "the
/// device said nothing", which is the difference between a gap in the data and
/// a gap in our reading of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identity {
    /// Which scheme [`Self::vendor_id`] and [`Self::product_id`] belong to.
    pub ids: IdSource,
    /// Vendor id, in the scheme [`Self::ids`] names.
    pub vendor_id: u16,
    /// Product id, unique within the vendor, in that same scheme.
    pub product_id: u16,
    /// The product name the OS reports, when it reports one.
    pub product: Option<String>,
    /// The manufacturer name the OS reports, when it reports one.
    ///
    /// This is why there is no hardcoded vendor-id table in this crate. The
    /// device already carries its maker's name; a table of guessed ids would
    /// be a second, worse source for something the hardware states directly,
    /// and would go stale the day a vendor ships under a new id.
    pub manufacturer: Option<String>,
    /// Serial number, when the device reports one and the OS exposes it.
    pub serial_number: Option<String>,
}

impl Identity {
    /// The best name available for this device, for reading aloud or printing.
    ///
    /// Falls back through product name, manufacturer, and finally the raw ids.
    /// The ids are never *only* a fallback flourish: they are what a device
    /// support request needs, so a device we cannot name is still a device you
    /// can report.
    #[must_use]
    pub fn describe(&self) -> String {
        match (self.manufacturer.as_deref(), self.product.as_deref()) {
            (Some(maker), Some(product)) if !Self::already_names(product, maker) => {
                format!("{maker} {product}")
            }
            (_, Some(product)) => product.to_owned(),
            (Some(maker), None) => format!("{maker} device"),
            (None, None) => format!(
                "unnamed device {:04x}:{:04x}",
                self.vendor_id, self.product_id
            ),
        }
    }

    /// Whether a product string already carries its maker's name.
    ///
    /// USB string descriptors are written by whoever wired up the firmware, so
    /// the two strings agree on the maker's name about as often as not:
    /// `ELGATO` beside `Elgato Stream Deck`, `Logitech` beside `logitech
    /// StreamCam`, `Logitech Inc.` beside `Logitech USB Receiver`. Comparing
    /// them literally puts the name in twice, and "ELGATO Elgato Stream Deck"
    /// read aloud sounds like the program is stuttering.
    ///
    /// Two rules, both from what descriptors actually look like: ignore case,
    /// and compare first words, so a maker who writes `Inc.` after their name
    /// in one string and not the other still matches.
    fn already_names(product: &str, maker: &str) -> bool {
        let maker = maker.trim();
        if maker.is_empty() {
            return true;
        }
        if product.len() >= maker.len() && product[..maker.len()].eq_ignore_ascii_case(maker) {
            return true;
        }
        match (first_word(product), first_word(maker)) {
            (Some(theirs), Some(ours)) => theirs.eq_ignore_ascii_case(ours),
            _ => false,
        }
    }

    /// How this device is written down when reporting it, ids included.
    #[must_use]
    pub fn full_description(&self) -> String {
        // The scheme is said aloud for a display, because a bare pair of hex
        // numbers after a name reads as a USB id, and a PNP code that looked
        // like one would send someone searching a database it is not in.
        let scheme = match self.ids {
            IdSource::Usb => "",
            IdSource::Edid => "display ",
        };
        format!(
            "{} ({scheme}{:04x}:{:04x})",
            self.describe(),
            self.vendor_id,
            self.product_id
        )
    }

    /// The key that decides whether two enumerated entries are one device.
    ///
    /// A single physical peripheral shows up once per HID collection, so a
    /// Stream Deck can appear three times in a raw enumeration. Listing it
    /// three times would make a five-device desk look like a fifteen-device
    /// one — worse than useless to someone reading the list aloud. The serial
    /// number separates two identical devices when it is reported; when it is
    /// not, two identical devices merge into one entry, which understates the
    /// count rather than inventing devices that are not there.
    #[must_use]
    pub fn merge_key(&self) -> (IdSource, u16, u16, Option<&str>) {
        (
            // The scheme is part of the key for the same reason it is part of
            // the identity: without it a display whose PNP code happened to
            // equal some USB vendor id would merge into that device and
            // disappear from the survey.
            self.ids,
            self.vendor_id,
            self.product_id,
            self.serial_number.as_deref(),
        )
    }
}

/// The first word of a string, without the punctuation stuck to it.
///
/// "Logitech, Inc." and "Logitech Inc." are the same company writing its own
/// name two ways, and a comma is not a reason to print that name twice.
fn first_word(text: &str) -> Option<&str> {
    let word = text
        .split_whitespace()
        .next()?
        .trim_end_matches(|c: char| !c.is_alphanumeric());
    (!word.is_empty()).then_some(word)
}

#[cfg(test)]
mod tests {
    use super::{IdSource, Identity};

    fn identity(manufacturer: Option<&str>, product: Option<&str>) -> Identity {
        Identity {
            ids: IdSource::Usb,
            vendor_id: 0x046d,
            product_id: 0xc52b,
            product: product.map(str::to_owned),
            manufacturer: manufacturer.map(str::to_owned),
            serial_number: None,
        }
    }

    #[test]
    fn a_maker_and_a_product_read_as_one_name() {
        assert_eq!(
            identity(Some("Elgato"), Some("Stream Deck")).describe(),
            "Elgato Stream Deck"
        );
    }

    /// Plenty of devices report a product string that already opens with the
    /// maker's name. Prefixing it again gives "Logitech Logitech StreamCam",
    /// which sounds like a bug to anyone hearing the list read out.
    #[test]
    fn a_product_that_already_names_its_maker_is_not_doubled() {
        assert_eq!(
            identity(Some("Logitech"), Some("Logitech StreamCam")).describe(),
            "Logitech StreamCam"
        );
    }

    /// USB string descriptors are written by whoever wired up the firmware,
    /// and the two strings agree on the maker's name about as often as not.
    /// Comparing them literally puts the name in twice, and "ELGATO Elgato
    /// Stream Deck" read aloud sounds like the program is stuttering.
    ///
    /// Every pair here is the shape a real descriptor takes.
    #[test]
    fn a_maker_named_twice_is_not_said_twice() {
        for (maker, product, expected) in [
            // Manufacturer in capitals, product in title case.
            ("ELGATO", "Elgato Stream Deck", "Elgato Stream Deck"),
            // Product in lower case.
            ("Logitech", "logitech StreamCam", "logitech StreamCam"),
            // A legal suffix on one string and not the other.
            (
                "Logitech Inc.",
                "Logitech USB Receiver",
                "Logitech USB Receiver",
            ),
            (
                "Logitech, Inc.",
                "Logitech USB Receiver",
                "Logitech USB Receiver",
            ),
            // Already correct, and must stay correct.
            ("Logitech", "Logitech StreamCam", "Logitech StreamCam"),
        ] {
            assert_eq!(
                identity(Some(maker), Some(product)).describe(),
                expected,
                "{maker:?} + {product:?}"
            );
        }
    }

    /// And a product that genuinely does not name its maker still gets it.
    #[test]
    fn a_product_that_does_not_name_its_maker_is_given_one() {
        assert_eq!(
            identity(Some("Elgato"), Some("Stream Deck MK.2")).describe(),
            "Elgato Stream Deck MK.2"
        );
        assert_eq!(
            identity(Some("Focusrite"), Some("Scarlett 2i2")).describe(),
            "Focusrite Scarlett 2i2"
        );
    }

    /// A manufacturer string of only spaces is no manufacturer at all, and
    /// must not produce a name with a hole at the front of it.
    #[test]
    fn a_blank_manufacturer_adds_nothing() {
        assert_eq!(
            identity(Some("   "), Some("Stream Deck")).describe(),
            "Stream Deck"
        );
    }

    #[test]
    fn a_product_alone_is_enough() {
        assert_eq!(
            identity(None, Some("MX Master 3S")).describe(),
            "MX Master 3S"
        );
    }

    #[test]
    fn a_maker_alone_still_says_something() {
        assert_eq!(identity(Some("Obsbot"), None).describe(), "Obsbot device");
    }

    /// The case that matters most: a device that tells us nothing is still
    /// listed, with the two numbers a support request needs.
    #[test]
    fn a_nameless_device_is_still_identifiable() {
        assert_eq!(identity(None, None).describe(), "unnamed device 046d:c52b");
    }

    #[test]
    fn the_full_description_always_carries_the_ids() {
        assert_eq!(
            identity(Some("Elgato"), Some("Stream Deck")).full_description(),
            "Elgato Stream Deck (046d:c52b)"
        );
    }

    #[test]
    fn collections_of_one_device_share_a_merge_key() {
        let first = identity(Some("Elgato"), Some("Stream Deck"));
        let mut second = identity(Some("Elgato"), Some("Stream Deck"));
        second.product = Some("Stream Deck (Consumer Control)".to_owned());
        assert_eq!(first.merge_key(), second.merge_key());
    }

    #[test]
    fn two_of_the_same_model_stay_apart_when_they_report_serials() {
        let mut first = identity(Some("Elgato"), Some("Stream Deck"));
        first.serial_number = Some("AL1".to_owned());
        let mut second = identity(Some("Elgato"), Some("Stream Deck"));
        second.serial_number = Some("AL2".to_owned());
        assert_ne!(first.merge_key(), second.merge_key());
    }
}

#[cfg(test)]
mod id_source_tests {
    use super::{IdSource, Identity};

    /// A display's identity, as `roadie devices` builds one from an EDID.
    fn display() -> Identity {
        Identity {
            ids: IdSource::Edid,
            vendor_id: 0x1E6D,
            product_id: 0x5B11,
            product: Some("ULTRAFINE".to_owned()),
            manufacturer: Some("LG".to_owned()),
            serial_number: None,
        }
    }

    #[test]
    fn a_displays_ids_are_not_read_as_usb_ids() {
        // A bare hex pair after a name reads as a USB id, and a PNP code that
        // looked like one would send someone searching a database it is not in.
        assert_eq!(
            display().full_description(),
            "LG ULTRAFINE (display 1e6d:5b11)"
        );
    }

    #[test]
    fn a_usb_device_says_its_ids_plainly() {
        let camera = Identity {
            ids: IdSource::Usb,
            vendor_id: 0x046D,
            product_id: 0x0893,
            product: Some("StreamCam".to_owned()),
            manufacturer: Some("Logitech".to_owned()),
            serial_number: None,
        };
        assert_eq!(camera.full_description(), "Logitech StreamCam (046d:0893)");
    }

    #[test]
    fn a_display_never_merges_into_a_usb_device_that_shares_its_numbers() {
        // The two schemes are administered by different bodies, so the same
        // pair of numbers can legitimately appear in both. Without the scheme
        // in the key, one of the two devices would vanish from the survey.
        let collision = Identity {
            ids: IdSource::Usb,
            ..display()
        };
        assert_ne!(display().merge_key(), collision.merge_key());
    }

    #[test]
    fn two_of_the_same_monitor_stay_two_when_they_carry_serial_numbers() {
        let first = Identity {
            serial_number: Some("12345".to_owned()),
            ..display()
        };
        let second = Identity {
            serial_number: Some("67890".to_owned()),
            ..display()
        };
        assert_ne!(first.merge_key(), second.merge_key());
    }

    #[test]
    fn a_display_with_no_edid_still_has_something_to_call_it() {
        // A connected connector whose EDID is empty is a real state: the
        // kernel reports the link before it has read the block, and some KVM
        // switches never let it.
        let nameless = Identity {
            ids: IdSource::Edid,
            product: Some("card0-DP-1".to_owned()),
            ..Identity::default()
        };
        assert_eq!(nameless.describe(), "card0-DP-1");
    }
}
