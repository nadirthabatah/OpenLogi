//! `openlogi doctor` — why nothing is working, and what to do about it.
//!
//! Every other command in this program assumes it can reach your hardware.
//! When it cannot, what it can honestly say is "nothing found", which is the
//! same sentence whether your desk is empty, a cable is out, or this process
//! is not allowed to open the devices sitting right there. Those have entirely
//! different answers and the first one is almost never the real cause.
//!
//! So: one command that checks each thing in turn and, for anything wrong,
//! says what to do about it in words you can act on. It is written to be read
//! aloud — a numbered list of steps, not a table of green and red dots — which
//! is the form this information has to take for it to be usable at all without
//! sight, and is a better form for everyone.
//!
//! The gathering touches the machine; the reasoning does not. [`diagnose`]
//! turns facts into findings and is where every judgement lives, so the advice
//! this command gives is checked rather than only ever seen on the one machine
//! it was written on.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;

/// Exit status for "something is wrong that will stop this program working".
///
/// Distinct from a failure of the command itself: `doctor` succeeded at
/// checking, and found a problem. A script that could not tell those apart
/// would report a broken tool when the tool is working perfectly and the
/// permissions are not.
const PROBLEMS_FOUND: u8 = 2;

#[derive(Debug, Args)]
pub struct DoctorArgs {}

/// What the machine actually looks like right now.
///
/// A plain record, so [`diagnose`] can be given a machine that does not exist
/// — the machine with no permissions, the machine with an empty desk — and
/// checked against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The operating system, as `cfg` reports it.
    pub platform: Platform,
    /// HID devices the survey could see.
    pub hid_devices: usize,
    /// Cameras the OS reported.
    pub cameras: usize,
    /// Raw HID nodes present, and how many this process could open.
    ///
    /// `None` off Linux, where there is no equivalent to count.
    pub hidraw: Option<Hidraw>,
    /// Whether a running agent answered.
    pub agent_reachable: bool,
    /// Where the configuration file would be, and whether it is there.
    pub config: Option<PathBuf>,
    /// Whether that configuration file exists.
    pub config_exists: bool,
    /// Saved Stream Deck layouts.
    pub layouts: usize,
}

/// Which operating system, for advice that differs by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux, where device access is udev rules.
    Linux,
    /// macOS, where device access is a privacy consent grant.
    MacOs,
    /// Windows, where HID devices are open to any process.
    Windows,
    /// Anything else.
    Other,
}

/// Raw HID nodes on a Linux machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hidraw {
    /// How many `/dev/hidraw*` nodes exist.
    pub present: usize,
    /// How many of them this process could open for reading.
    pub openable: usize,
}

/// How a check came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to do.
    Fine(String),
    /// Something is wrong, with the steps that fix it.
    Problem {
        /// What is wrong, in one sentence.
        detail: String,
        /// What to do, in order.
        fix: Vec<String>,
    },
    /// Could not be determined, and why that is not itself a fault.
    Undetermined(String),
}

/// One thing checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// What was checked.
    pub name: &'static str,
    /// What was found.
    pub verdict: Verdict,
}

/// Look at the machine and say what is wrong with it.
///
/// # Errors
///
/// Does not fail on a broken machine — that is the case it exists for. Fails
/// only when the survey itself cannot run.
pub async fn run(_args: DoctorArgs) -> Result<ExitCode> {
    let facts = gather().await;
    let checks = diagnose(&facts);
    let problems = checks
        .iter()
        .filter(|check| matches!(check.verdict, Verdict::Problem { .. }))
        .count();
    print!("{}", render(&checks, problems));
    Ok(if problems == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(PROBLEMS_FOUND)
    })
}

/// Read the machine.
async fn gather() -> Facts {
    // A survey that fails is itself a fact worth reporting as "saw nothing",
    // not a reason to abandon the other seven checks. Someone running `doctor`
    // has already hit a failure; a second one is not news.
    let hid_devices = openlogi_hid::survey::hid_peripherals()
        .await
        .map_or(0, |found| found.len());
    let config = openlogi_core::paths::config_path().ok();
    Facts {
        platform: platform(),
        hid_devices,
        cameras: openlogi_camera::enumerate_all_cameras().len(),
        hidraw: count_hidraw(),
        agent_reachable: agent_reachable().await,
        config_exists: config.as_ref().is_some_and(|path| path.is_file()),
        config,
        layouts: crate::cmd::streamdeck::layout_library()
            .ok()
            .map_or(0, |path| count_layouts(&path)),
    }
}

/// This build's operating system.
const fn platform() -> Platform {
    if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Other
    }
}

/// Count `/dev/hidraw*` nodes and how many open.
///
/// Vendor-neutral on purpose. The permissions crate probes Logitech nodes
/// specifically, which was right when this program was a Logitech utility and
/// is wrong now: someone whose only device is a Stream Deck would be told
/// their access is fine while nothing works.
fn count_hidraw() -> Option<Hidraw> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let entries = std::fs::read_dir("/dev").ok()?;
    let nodes: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("hidraw"))
        })
        .collect();
    let openable = nodes
        .iter()
        .filter(|path| std::fs::File::open(path).is_ok())
        .count();
    Some(Hidraw {
        present: nodes.len(),
        openable,
    })
}

/// Layout files in a directory.
fn count_layouts(directory: &std::path::Path) -> usize {
    std::fs::read_dir(directory).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "toml")
            })
            .count()
    })
}

/// Whether an agent answers.
async fn agent_reachable() -> bool {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        openlogi_ipc::client::connect(),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

/// Turn facts into findings.
///
/// Every judgement this command makes lives here, taking a plain record and
/// returning plain findings, so the advice can be checked against machines
/// this one will never be — the machine with no permissions, the empty desk,
/// the one where a Stream Deck is the only device.
#[must_use]
pub fn diagnose(facts: &Facts) -> Vec<Check> {
    vec![
        device_access(facts),
        devices_visible(facts),
        agent(facts),
        configuration(facts),
        layouts(facts),
    ]
}

/// Can this process open the devices at all?
///
/// First, because it is the cause of most of what follows: a permissions
/// problem makes an attached device invisible, and being told "no devices
/// found" first would send someone to check their cables.
fn device_access(facts: &Facts) -> Check {
    let verdict = match (facts.platform, facts.hidraw) {
        (Platform::Linux, Some(hidraw)) => linux_access(hidraw),
        (Platform::Linux, None) => Verdict::Undetermined(
            "could not read /dev to look for HID devices, which is unusual and worth \
             investigating on its own"
                .to_owned(),
        ),
        (Platform::MacOs, _) => macos_access(facts),
        (Platform::Windows, _) => Verdict::Fine(
            "Windows lets any program open HID devices; there is nothing to grant".to_owned(),
        ),
        (Platform::Other, _) => Verdict::Undetermined(
            "this platform is not one whose device permissions are known here".to_owned(),
        ),
    };
    Check {
        name: "Permission to open devices",
        verdict,
    }
}

/// The Linux reading of the hidraw counts.
fn linux_access(hidraw: Hidraw) -> Verdict {
    if hidraw.present == 0 {
        return Verdict::Undetermined(
            "no /dev/hidraw devices exist at all, so there is nothing to have permission \
             for. Either nothing is plugged in, or this is a container or virtual machine \
             with no USB passed through."
                .to_owned(),
        );
    }
    if hidraw.openable == 0 {
        return Verdict::Problem {
            detail: format!(
                "{} attached and this program cannot open any of them. \
                 This is a permissions problem, not a hardware one — the devices are \
                 there.",
                counted(hidraw.present, "HID device is", "HID devices are")
            ),
            fix: vec![
                "Install the udev rules, which give your user account access to HID \
                 devices. The project ships them; see the README for the file and where \
                 it goes."
                    .to_owned(),
                "Reload the rules without rebooting: sudo udevadm control --reload-rules \
                 && sudo udevadm trigger"
                    .to_owned(),
                "Unplug the device and plug it back in — a rule applies when a device \
                 appears, so one already attached keeps the permissions it was given."
                    .to_owned(),
                "Run openlogi doctor again to confirm.".to_owned(),
            ],
        };
    }
    if hidraw.openable < hidraw.present {
        return Verdict::Problem {
            detail: format!(
                "{} of {} attached HID devices can be opened. The rest are visible but \
                 out of reach, so some of your peripherals will work here and some will \
                 not.",
                hidraw.openable, hidraw.present
            ),
            fix: vec![
                "The udev rules are installed but do not cover every device. If a rule \
                 lists vendor ids, it needs one for the peripheral that is not working."
                    .to_owned(),
                "openlogi devices lists what was found, with the vendor and product ids \
                 a rule needs."
                    .to_owned(),
            ],
        };
    }
    Verdict::Fine(format!(
        "all {} can be opened",
        counted(
            hidraw.present,
            "attached HID device",
            "attached HID devices"
        )
    ))
}

/// The macOS reading, which is a consent grant rather than a file mode.
fn macos_access(facts: &Facts) -> Verdict {
    if facts.hid_devices > 0 {
        return Verdict::Fine(
            "HID devices are being read, so Input Monitoring is granted to this program".to_owned(),
        );
    }
    Verdict::Problem {
        detail: "no HID device could be read. On macOS that is usually Input Monitoring, \
                 which is granted per program — the app having it does not give it to this \
                 command, and vice versa."
            .to_owned(),
        fix: vec![
            "Open System Settings, then Privacy & Security, then Input Monitoring.".to_owned(),
            "Turn on the entry for this program. If it is not listed, run any openlogi \
             command that touches a device once and macOS will add it."
                .to_owned(),
            "Quit and reopen your terminal. A grant applies to a process when it starts, \
             so the session you were already in keeps the answer it had."
                .to_owned(),
            "Run openlogi doctor again to confirm.".to_owned(),
        ],
    }
}

/// Is anything actually there?
fn devices_visible(facts: &Facts) -> Check {
    let total = facts.hid_devices + facts.cameras;
    let verdict = if total == 0 {
        Verdict::Problem {
            detail: "no peripheral of any kind was found: no HID device and no camera.".to_owned(),
            fix: vec![
                "If the permission check above found a problem, fix that first — it is \
                 almost certainly the cause of this one."
                    .to_owned(),
                "Otherwise check the cable and the port, and try a different port.".to_owned(),
                "A device connected over Bluetooth has to be paired with the operating \
                 system first; this program does not pair it for you."
                    .to_owned(),
            ],
        }
    } else {
        Verdict::Fine(format!(
            "{} and {} found; openlogi devices lists them",
            counted(facts.hid_devices, "HID device", "HID devices"),
            counted(facts.cameras, "camera", "cameras")
        ))
    };
    Check {
        name: "Peripherals found",
        verdict,
    }
}

/// The agent, which is not required for the CLI but changes what takes effect.
fn agent(facts: &Facts) -> Check {
    let verdict = if facts.agent_reachable {
        Verdict::Fine("the background agent is running and answering".to_owned())
    } else {
        // Deliberately not a problem. Every command here works without it,
        // and calling a working setup broken because an optional part is not
        // running is how a diagnostic loses the trust it needs.
        Verdict::Undetermined(
            "no background agent is running. Nothing here needs one — but a configuration \
             change will not take effect in the app until the agent reloads it, and on \
             macOS the agent is what holds the permission grant that button remapping \
             uses."
                .to_owned(),
        )
    };
    Check {
        name: "Background agent",
        verdict,
    }
}

/// Where settings live.
fn configuration(facts: &Facts) -> Check {
    let verdict = match (&facts.config, facts.config_exists) {
        (Some(path), true) => Verdict::Fine(format!("{}", path.display())),
        (Some(path), false) => Verdict::Undetermined(format!(
            "no configuration file yet. It will be written to {} the first time you \
             change something. That is the normal state on a new machine.",
            path.display()
        )),
        (None, _) => Verdict::Problem {
            detail: "could not work out where configuration should live, which means \
                     nothing can be saved."
                .to_owned(),
            fix: vec![
                "This normally means HOME is unset. Check that your shell has it: echo \
                 $HOME"
                    .to_owned(),
            ],
        },
    };
    Check {
        name: "Configuration",
        verdict,
    }
}

/// Saved layouts, so a bundle's contents are never a surprise.
fn layouts(facts: &Facts) -> Check {
    let verdict = if facts.layouts == 0 {
        Verdict::Undetermined(
            "no Stream Deck layouts saved. openlogi streamdeck example <name> starts one."
                .to_owned(),
        )
    } else {
        Verdict::Fine(format!(
            "{} saved; openlogi profile export carries them",
            counted(facts.layouts, "layout", "layouts")
        ))
    };
    Check {
        name: "Saved layouts",
        verdict,
    }
}

/// A count with the right noun for it.
///
/// "1 thing(s)" is the kind of thing that gets waved through in a table and is
/// unbearable read aloud: a screen reader says "thing open paren s close
/// paren". This output exists to be listened to.
fn counted(how_many: usize, one: &str, many: &str) -> String {
    if how_many == 1 {
        format!("{how_many} {one}")
    } else {
        format!("{how_many} {many}")
    }
}

/// The report, as the text a person receives.
///
/// Problems are repeated at the end as one numbered list of steps. Someone who
/// has just heard eight checks read out should not have to scroll back through
/// them to find the two things they actually have to do.
#[must_use]
pub fn render(checks: &[Check], problems: usize) -> String {
    let mut out = String::new();
    for check in checks {
        match &check.verdict {
            Verdict::Fine(detail) => {
                let _ = writeln!(out, "OK    {}: {detail}", check.name);
            }
            Verdict::Undetermined(detail) => {
                let _ = writeln!(out, "NOTE  {}: {detail}", check.name);
            }
            Verdict::Problem { detail, .. } => {
                let _ = writeln!(out, "FIX   {}: {detail}", check.name);
            }
        }
    }
    let _ = writeln!(out);

    if problems == 0 {
        let _ = writeln!(out, "Nothing needs fixing.");
        return out;
    }

    // Spelled out rather than "1 thing(s)". This is read aloud as often as it
    // is read on screen, and a screen reader says "thing open paren s close
    // paren", which is worse than either word would have been.
    let _ = if problems == 1 {
        writeln!(out, "One thing to fix:")
    } else {
        writeln!(
            out,
            "{problems} things to fix. In order, because the first often causes the rest:"
        )
    };
    let mut step = 1;
    for check in checks {
        let Verdict::Problem { detail, fix } = &check.verdict else {
            continue;
        };
        let _ = writeln!(out);
        let _ = writeln!(out, "{}: {detail}", check.name);
        for instruction in fix {
            let _ = writeln!(out, "  {step}. {instruction}");
            step += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Check, Facts, Hidraw, Platform, Verdict, counted, diagnose, render};

    /// A machine where everything is fine, to vary one thing at a time from.
    fn healthy() -> Facts {
        Facts {
            platform: Platform::Linux,
            hid_devices: 3,
            cameras: 1,
            hidraw: Some(Hidraw {
                present: 4,
                openable: 4,
            }),
            agent_reachable: true,
            config: Some(PathBuf::from("/home/me/.config/openlogi/config.toml")),
            config_exists: true,
            layouts: 2,
        }
    }

    fn find<'a>(checks: &'a [Check], name: &str) -> &'a Verdict {
        &checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("no check named {name}"))
            .verdict
    }

    fn problems(checks: &[Check]) -> usize {
        checks
            .iter()
            .filter(|check| matches!(check.verdict, Verdict::Problem { .. }))
            .count()
    }

    #[test]
    fn a_healthy_machine_has_nothing_to_fix() {
        let checks = diagnose(&healthy());
        assert_eq!(problems(&checks), 0);
        assert!(render(&checks, 0).contains("Nothing needs fixing."));
    }

    /// The case this command exists for. Devices attached, none openable, is
    /// a permissions problem — and saying "no devices found" instead sends
    /// someone to check cables that are perfectly well plugged in.
    #[test]
    fn devices_present_but_unopenable_is_named_as_permissions() {
        let mut facts = healthy();
        facts.hidraw = Some(Hidraw {
            present: 4,
            openable: 0,
        });
        facts.hid_devices = 0;
        facts.cameras = 0;

        let checks = diagnose(&facts);
        let Verdict::Problem { detail, fix } = find(&checks, "Permission to open devices") else {
            panic!("unopenable devices must be a problem");
        };
        assert!(
            detail.contains("permissions problem, not a hardware one"),
            "{detail}"
        );
        assert!(
            fix.iter().any(|step| step.contains("udevadm")),
            "the fix has to be a command someone can run: {fix:?}"
        );
        assert!(
            fix.iter().any(|step| step.contains("plug it back in")),
            "a rule applies when a device appears, so this step is not optional: {fix:?}"
        );
    }

    /// Permission problems come before "nothing found", because the first
    /// causes the second and fixing them in the other order fixes nothing.
    #[test]
    fn permission_is_reported_before_devices_are_missing() {
        let checks = diagnose(&healthy());
        let names: Vec<&str> = checks.iter().map(|check| check.name).collect();
        let permission = names
            .iter()
            .position(|name| *name == "Permission to open devices")
            .expect("a permission check");
        let devices = names
            .iter()
            .position(|name| *name == "Peripherals found")
            .expect("a devices check");
        assert!(permission < devices, "{names:?}");
    }

    /// Some devices reachable and some not is its own answer: half a desk
    /// working looks like a flaky program rather than an incomplete rule.
    #[test]
    fn a_partly_reachable_desk_is_its_own_finding() {
        let mut facts = healthy();
        facts.hidraw = Some(Hidraw {
            present: 4,
            openable: 2,
        });
        let checks = diagnose(&facts);
        let Verdict::Problem { detail, fix } = find(&checks, "Permission to open devices") else {
            panic!("a partly reachable desk is a problem");
        };
        assert!(detail.contains("2 of 4"), "{detail}");
        assert!(
            fix.iter().any(|step| step.contains("openlogi devices")),
            "it has to say how to get the ids a rule needs: {fix:?}"
        );
    }

    /// A container or VM with no USB passed through. Not a fault, and calling
    /// it one would have someone rewriting working udev rules.
    #[test]
    fn a_machine_with_no_hid_nodes_at_all_is_not_blamed_for_permissions() {
        let mut facts = healthy();
        facts.hidraw = Some(Hidraw {
            present: 0,
            openable: 0,
        });
        let checks = diagnose(&facts);
        let Verdict::Undetermined(detail) = find(&checks, "Permission to open devices") else {
            panic!("no nodes at all is not a permissions problem");
        };
        assert!(detail.contains("container"), "{detail}");
    }

    /// On macOS the grant is per program, and the single most common wrong
    /// belief is that granting it to the app grants it to the terminal.
    #[test]
    fn macos_advice_names_input_monitoring_and_that_it_is_per_program() {
        let facts = Facts {
            platform: Platform::MacOs,
            hid_devices: 0,
            cameras: 0,
            hidraw: None,
            ..healthy()
        };
        let checks = diagnose(&facts);
        let Verdict::Problem { detail, fix } = find(&checks, "Permission to open devices") else {
            panic!("no readable HID on macOS is a problem");
        };
        assert!(detail.contains("Input Monitoring"), "{detail}");
        assert!(detail.contains("per program"), "{detail}");
        assert!(
            fix.iter().any(|step| step.contains("reopen your terminal")),
            "a grant applies at process start; without this step nothing changes: {fix:?}"
        );
    }

    #[test]
    fn windows_has_nothing_to_grant_and_says_so() {
        let facts = Facts {
            platform: Platform::Windows,
            hidraw: None,
            ..healthy()
        };
        let checks = diagnose(&facts);
        assert!(matches!(
            find(&checks, "Permission to open devices"),
            Verdict::Fine(_)
        ));
    }

    /// A diagnostic that calls a working setup broken loses the trust it needs
    /// to be useful. The agent is optional for everything the CLI does.
    #[test]
    fn a_missing_agent_is_a_note_and_never_a_problem() {
        let mut facts = healthy();
        facts.agent_reachable = false;
        let checks = diagnose(&facts);
        assert!(matches!(
            find(&checks, "Background agent"),
            Verdict::Undetermined(_)
        ));
        assert_eq!(problems(&checks), 0, "a missing agent breaks nothing");
    }

    /// The normal state on a new machine — which is exactly the machine
    /// someone runs this on — must not read as a fault.
    #[test]
    fn a_machine_with_no_configuration_yet_is_not_broken() {
        let mut facts = healthy();
        facts.config_exists = false;
        let checks = diagnose(&facts);
        let Verdict::Undetermined(detail) = find(&checks, "Configuration") else {
            panic!("no config yet is normal");
        };
        assert!(detail.contains("normal state on a new machine"), "{detail}");
        assert_eq!(problems(&checks), 0);
    }

    #[test]
    fn nowhere_to_save_settings_is_a_real_problem() {
        let mut facts = healthy();
        facts.config = None;
        facts.config_exists = false;
        let checks = diagnose(&facts);
        let Verdict::Problem { fix, .. } = find(&checks, "Configuration") else {
            panic!("nowhere to save is a problem");
        };
        assert!(fix.iter().any(|step| step.contains("HOME")), "{fix:?}");
    }

    /// Someone who has just heard several checks read out should not have to
    /// scroll back to find the two things they actually have to do.
    #[test]
    fn the_steps_are_repeated_at_the_end_as_one_numbered_list() {
        let mut facts = healthy();
        facts.hidraw = Some(Hidraw {
            present: 4,
            openable: 0,
        });
        facts.hid_devices = 0;
        facts.cameras = 0;
        facts.config = None;

        let checks = diagnose(&facts);
        let text = render(&checks, problems(&checks));
        assert!(text.contains("things to fix"), "{text}");
        for step in ["  1. ", "  2. ", "  3. "] {
            assert!(text.contains(step), "missing {step} in:\n{text}");
        }
    }

    /// Numbering runs across every problem rather than restarting per check,
    /// so "step 5" means one thing rather than several.
    #[test]
    fn the_steps_are_numbered_once_across_the_whole_list() {
        let mut facts = healthy();
        facts.hidraw = Some(Hidraw {
            present: 4,
            openable: 0,
        });
        facts.hid_devices = 0;
        facts.cameras = 0;
        let checks = diagnose(&facts);
        let text = render(&checks, problems(&checks));
        let numbers: Vec<&str> = text
            .lines()
            .filter_map(|line| {
                line.trim()
                    .split('.')
                    .next()
                    .filter(|_| line.starts_with("  "))
            })
            .collect();
        assert_eq!(
            numbers,
            vec!["1", "2", "3", "4", "5", "6", "7"],
            "numbering must run straight through: {text}"
        );
    }

    /// "1 thing(s)" is unbearable read aloud: a screen reader says "thing open
    /// paren s close paren".
    #[test]
    fn counts_use_words_rather_than_parenthesised_plurals() {
        assert_eq!(counted(1, "camera", "cameras"), "1 camera");
        assert_eq!(counted(0, "camera", "cameras"), "0 cameras");
        assert_eq!(counted(2, "camera", "cameras"), "2 cameras");

        let mut facts = healthy();
        facts.hidraw = Some(Hidraw {
            present: 4,
            openable: 0,
        });
        facts.hid_devices = 0;
        facts.cameras = 0;
        facts.config = Some(PathBuf::from("/x/config.toml"));
        let checks = diagnose(&facts);
        let text = render(&checks, problems(&checks));
        assert!(!text.contains("(s)"), "{text}");
    }

    /// Every problem has to carry at least one step. A finding that says
    /// something is wrong and nothing about what to do is a worse outcome
    /// than not checking.
    #[test]
    fn every_problem_carries_something_to_do_about_it() {
        let machines = [
            Facts {
                hidraw: Some(Hidraw {
                    present: 4,
                    openable: 0,
                }),
                hid_devices: 0,
                cameras: 0,
                ..healthy()
            },
            Facts {
                hidraw: Some(Hidraw {
                    present: 4,
                    openable: 1,
                }),
                ..healthy()
            },
            Facts {
                platform: Platform::MacOs,
                hidraw: None,
                hid_devices: 0,
                cameras: 0,
                ..healthy()
            },
            Facts {
                config: None,
                config_exists: false,
                ..healthy()
            },
        ];
        for facts in machines {
            for check in diagnose(&facts) {
                let Verdict::Problem { detail, fix } = check.verdict else {
                    continue;
                };
                assert!(!fix.is_empty(), "{} said '{detail}' and no fix", check.name);
                assert!(
                    fix.iter().all(|step| !step.trim().is_empty()),
                    "{} has an empty step",
                    check.name
                );
            }
        }
    }
}
