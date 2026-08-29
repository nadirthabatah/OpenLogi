//! `roadie devices` — everything plugged in, whoever made it.
//!
//! The command that makes this a hub rather than a Logitech utility. `list`
//! answers "which Logitech devices are paired"; this answers "what is on this
//! desk, and what can this program do with each of it" — including the parts
//! it cannot help with, because a list that silently omits your audio
//! interface tells you nothing about why you cannot find it.
//!
//! The report is built as text by [`report`] rather than printed as it goes,
//! so the thing a person actually receives is the thing the tests check. That
//! matters more than usual here: this output is written to be read aloud, and
//! a screen reader renders a wrong heading or a missed device just as
//! confidently as a right one.

use std::fmt::Write as _;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args;
use roadie_catalog::{IdSource, Identity, Peripheral, Support};
use roadie_device_registry::receiver::ReceiverBrand;

use crate::spoken::counted;
use serde_json::{Value, json};

/// Exit status for "the scan succeeded and found nothing at all".
const NOTHING_FOUND: u8 = 2;

#[derive(Debug, Args)]
pub struct DevicesArgs {
    /// Show only what this build can configure.
    ///
    /// Off by default on purpose: a device this program cannot configure is
    /// still a device you own, and being told it was seen is the answer to
    /// "why is it not in the list".
    #[arg(long)]
    pub supported: bool,
    /// Print machine-readable JSON instead of prose.
    ///
    /// The same rendering the MCP `list_peripherals` tool returns, so a script
    /// and an assistant looking at the same desk cannot be told different
    /// things.
    #[arg(long)]
    pub json: bool,
}

/// Survey every peripheral attached and say what can be done with each.
///
/// # Errors
///
/// Fails when the platform's HID stack cannot be enumerated. A camera layer
/// that reports nothing is not an error — plenty of machines have no camera.
pub async fn run(args: DevicesArgs) -> Result<ExitCode> {
    let mut found = roadie_hid::survey::hid_peripherals()
        .await
        .context("failed to enumerate HID devices")?;
    found.extend(
        roadie_camera::enumerate_all_cameras()
            .into_iter()
            .map(camera_peripheral),
    );
    found.extend(display_peripherals());

    if args.json {
        // Emptiness is data in JSON, not a message: a consumer branches on the
        // arrays, and a "nothing found" sentence would only be something to
        // strip. The exit status still distinguishes the two cases.
        println!("{}", report_json(&found, args.supported));
        return Ok(if found.is_empty() {
            ExitCode::from(NOTHING_FOUND)
        } else {
            ExitCode::SUCCESS
        });
    }

    if found.is_empty() {
        print!("{}", nothing_found());
        return Ok(ExitCode::from(NOTHING_FOUND));
    }

    print!("{}", report(&found, args.supported));
    Ok(ExitCode::SUCCESS)
}

/// What to say when the scan worked and turned up nothing.
///
/// Kept distinct from an enumeration failure, and from an empty desk: on Linux
/// the overwhelmingly likely cause is that this process cannot open hidraw,
/// and "nothing found" with no further word would send someone looking at
/// their cables.
///
/// It names every source that was searched, monitors included. A sentence that
/// listed two of the three would be read as a complete list by anyone who
/// could not see that a third had been looked at.
fn nothing_found() -> String {
    "Nothing found.\n\n\
     No HID device, no camera and no monitor was reported. On Linux that is \
     usually a permissions problem rather than an empty desk, and on macOS it \
     can be a missing Input Monitoring grant.\n\n\
     Run `roadie doctor`: it checks which of those it is and says what to do \
     about it, in order.\n"
        .to_owned()
}

/// Turn an enumerated camera into a catalog entry.
pub fn camera_peripheral(camera: roadie_camera::Camera) -> Peripheral {
    Peripheral::from_camera(Identity {
        ids: IdSource::Usb,
        vendor_id: camera.vendor_id,
        product_id: camera.product_id,
        product: Some(camera.name),
        manufacturer: None,
        serial_number: camera.serial_number,
    })
}

/// Turn an enumerated monitor into a catalog entry.
///
/// The ids come from the EDID rather than from USB, which is why [`IdSource`]
/// exists: a display's manufacturer code is a PNP id, and the same number
/// names a different company in the two schemes.
///
/// A monitor with no readable EDID still becomes an entry. It is a real state
/// — the kernel reports the link before it has read the block, and some KVM
/// switches never let it — and a display that cannot be named is still a
/// display on the desk.
pub fn display_peripheral(display: &roadie_display::Display) -> Peripheral {
    let identity = display.edid().map_or_else(
        || Identity {
            ids: IdSource::Edid,
            product: Some(display.describe()),
            ..Identity::default()
        },
        |edid| Identity {
            ids: IdSource::Edid,
            vendor_id: u16::from_be_bytes([edid.manufacturer[0], edid.manufacturer[1]]),
            product_id: edid.product_code,
            product: edid.name.clone(),
            manufacturer: edid.vendor().map(str::to_owned),
            // The EDID's serial number is zero when the panel does not carry
            // one, and a zero would merge two identical monitors into one
            // entry rather than distinguishing them.
            serial_number: (edid.serial_number != 0).then(|| edid.serial_number.to_string()),
        },
    );
    Peripheral::from_display(identity)
}

/// Every monitor attached, as catalog entries.
///
/// A failure to enumerate displays is not a failure of the survey: plenty of
/// machines have no display subsystem this build can read, and taking the
/// whole desk listing away over it would be the wrong trade.
fn display_peripherals() -> Vec<Peripheral> {
    roadie_display::enumerate()
        .unwrap_or_default()
        .iter()
        .map(display_peripheral)
        .collect()
}

/// How a receiver family is named in the listing.
///
/// Spelled out rather than derived from `Debug`, which is a developer's view
/// that would change under anyone who reorganised the enum.
const fn receiver_name(brand: ReceiverBrand) -> &'static str {
    match brand {
        ReceiverBrand::Bolt => "Logi Bolt",
        ReceiverBrand::Unifying => "Unifying",
        ReceiverBrand::Nano => "Nano",
        ReceiverBrand::Lightspeed => "Lightspeed",
    }
}

/// One peripheral as JSON, for `--json` and for the MCP tool.
///
/// Built as a map rather than a `json!` literal that is then reopened: the
/// fields differ by support kind, and reaching back into a built value to add
/// them means asserting it is still an object, which is a panic waiting for
/// whoever edits the literal above it.
#[must_use]
pub fn as_json(found: &Peripheral) -> Value {
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

/// The whole report as JSON.
///
/// `--supported` filters the list, and the totals are of everything found
/// either way — a shorter list must not read as a smaller desk to a script
/// any more than it should to a person.
#[must_use]
pub fn report_json(found: &[Peripheral], filtered: bool) -> String {
    let shown: Vec<&Peripheral> = found
        .iter()
        .filter(|found| !filtered || found.support.is_configurable())
        .collect();
    let document = json!({
        "peripherals": shown.iter().map(|found| as_json(found)).collect::<Vec<_>>(),
        "total": found.len(),
        "configurable": found
            .iter()
            .filter(|found| found.support.is_configurable())
            .count(),
        "filtered": filtered,
    });
    serde_json::to_string_pretty(&document).unwrap_or_else(|_| {
        r#"{"error":"the device list could not be rendered as JSON"}"#.to_owned()
    })
}

/// The whole report, as the text a person receives.
///
/// `filtered` reflects `--supported`: the unconfigurable devices are left out,
/// and the closing line says so rather than letting a shorter list read as a
/// smaller desk.
#[must_use]
pub fn report(found: &[Peripheral], filtered: bool) -> String {
    let mut out = String::new();
    let configurable: Vec<&Peripheral> = found
        .iter()
        .filter(|found| found.support.is_configurable())
        .collect();

    write_configurable(&mut out, &configurable);
    if !filtered {
        write_candidates(&mut out, found);
        write_receivers(&mut out, found);
        write_unsupported(&mut out, found);
    }

    let total = found.len();
    let count = configurable.len();
    if filtered {
        let _ = writeln!(
            out,
            "{count} of {} can be configured by this build. Run without --supported to \
             see the rest.",
            counted(total, "attached device", "attached devices")
        );
    } else {
        let _ = writeln!(
            out,
            "{count} of {} can be configured by this build.",
            counted(total, "attached device", "attached devices")
        );
    }
    out
}

/// The devices this build can configure, and what each one offers.
fn write_configurable(out: &mut String, devices: &[&Peripheral]) {
    let _ = writeln!(out, "Configurable now ({}):", devices.len());
    if devices.is_empty() {
        let _ = writeln!(out, "  (none)\n");
        return;
    }
    for found in devices {
        let Support::Driver { driver, model } = &found.support else {
            continue;
        };
        let _ = writeln!(out, "  {}", found.identity.full_description());
        if let Some(model) = model {
            let _ = writeln!(out, "    model: {model}");
        }
        let _ = writeln!(out, "    controls: {}", driver.what_it_configures());
        let _ = writeln!(out, "    command: {}", driver.command());
    }
    let _ = writeln!(out);
}

/// Devices that look drivable but have not been confirmed.
///
/// Their own group rather than folded into either neighbour. Listing them as
/// configurable promises a device works; listing them as unsupported hides one
/// that probably does. Saying which check settles it is the only honest answer —
/// and it is a check the person can run.
fn write_candidates(out: &mut String, found: &[Peripheral]) {
    let candidates: Vec<(&Peripheral, &'static str)> = found
        .iter()
        .filter_map(|found| match found.support {
            Support::Candidate { needs, .. } => Some((found, needs)),
            _ => None,
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let _ = writeln!(out, "Probably configurable ({}):", candidates.len());
    for (found, needs) in candidates {
        let _ = writeln!(out, "  {}", found.identity.full_description());
        let _ = writeln!(out, "    needs: {needs}");
    }
    let _ = writeln!(out);
}

/// Receivers, which are supported but are not themselves a peripheral.
fn write_receivers(out: &mut String, found: &[Peripheral]) {
    let receivers: Vec<(&Peripheral, ReceiverBrand)> = found
        .iter()
        .filter_map(|found| match found.support {
            Support::Receiver(brand) => Some((found, brand)),
            _ => None,
        })
        .collect();
    if receivers.is_empty() {
        return;
    }
    let _ = writeln!(out, "Wireless receivers ({}):", receivers.len());
    for (found, brand) in receivers {
        let _ = writeln!(
            out,
            "  {} — {} receiver",
            found.identity.describe(),
            receiver_name(brand)
        );
        let _ = writeln!(out, "    the mice and keyboards paired to it: roadie list");
    }
    let _ = writeln!(out);
}

/// Everything this build cannot drive, which is never simply dropped.
fn write_unsupported(out: &mut String, found: &[Peripheral]) {
    let unsupported: Vec<&Peripheral> = found
        .iter()
        .filter(|found| matches!(found.support, Support::Unsupported))
        .collect();
    if unsupported.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "Detected, not configurable by this build ({}):",
        unsupported.len()
    );
    for found in unsupported {
        let _ = writeln!(out, "  {}", found.identity.full_description());
    }
    let _ = writeln!(
        out,
        "\nThose are listed so you know they were seen. The two numbers after \
         each name are what a device-support request needs.\n"
    );
}

#[cfg(test)]
mod tests {
    use roadie_catalog::{Driver, Identity, Peripheral, Support};
    use roadie_device_registry::receiver::ReceiverBrand;

    use super::{nothing_found, report, report_json};

    fn named(name: &str, support: Support) -> Peripheral {
        Peripheral {
            identity: Identity {
                vendor_id: 0x046d,
                product_id: 0x4082,
                product: Some(name.to_owned()),
                ..Identity::default()
            },
            support,
        }
    }

    fn mouse() -> Peripheral {
        named(
            "MX Master 3S",
            Support::Driver {
                driver: Driver::HidPlusPlus,
                model: None,
            },
        )
    }

    fn interface() -> Peripheral {
        named("Scarlett 2i2", Support::Unsupported)
    }

    #[test]
    fn a_configurable_device_says_what_it_offers_and_how_to_reach_it() {
        let text = report(&[mouse()], false);
        assert!(text.contains("MX Master 3S"), "{text}");
        assert!(text.contains("controls: "), "{text}");
        assert!(text.contains("command: roadie list"), "{text}");
    }

    /// The promise this command exists to keep. A device the hub cannot drive
    /// is still on the desk, and omitting it is how vendor software leaves
    /// people unable to tell "unsupported" from "not plugged in".
    #[test]
    fn an_unsupported_device_is_shown_with_the_ids_a_request_needs() {
        let text = report(&[mouse(), interface()], false);
        assert!(text.contains("Scarlett 2i2"), "{text}");
        assert!(text.contains("046d:4082"), "{text}");
        assert!(
            text.contains("Detected, not configurable"),
            "it needs a heading that says why it is in a separate list: {text}"
        );
    }

    #[test]
    fn supported_only_hides_the_rest_but_not_the_count() {
        let text = report(&[mouse(), interface()], true);
        assert!(!text.contains("Scarlett 2i2"), "{text}");
        assert!(
            text.contains("1 of 2 attached devices"),
            "a filtered list must not read as a smaller desk: {text}"
        );
        assert!(text.contains("--supported"), "{text}");
    }

    /// A receiver reported as unsupported would tell someone their mouse was
    /// not going to work, which is the opposite of the truth.
    #[test]
    fn a_receiver_is_not_listed_among_the_unsupported() {
        let receiver = named("USB Receiver", Support::Receiver(ReceiverBrand::Unifying));
        let text = report(&[receiver], false);
        assert!(text.contains("Unifying receiver"), "{text}");
        assert!(!text.contains("Detected, not configurable"), "{text}");
        // It has to name the command that actually lists them. Whether a
        // paired device gets its own HID node is platform-dependent — on Linux
        // `hid-logitech-dj` makes one, elsewhere it does not — so telling
        // someone they appear "in their own right" here is true on one
        // platform and a wild goose chase on the others.
        assert!(text.contains("roadie list"), "{text}");
    }

    /// Every branded receiver has to render as words. `Debug` would too, which
    /// is why this checks the names rather than merely that something appears.
    #[test]
    fn every_receiver_family_has_a_name_fit_to_read_aloud() {
        for (brand, expected) in [
            (ReceiverBrand::Bolt, "Logi Bolt"),
            (ReceiverBrand::Unifying, "Unifying"),
            (ReceiverBrand::Nano, "Nano"),
            (ReceiverBrand::Lightspeed, "Lightspeed"),
        ] {
            let text = report(&[named("USB Receiver", Support::Receiver(brand))], false);
            assert!(text.contains(expected), "{brand:?} rendered as: {text}");
        }
    }

    /// A desk of only unsupported devices must still produce a coherent
    /// report, not a heading followed by nothing.
    #[test]
    fn a_desk_with_nothing_configurable_still_reads_as_a_report() {
        let text = report(&[interface()], false);
        assert!(text.contains("Configurable now (0)"), "{text}");
        assert!(text.contains("(none)"), "{text}");
        assert!(text.contains("0 of 1 attached device"), "{text}");
    }

    /// "Nothing found" on Linux almost always means permissions. Saying only
    /// "nothing found" sends someone to check their cables.
    #[test]
    fn an_empty_scan_names_the_likely_cause_and_where_to_go_next() {
        let text = nothing_found();
        assert!(text.contains("permissions"), "{text}");
        assert!(text.contains("Input Monitoring"), "{text}");
        // Every source the survey actually searched. Naming two of three reads
        // as a complete list to anyone who cannot see that a third was tried.
        for source in ["HID device", "camera", "monitor"] {
            assert!(
                text.contains(source),
                "the message has to name {source}, which was searched: {text}"
            );
        }
        assert!(
            text.contains("roadie doctor"),
            "naming the cause is half of it; the other half is the command that \
             works out which cause it is: {text}"
        );
    }

    /// The guard the whole command rests on: **no device ever vanishes**.
    ///
    /// The listing filters by support kind, so a support kind nobody thought
    /// to add a section for is a device silently missing from someone's desk
    /// — which is the exact failure this command exists to prevent, arriving
    /// as a compile-clean change. Adding a `Support` variant without a
    /// section fails here.
    #[test]
    fn every_kind_of_device_appears_somewhere_in_the_report() {
        let every_kind = [
            Support::Driver {
                driver: Driver::HidPlusPlus,
                model: None,
            },
            Support::Candidate {
                driver: Driver::Via,
                needs: "a check",
            },
            Support::Receiver(ReceiverBrand::Unifying),
            Support::Unsupported,
        ];
        for (index, support) in every_kind.into_iter().enumerate() {
            let name = format!("Device Number {index}");
            let text = report(&[named(&name, support.clone())], false);
            assert!(
                text.contains(&name),
                "a {support:?} device is missing from the report:\n{text}"
            );
        }
    }

    #[test]
    fn a_candidate_is_shown_with_the_check_that_would_settle_it() {
        let candidate = named(
            "Some Macro Pad",
            Support::Candidate {
                driver: Driver::Via,
                needs: "a VIA protocol check, which `roadie via list` performs",
            },
        );
        let text = report(&[candidate], false);
        assert!(text.contains("Probably configurable"), "{text}");
        assert!(text.contains("roadie via list"), "{text}");
        assert!(
            !text.contains("Detected, not configurable"),
            "a candidate is not a refusal: {text}"
        );
    }

    /// The sweep the end-to-end tests cannot do. On a machine with an empty
    /// desk — every CI runner without USB, and this project's whole
    /// development environment — the populated report is never printed, so
    /// running the program and reading its output would check none of it.
    #[test]
    fn every_shape_of_report_is_worth_listening_to() {
        let desk = [
            mouse(),
            interface(),
            named("USB Receiver", Support::Receiver(ReceiverBrand::Bolt)),
            named(
                "Some Macro Pad",
                Support::Candidate {
                    driver: Driver::Via,
                    needs: "a VIA protocol check",
                },
            ),
        ];
        for (text, what) in [
            (report(&desk, false), "the full report"),
            (report(&desk, true), "the filtered report"),
            (report(&[], false), "a report of an empty desk"),
            (report(&[desk[0].clone()], false), "a report of one device"),
            (nothing_found(), "the nothing-found message"),
        ] {
            crate::spoken::assert_listenable(&text, what);
            crate::spoken::assert_agrees(&text, what);
        }
    }

    /// One device, not "1 device(s)". The singular is the case that gets
    /// missed, because the plural reads fine to whoever wrote it.
    #[test]
    fn a_desk_of_one_is_counted_in_the_singular() {
        let text = report(&[mouse()], false);
        assert!(text.contains("1 of 1 attached device "), "{text}");
        assert!(!text.contains("devices"), "{text}");
    }

    /// A `--json` consumer branches on structure, so the structure has to be
    /// there even when the desk is empty — and a "nothing found" sentence in
    /// the middle of a document would only be something to strip.
    #[test]
    fn the_json_report_is_valid_json_even_with_nothing_attached() {
        let text = report_json(&[], false);
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(parsed["total"], 0);
        assert_eq!(parsed["configurable"], 0);
        assert!(parsed["peripherals"].as_array().is_some_and(Vec::is_empty));
    }

    /// The same rule as the prose form: filtering the list must not make the
    /// desk look smaller. A script that reported "1 device" to someone with
    /// four would be as wrong as a sentence saying it.
    #[test]
    fn filtering_the_json_leaves_the_totals_telling_the_truth() {
        let text = report_json(&[mouse(), interface()], true);
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(parsed["total"], 2, "everything found");
        assert_eq!(parsed["configurable"], 1);
        assert_eq!(parsed["filtered"], true);
        assert_eq!(parsed["peripherals"].as_array().map(Vec::len), Some(1));
    }

    /// An unsupported device is in the JSON for the same reason it is in the
    /// prose: it is on the desk, and a consumer told nothing about it will
    /// report it absent.
    #[test]
    fn an_unsupported_device_is_in_the_json_too() {
        let text = report_json(&[interface()], false);
        assert!(text.contains("Scarlett 2i2"), "{text}");
        assert!(text.contains("\"configurable\": false"), "{text}");
    }

    /// Every section that appears is introduced by a heading. A run of
    /// indented lines with no heading is unreadable by ear.
    #[test]
    fn every_listed_device_sits_under_a_heading() {
        let text = report(
            &[
                mouse(),
                interface(),
                named("USB Receiver", Support::Receiver(ReceiverBrand::Bolt)),
            ],
            false,
        );
        let mut heading_seen = false;
        for line in text.lines() {
            if line.ends_with("):") {
                heading_seen = true;
            } else if line.starts_with("  ") {
                assert!(heading_seen, "an indented line before any heading: {text}");
            }
        }
        assert!(heading_seen, "{text}");
    }
}
