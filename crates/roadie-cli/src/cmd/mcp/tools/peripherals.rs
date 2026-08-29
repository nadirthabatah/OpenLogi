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

use roadie_catalog::Peripheral;
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
    let mut found = roadie_hid::survey::hid_peripherals()
        .await
        .map_err(|error| format!("failed to enumerate HID devices: {error}"))?;
    // The same two helpers `roadie devices` uses, so a script and an assistant
    // reading the same desk cannot be told different things about it.
    found.extend(
        roadie_camera::enumerate_all_cameras()
            .into_iter()
            .map(crate::cmd::devices::camera_peripheral),
    );
    found.extend(
        roadie_display::enumerate()
            .unwrap_or_default()
            .iter()
            .map(crate::cmd::devices::display_peripheral),
    );
    // Two seconds of multicast, the same as `roadie devices`. The cost is
    // real, and paying it here rather than leaving the network out is what
    // keeps the promise above true: a script and an assistant reading the same
    // desk must not be told different things about it.
    found.extend(
        roadie_keylight::discover(std::time::Duration::from_secs(2))
            .unwrap_or_default()
            .iter()
            .map(|light| {
                crate::cmd::devices::key_light_peripheral(light.name(), light.info().ok().as_ref())
            }),
    );

    // The same rendering `roadie devices --json` prints, so a script and an
    // assistant reading the same desk cannot be told different things.
    rendered(&summary(&found))
}

/// The answer, as the model receives it.
///
/// Split from the enumeration above so the two counts can be checked. They are
/// what an assistant repeats out loud — "three of seven can be configured" —
/// and a count that quietly includes the unconfigurable ones is a sentence
/// nobody can tell is wrong from the outside.
fn summary(found: &[Peripheral]) -> Value {
    let configurable = found
        .iter()
        .filter(|found| found.support.is_configurable())
        .count();
    json!({
        "peripherals": found.iter().map(crate::cmd::devices::as_json).collect::<Vec<_>>(),
        "total": found.len(),
        "configurable": configurable,
    })
}

#[cfg(test)]
mod tests {
    use roadie_catalog::{Driver, Identity, Peripheral, Support};
    use roadie_device_registry::receiver::ReceiverBrand;

    use crate::cmd::devices::as_json as describe;

    use super::{summary, tools};

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

    /// The two numbers an assistant repeats out loud.
    ///
    /// "Three of seven can be configured" is a sentence nobody can tell is
    /// wrong from the outside, so the count has to be right at the source. A
    /// mutation sweep found it counting everything and no test noticed.
    #[test]
    fn the_counts_separate_what_can_be_configured_from_what_cannot() {
        let desk = [
            peripheral(Support::Driver {
                driver: Driver::HidPlusPlus,
                model: None,
            }),
            peripheral(Support::Unsupported),
            peripheral(Support::Receiver(ReceiverBrand::Unifying)),
            peripheral(Support::Candidate {
                driver: Driver::Via,
                needs: "a check",
            }),
        ];
        let answer = summary(&desk);
        assert_eq!(answer["total"], 4);
        assert_eq!(
            answer["configurable"], 1,
            "only the driver-backed device is configurable: {answer}"
        );
        // Every device is still listed, whatever the count says.
        assert_eq!(
            answer["peripherals"].as_array().expect("a list").len(),
            4,
            "a device must never be dropped from the listing: {answer}"
        );
    }

    #[test]
    fn a_configurable_device_carries_its_driver_and_command() {
        let entry = describe(&peripheral(Support::Driver {
            driver: Driver::HidPlusPlus,
            model: None,
        }));
        assert_eq!(entry["configurable"], true);
        assert_eq!(entry["driver"], "hidpp");
        assert_eq!(entry["command"], "roadie list");
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
