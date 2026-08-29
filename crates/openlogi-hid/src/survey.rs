//! Everything plugged in, whoever made it.
//!
//! `openlogi list` answers "which Logitech devices are paired", which is the
//! question upstream set out to answer. This module answers a different one:
//! *what is on this desk, and what can this program do with each of it?* That
//! is the question a hub has to answer, and the answer has to include the
//! devices it cannot help with — a list that silently omits your audio
//! interface tells you nothing about why you cannot find it.
//!
//! Enumeration is the only part here that needs a machine with USB. The
//! reshaping of it — collapsing the several HID collections one device
//! exposes into one entry, and keeping the verdict that says the most — is
//! pure and lives in [`merge`], where it is tested.

use openlogi_catalog::{Identity, Peripheral};
use openlogi_device::backend::BackendError;

use crate::transport::enumerate_devices;

/// Every HID device attached, classified, one entry per physical device.
///
/// # Errors
///
/// [`BackendError`] if the platform's HID stack cannot be enumerated.
pub async fn hid_peripherals() -> Result<Vec<Peripheral>, BackendError> {
    let reported: Vec<Reported> = enumerate_devices()
        .await?
        .into_iter()
        .map(|device| Reported {
            vendor_id: device.vendor_id,
            product_id: device.product_id,
            name: device.name.clone(),
            manufacturer: device.manufacturer.clone(),
            serial_number: device.serial_number.clone(),
            usage_page: device.usage_page,
            usage_id: device.usage_id,
        })
        .collect();
    Ok(survey_of(&reported))
}

/// One HID collection, as the OS describes it.
///
/// The metadata half of an enumerated node, without the open handle. The
/// handle is what makes a real node impossible to construct in a test, and
/// nothing about deciding *what a device is* needs it — so the decision runs
/// over this instead, and the part of the survey that chooses what you are
/// shown can be checked on a machine with nothing plugged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// The product name the OS reports.
    pub name: String,
    /// The manufacturer name, when the OS reports one.
    pub manufacturer: Option<String>,
    /// The serial number, when the OS reports one.
    pub serial_number: Option<String>,
    /// HID usage page of this collection.
    pub usage_page: u16,
    /// HID usage id of this collection.
    pub usage_id: u16,
}

/// The whole survey, over what the OS said rather than over open handles.
///
/// Everything `hid_peripherals` does except the enumeration itself: identify
/// each collection, classify it, and collapse the several collections of one
/// device into a single entry.
#[must_use]
pub fn survey_of(reported: &[Reported]) -> Vec<Peripheral> {
    let collections = reported
        .iter()
        .map(|node| {
            let identity = identity_of(
                node.vendor_id,
                node.product_id,
                &node.name,
                node.manufacturer.as_deref(),
                node.serial_number.as_deref(),
            );
            Peripheral::from_hid(identity, node.usage_page, node.usage_id)
        })
        .collect();
    merge(collections)
}

/// Build an identity from what the OS reported about one node.
///
/// Extracted from the enumeration so the mapping is checkable: enumeration
/// needs a machine with USB, and this does not. What it guards against is
/// dull and easy — the product and manufacturer strings swapping places, or a
/// serial landing in the wrong field — which is exactly the sort of mistake
/// that survives review and then renames every device in the list.
fn identity_of(
    vendor_id: u16,
    product_id: u16,
    name: &str,
    manufacturer: Option<&str>,
    serial_number: Option<&str>,
) -> Identity {
    Identity {
        vendor_id,
        product_id,
        product: non_empty(name),
        manufacturer: manufacturer.and_then(non_empty),
        serial_number: serial_number.and_then(non_empty),
    }
}

/// A reported string, or `None` when the OS reported an empty one.
///
/// Backends differ on whether an absent string arrives as `None` or as `""`,
/// and an empty product name rendered as a name gives a blank line in the
/// listing — a device that appears to have no identity at all.
fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Collapse the collections of one physical device into a single entry.
///
/// Order is by identity rather than by enumeration, so the same desk produces
/// the same list twice running. That matters more here than it looks: a list
/// read aloud, or diffed between two machines while moving a setup across, is
/// only useful if its order is stable.
#[must_use]
pub fn merge(found: Vec<Peripheral>) -> Vec<Peripheral> {
    let mut merged: Vec<Peripheral> = Vec::new();
    for candidate in found {
        let seen = merged
            .iter()
            .position(|seen| seen.identity.merge_key() == candidate.identity.merge_key());
        match seen {
            // `swap_remove` disturbs the order, which the sort below restores
            // anyway; taking the entry by value is what lets the two verdicts
            // be compared rather than one blindly overwriting the other.
            Some(index) => {
                let existing = merged.swap_remove(index);
                merged.push(existing.merge(candidate));
            }
            None => merged.push(candidate),
        }
    }
    merged.sort_by(|a, b| {
        (
            a.identity.vendor_id,
            a.identity.product_id,
            a.identity.serial_number.as_deref(),
        )
            .cmp(&(
                b.identity.vendor_id,
                b.identity.product_id,
                b.identity.serial_number.as_deref(),
            ))
    });
    merged
}

#[cfg(test)]
mod tests {
    use openlogi_catalog::{Driver, Identity, Peripheral, Support};

    use super::{Reported, identity_of, merge, non_empty, survey_of};

    fn peripheral(
        vendor_id: u16,
        product_id: u16,
        serial: Option<&str>,
        support: Support,
    ) -> Peripheral {
        Peripheral {
            identity: Identity {
                vendor_id,
                product_id,
                serial_number: serial.map(str::to_owned),
                ..Identity::default()
            },
            support,
        }
    }

    fn configurable() -> Support {
        Support::Driver {
            driver: Driver::HidPlusPlus,
            model: None,
        }
    }

    /// The whole point of merging: one Stream Deck exposing three collections
    /// must not read as three Stream Decks. Someone hearing this list aloud
    /// would have no way to tell that was a bug rather than their own desk.
    #[test]
    fn the_collections_of_one_device_become_one_entry() {
        let merged = merge(vec![
            peripheral(0x0fd9, 0x0080, None, Support::Unsupported),
            peripheral(0x0fd9, 0x0080, None, Support::Unsupported),
            peripheral(0x0fd9, 0x0080, None, Support::Unsupported),
        ]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn merging_keeps_the_collection_that_can_be_configured() {
        let merged = merge(vec![
            peripheral(0x046d, 0x4082, None, Support::Unsupported),
            peripheral(0x046d, 0x4082, None, configurable()),
        ]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].support.is_configurable());
    }

    /// Two identical mice that report serials are two mice. Collapsing them
    /// would tell someone they own one fewer device than they do.
    #[test]
    fn two_of_the_same_model_with_serials_stay_two_devices() {
        let merged = merge(vec![
            peripheral(0x046d, 0x4082, Some("A"), configurable()),
            peripheral(0x046d, 0x4082, Some("B"), configurable()),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn the_order_does_not_depend_on_the_order_things_were_enumerated() {
        let one = peripheral(0x0fd9, 0x0080, None, configurable());
        let two = peripheral(0x046d, 0x4082, None, configurable());
        let three = peripheral(0x1234, 0x5678, None, Support::Unsupported);
        let forwards = merge(vec![one.clone(), two.clone(), three.clone()]);
        let backwards = merge(vec![three, two, one]);
        assert_eq!(forwards, backwards);
        assert_eq!(forwards[0].identity.vendor_id, 0x046d);
    }

    /// An unsupported device is never dropped. It is the answer to "why is my
    /// interface not in the list", and it is how the list of what to support
    /// next gets written.
    #[test]
    fn an_unsupported_device_survives_merging() {
        let merged = merge(vec![peripheral(0x1235, 0x8210, None, Support::Unsupported)]);
        assert_eq!(merged.len(), 1);
        assert!(!merged[0].support.is_configurable());
    }

    /// Every field where it belongs. Dull, and the kind of mistake that
    /// survives review and then renames every device in the list.
    #[test]
    fn every_reported_field_lands_in_its_own_place() {
        let identity = identity_of(
            0x046d,
            0x4082,
            "MX Master 3S",
            Some("Logitech"),
            Some("AB12"),
        );
        assert_eq!(identity.vendor_id, 0x046d);
        assert_eq!(identity.product_id, 0x4082);
        assert_eq!(identity.product.as_deref(), Some("MX Master 3S"));
        assert_eq!(identity.manufacturer.as_deref(), Some("Logitech"));
        assert_eq!(identity.serial_number.as_deref(), Some("AB12"));
    }

    /// Absent and empty are the same thing here, and both have to reach the
    /// naming logic as `None` — an empty product name rendered as a name is a
    /// blank line where a device should be.
    #[test]
    fn absent_and_empty_reported_strings_both_become_nothing() {
        let absent = identity_of(0x046d, 0x4082, "", None, None);
        assert_eq!(absent.product, None);
        assert_eq!(absent.manufacturer, None);
        assert_eq!(absent.serial_number, None);

        let blank = identity_of(0x046d, 0x4082, "  ", Some(""), Some("   "));
        assert_eq!(blank, absent, "whitespace is not a name");
        assert_eq!(blank.describe(), "unnamed device 046d:4082");
    }

    #[test]
    fn an_empty_reported_string_is_treated_as_no_string() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("   "), None);
        assert_eq!(non_empty(" MX Master 3S "), Some("MX Master 3S".to_owned()));
    }

    fn reported(
        vendor_id: u16,
        product_id: u16,
        name: &str,
        usage_page: u16,
        usage_id: u16,
    ) -> Reported {
        Reported {
            vendor_id,
            product_id,
            name: name.to_owned(),
            manufacturer: None,
            serial_number: None,
            usage_page,
            usage_id,
        }
    }

    /// A desk as the operating system actually reports it: several devices,
    /// most of them exposing more than one collection, arriving in no
    /// particular order.
    ///
    /// The pieces of this are each tested on their own. This checks that the
    /// whole thing composes into the answer a person would expect to hear,
    /// which is the product's headline question and was the one step nothing
    /// covered — because enumeration needs a machine with USB and this does
    /// not.
    fn a_realistic_desk() -> Vec<Reported> {
        vec![
            // A Stream Deck MK.2: three collections, one of them the vendor
            // page the driver actually talks over.
            reported(0x0fd9, 0x0080, "Stream Deck MK.2", 0x0001, 0x0006),
            reported(0x0fd9, 0x0080, "Stream Deck MK.2", 0x000c, 0x0001),
            reported(0x0fd9, 0x0080, "Stream Deck MK.2", 0xff00, 0x0001),
            // A Logitech Unifying receiver, and a mouse behind it: the mouse
            // shows a plain mouse collection as well as its HID++ one.
            reported(0x046d, 0xc52b, "USB Receiver", 0xff00, 0x0002),
            reported(0x046d, 0x4082, "MX Master 3S", 0x0001, 0x0002),
            reported(0x046d, 0x4082, "MX Master 3S", 0xff00, 0x0002),
            // A QMK macro pad: a keyboard collection and the VIA one.
            reported(0x4653, 0x0001, "Some Macro Pad", 0x0001, 0x0006),
            reported(0x4653, 0x0001, "Some Macro Pad", 0xff60, 0x0061),
            // And an audio interface nothing here drives.
            reported(0x1235, 0x8210, "Scarlett 2i2", 0x0001, 0x0000),
        ]
    }

    #[test]
    fn a_realistic_desk_collapses_to_the_devices_on_it() {
        let found = survey_of(&a_realistic_desk());
        let names: Vec<&str> = found
            .iter()
            .map(|found| found.identity.product.as_deref().unwrap_or(""))
            .collect();
        // Nine collections are five devices, ordered by vendor and product id:
        // Logitech (046d) before Elgato (0fd9) before Focusrite (1235) before
        // the macro pad (4653). Ordering by identity rather than by the order
        // the OS happened to report things is what makes the list the same
        // twice running, which matters when it is read aloud or diffed between
        // two machines while a setup is being moved.
        assert_eq!(
            names,
            vec![
                "MX Master 3S",
                "USB Receiver",
                "Stream Deck MK.2",
                "Scarlett 2i2",
                "Some Macro Pad",
            ],
        );
    }

    #[test]
    fn each_device_on_a_realistic_desk_is_classified_correctly() {
        let found = survey_of(&a_realistic_desk());
        let by_name = |wanted: &str| {
            found
                .iter()
                .find(|found| found.identity.product.as_deref() == Some(wanted))
                .unwrap_or_else(|| panic!("{wanted} is missing from the survey"))
                .support
                .clone()
        };

        assert!(
            matches!(
                by_name("Stream Deck MK.2"),
                Support::Driver {
                    driver: Driver::StreamDeck,
                    ..
                }
            ),
            "the plain collections must not outvote the vendor one"
        );
        assert!(
            matches!(
                by_name("MX Master 3S"),
                Support::Driver {
                    driver: Driver::HidPlusPlus,
                    ..
                }
            ),
            "the mouse collection must not outvote the HID++ one"
        );
        assert!(matches!(by_name("USB Receiver"), Support::Receiver(_)));
        assert!(matches!(
            by_name("Some Macro Pad"),
            Support::Candidate {
                driver: Driver::Via,
                ..
            }
        ));
        assert_eq!(
            by_name("Scarlett 2i2"),
            Support::Unsupported,
            "and the one nothing drives is still on the list"
        );
    }

    /// The order the OS hands collections over is not something we control,
    /// and it decides nothing here.
    #[test]
    fn the_answer_does_not_depend_on_the_order_the_os_reported_things() {
        let forwards = survey_of(&a_realistic_desk());
        let mut shuffled = a_realistic_desk();
        shuffled.reverse();
        assert_eq!(survey_of(&shuffled), forwards);
    }
}
