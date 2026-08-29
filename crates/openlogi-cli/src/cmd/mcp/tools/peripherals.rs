//! Everything plugged in, over MCP.
//!
//! The question an assistant is actually asked is "what do I have plugged in?"
//! — not "which Logitech devices are paired", which is what `list_devices`
//! answers. Answering the real question means one tool that spans vendors, so
//! the model does not have to guess which brand-specific tool to reach for,
//! and does not have to call four of them to find out a device is not there.
//!
//! Like the camera and Stream Deck tools, this reads hardware directly rather
//! than through the agent: the survey is HID enumeration plus the camera
//! layer, neither of which the agent owns, and routing it through one would
//! mean inventing a wire contract for something already available here.
//!
//! Devices this build cannot configure are included, and marked. Omitting them
//! would leave the model confidently telling someone their audio interface is
//! not connected when it is plugged in and lit up.

use openlogi_catalog::{Identity, Peripheral, Support};
use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![json!({
        "name": "list_peripherals",
        "description": "Every peripheral attached to this computer, whoever made it, \
            and what this build can configure on each. Use this to answer \"what do I \
            have plugged in\" — it spans vendors, unlike list_devices, which covers \
            only Logitech HID++ devices. Devices with no driver here are included and \
            marked `configurable: false`; report them as present but not configurable \
            rather than as absent.",
        "inputSchema": no_arguments_schema(),
    })]
}

/// Run `list_peripherals`.
pub async fn list_peripherals() -> Result<String, String> {
    let mut found = openlogi_hid::survey::hid_peripherals()
        .await
        .map_err(|error| format!("failed to enumerate HID devices: {error}"))?;
    found.extend(
        openlogi_camera::enumerate_all_cameras()
            .into_iter()
            .map(|camera| {
                Peripheral::from_camera(Identity {
                    vendor_id: camera.vendor_id,
                    product_id: camera.product_id,
                    product: Some(camera.name),
                    manufacturer: None,
                    serial_number: camera.serial_number,
                })
            }),
    );

    let configurable = found
        .iter()
        .filter(|found| found.support.is_configurable())
        .count();
    let peripherals: Vec<Value> = found.iter().map(describe).collect();
    rendered(&json!({
        "peripherals": peripherals,
        "total": found.len(),
        "configurable": configurable,
    }))
}

/// One peripheral, as the model sees it.
///
/// Built as a map rather than a `json!` literal that is then reopened: the
/// fields differ by support kind, and reaching back into a built value to add
/// them means asserting it is still an object, which is a panic waiting for
/// whoever edits the literal above it.
fn describe(found: &Peripheral) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("name".to_owned(), json!(found.identity.describe()));
    entry.insert(
        "vendor_id".to_owned(),
        json!(format!("{:04x}", found.identity.vendor_id)),
    );
    entry.insert(
        "product_id".to_owned(),
        json!(format!("{:04x}", found.identity.product_id)),
    );
    entry.insert(
        "configurable".to_owned(),
        json!(found.support.is_configurable()),
    );
    if let Some(serial) = &found.identity.serial_number {
        entry.insert("serial".to_owned(), json!(serial));
    }
    match &found.support {
        Support::Driver { driver, model } => {
            entry.insert("driver".to_owned(), json!(driver.id()));
            entry.insert("controls".to_owned(), json!(driver.what_it_configures()));
            entry.insert("command".to_owned(), json!(driver.command()));
            if let Some(model) = model {
                entry.insert("model".to_owned(), json!(model));
            }
        }
        Support::Candidate { driver, needs } => {
            entry.insert("likely_driver".to_owned(), json!(driver.id()));
            entry.insert(
                "note".to_owned(),
                json!(format!(
                    "Attached, and it looks like something the {} driver handles, but \
                     that is not confirmed without {needs}. Say it is probably \
                     supported rather than that it is.",
                    driver.id()
                )),
            );
        }
        Support::Receiver(_) => {
            entry.insert("kind".to_owned(), json!("wireless receiver"));
            entry.insert(
                "note".to_owned(),
                json!(
                    "A receiver, not a peripheral. The devices paired to it appear \
                     separately; use list_devices for those."
                ),
            );
        }
        Support::Unsupported => {
            entry.insert(
                "note".to_owned(),
                json!(
                    "Attached, but no driver in this build configures it. It is \
                     present — say so — and the vendor and product ids above are \
                     what a device-support request needs."
                ),
            );
        }
    }
    Value::Object(entry)
}

#[cfg(test)]
mod tests {
    use openlogi_catalog::{Driver, Identity, Peripheral, Support};
    use openlogi_device_registry::receiver::ReceiverBrand;

    use super::{describe, tools};

    fn peripheral(support: Support) -> Peripheral {
        Peripheral {
            identity: Identity {
                vendor_id: 0x046d,
                product_id: 0x4082,
                product: Some("MX Master 3S".to_owned()),
                ..Identity::default()
            },
            support,
        }
    }

    #[test]
    fn a_configurable_device_carries_its_driver_and_command() {
        let entry = describe(&peripheral(Support::Driver {
            driver: Driver::HidPlusPlus,
            model: None,
        }));
        assert_eq!(entry["configurable"], true);
        assert_eq!(entry["driver"], "hidpp");
        assert_eq!(entry["command"], "openlogi list");
        assert_eq!(entry["name"], "MX Master 3S");
    }

    /// Ids go out as the four hex digits every USB reference prints, not as
    /// the decimal a JSON number would give. A model relaying "1133:16514" to
    /// someone filing a support request has relayed the wrong thing.
    #[test]
    fn ids_are_rendered_as_hex() {
        let entry = describe(&peripheral(Support::Unsupported));
        assert_eq!(entry["vendor_id"], "046d");
        assert_eq!(entry["product_id"], "4082");
    }

    /// The failure this tool exists to prevent: a model told nothing about an
    /// unsupported device will say it is not connected.
    #[test]
    fn an_unsupported_device_is_told_to_be_reported_as_present() {
        let entry = describe(&peripheral(Support::Unsupported));
        assert_eq!(entry["configurable"], false);
        let note = entry["note"].as_str().expect("a note");
        assert!(note.contains("present"), "{note}");
    }

    /// A receiver marked simply "not configurable" would have the model tell
    /// someone their mouse is unsupported.
    #[test]
    fn a_receiver_is_explained_rather_than_left_looking_unsupported() {
        let entry = describe(&peripheral(Support::Receiver(ReceiverBrand::Unifying)));
        assert_eq!(entry["kind"], "wireless receiver");
        let note = entry["note"].as_str().expect("a note");
        assert!(note.contains("list_devices"), "{note}");
    }

    #[test]
    fn a_serial_is_included_only_when_the_device_reports_one() {
        let without = describe(&peripheral(Support::Unsupported));
        assert!(without.get("serial").is_none());

        let mut found = peripheral(Support::Unsupported);
        found.identity.serial_number = Some("AL1".to_owned());
        assert_eq!(describe(&found)["serial"], "AL1");
    }

    /// The description has to tell the model when *not* to reach for this
    /// tool's neighbour, or it will call the Logitech-only one and report an
    /// empty desk.
    #[test]
    fn the_description_distinguishes_this_tool_from_list_devices() {
        let catalog = tools();
        let description = catalog[0]["description"].as_str().expect("a description");
        assert!(description.contains("list_devices"), "{description}");
        assert!(description.contains("plugged in"), "{description}");
    }
}
