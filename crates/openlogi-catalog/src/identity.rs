//! What the operating system says a plugged-in thing is.

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
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id, unique within the vendor.
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
            (Some(maker), Some(product)) if !product.starts_with(maker) => {
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

    /// How this device is written down when reporting it, ids included.
    #[must_use]
    pub fn full_description(&self) -> String {
        format!(
            "{} ({:04x}:{:04x})",
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
    pub fn merge_key(&self) -> (u16, u16, Option<&str>) {
        (
            self.vendor_id,
            self.product_id,
            self.serial_number.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Identity;

    fn identity(manufacturer: Option<&str>, product: Option<&str>) -> Identity {
        Identity {
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
