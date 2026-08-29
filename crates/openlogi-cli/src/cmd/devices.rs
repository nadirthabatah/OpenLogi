//! `openlogi devices` — everything plugged in, whoever made it.
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
use openlogi_catalog::{Identity, Peripheral, Support};
use openlogi_device_registry::receiver::ReceiverBrand;

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
}

/// Survey every peripheral attached and say what can be done with each.
///
/// # Errors
///
/// Fails when the platform's HID stack cannot be enumerated. A camera layer
/// that reports nothing is not an error — plenty of machines have no camera.
pub async fn run(args: DevicesArgs) -> Result<ExitCode> {
    let mut found = openlogi_hid::survey::hid_peripherals()
        .await
        .context("failed to enumerate HID devices")?;
    found.extend(
        openlogi_camera::enumerate_all_cameras()
            .into_iter()
            .map(camera_peripheral),
    );

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
fn nothing_found() -> String {
    "Nothing found.\n\n\
     No HID device and no camera was reported. On Linux that is usually a \
     permissions problem rather than an empty desk — see the udev rules in the \
     README — and on macOS it can be a missing Input Monitoring grant.\n"
        .to_owned()
}

/// Turn an enumerated camera into a catalog entry.
fn camera_peripheral(camera: openlogi_camera::Camera) -> Peripheral {
    Peripheral::from_camera(Identity {
        vendor_id: camera.vendor_id,
        product_id: camera.product_id,
        product: Some(camera.name),
        manufacturer: None,
        serial_number: camera.serial_number,
    })
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
            "{count} of {total} attached device(s) can be configured by this build. \
             Run without --supported to see the rest."
        );
    } else {
        let _ = writeln!(
            out,
            "{count} of {total} attached device(s) can be configured by this build."
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
        let _ = writeln!(
            out,
            "    the devices paired to it are listed in their own right"
        );
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
    use openlogi_catalog::{Driver, Identity, Peripheral, Support};
    use openlogi_device_registry::receiver::ReceiverBrand;

    use super::{nothing_found, report};

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
        assert!(text.contains("command: openlogi list"), "{text}");
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
            text.contains("1 of 2 attached device(s)"),
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
        assert!(text.contains("paired to it"), "{text}");
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
        assert!(text.contains("0 of 1 attached device(s)"), "{text}");
    }

    /// "Nothing found" on Linux almost always means permissions. Saying only
    /// "nothing found" sends someone to check their cables.
    #[test]
    fn an_empty_scan_names_the_likely_cause() {
        let text = nothing_found();
        assert!(text.contains("permissions"), "{text}");
        assert!(text.contains("udev"), "{text}");
        assert!(text.contains("Input Monitoring"), "{text}");
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
                needs: "a VIA protocol check, which `openlogi via list` performs",
            },
        );
        let text = report(&[candidate], false);
        assert!(text.contains("Probably configurable"), "{text}");
        assert!(text.contains("openlogi via list"), "{text}");
        assert!(
            !text.contains("Detected, not configurable"),
            "a candidate is not a refusal: {text}"
        );
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
