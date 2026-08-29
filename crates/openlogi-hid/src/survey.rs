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
    let collections = enumerate_devices()
        .await?
        .into_iter()
        .map(|device| {
            let identity = Identity {
                vendor_id: device.vendor_id,
                product_id: device.product_id,
                product: non_empty(&device.name),
                manufacturer: device.manufacturer.as_deref().and_then(non_empty),
                serial_number: device.serial_number.as_deref().and_then(non_empty),
            };
            Peripheral::from_hid(identity, device.usage_page, device.usage_id)
        })
        .collect();
    Ok(merge(collections))
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

    use super::{merge, non_empty};

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

    #[test]
    fn an_empty_reported_string_is_treated_as_no_string() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("   "), None);
        assert_eq!(non_empty(" MX Master 3S "), Some("MX Master 3S".to_owned()));
    }
}
