//! `roadie tourbox` — find TourBox controllers and watch what they send.
//!
//! A TourBox is not a HID device, so it does not appear anywhere the other
//! subcommands look. It is a USB serial port, and the only thing that makes
//! one recognisable is its USB vendor identity — see
//! [`roadie_tourbox::model`] for why a TourBox Neo cannot be found this way.
//!
//! `listen` exists for the same reason `streamdeck verify` does: the layout
//! of a controller nobody can see has to be checked by pressing things and
//! hearing what comes back. It names the control rather than printing a
//! code, because a hexadecimal byte read aloud is not an answer.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{Args, Subcommand};
use roadie_tourbox::serial::{SerialError, TourBox, ports};

use crate::spoken::counted;

/// Nothing was attached. The scan itself worked, so this is not a failure of
/// the command — the same status `roadie streamdeck` reports for it.
const NOTHING_FOUND: u8 = 2;

/// A device was found but could not be opened. Distinct from finding
/// nothing, because the thing to do about it is completely different.
const UNREACHABLE: u8 = 3;

/// What a TourBox is, in one sentence, for someone who has not met one.
const WHAT_IT_IS: &str = "A TourBox is a controller with buttons, a knob, a dial and a scroll wheel, \
     meant for the hand that is not on the mouse.";

/// Advice when no TourBox is attached.
///
/// Cable first, deliberately. A TourBox ships with a cable that many people
/// replace with whatever is nearest, and a charge-only USB-C cable presents
/// no device at all — which looks exactly like a broken controller. That was
/// the actual cause on this project's first encounter with one.
const NOTHING_FOUND_ADVICE: &[&str] = &[
    "Check the cable first. A charge-only USB-C cable carries power but no data, \
     so the controller lights up and never appears to this computer.",
    "If it is a TourBox Neo, it cannot be recognised automatically, because it \
     connects through a general-purpose serial adapter that this build cannot \
     tell apart from any other. Name its port yourself with the port argument.",
    "Bluetooth is not read by this build. Connect the controller with a cable.",
];

/// Options for `roadie tourbox`.
#[derive(Debug, Args)]
pub struct TourBoxArgs {
    /// What to do. Listing what is attached is the default.
    #[command(subcommand)]
    pub cmd: Option<TourBoxCmd>,
}

/// The `roadie tourbox` subcommands.
#[derive(Debug, Subcommand)]
pub enum TourBoxCmd {
    /// List the TourBox controllers attached to this computer.
    List,
    /// Print each button press and wheel turn as it arrives, to check the
    /// controller against this build.
    Listen(ListenArgs),
}

/// Options for `roadie tourbox listen`.
#[derive(Debug, Args)]
pub struct ListenArgs {
    /// Stop after this many seconds with nothing pressed.
    #[arg(long, default_value_t = 30)]
    pub quiet_seconds: u64,

    /// The serial port to open, for a controller this build cannot
    /// recognise on its own.
    #[arg(long)]
    pub port: Option<String>,
}

impl TourBoxCmd {
    /// Run the subcommand and report the process exit status.
    ///
    /// # Errors
    ///
    /// When the host's serial ports cannot be listed at all, which is a
    /// failure of the computer rather than of any controller.
    pub fn run(self) -> Result<ExitCode> {
        match self {
            Self::List => list(),
            Self::Listen(args) => listen(&args),
        }
    }
}

/// Say what is attached.
fn list() -> Result<ExitCode> {
    let found = ports()?;
    if found.is_empty() {
        println!("No TourBox is attached.");
        println!();
        println!("{WHAT_IT_IS}");
        println!();
        for line in NOTHING_FOUND_ADVICE {
            println!("{line}");
        }
        return Ok(ExitCode::from(NOTHING_FOUND));
    }

    println!(
        "{} attached:",
        counted(found.len(), "TourBox is", "TourBoxes are")
    );
    for port in &found {
        println!("  {}", port.describe());
        let model = port.model;
        println!(
            "    {} and {}",
            counted(model.buttons.len(), "button", "buttons"),
            counted(model.wheels.len(), "wheel", "wheels")
        );
        println!("    to see what it sends: roadie tourbox listen");
    }
    Ok(ExitCode::SUCCESS)
}

/// Watch one controller and name what it sends.
fn listen(args: &ListenArgs) -> Result<ExitCode> {
    let opened = if let Some(path) = &args.port {
        TourBox::open_path(path, None)
    } else {
        let found = ports()?;
        let Some(port) = found.first() else {
            println!("No TourBox is attached, so there is nothing to listen to.");
            println!();
            for line in NOTHING_FOUND_ADVICE {
                println!("{line}");
            }
            return Ok(ExitCode::from(NOTHING_FOUND));
        };
        if found.len() > 1 {
            println!(
                "{} attached. Listening to the first one; name a port to choose another.",
                counted(found.len(), "TourBox is", "TourBoxes are")
            );
        }
        TourBox::open(port)
    };

    let mut tourbox = match opened {
        Ok(tourbox) => tourbox,
        Err(error) => {
            println!("{}", unreachable_advice(&error));
            return Ok(ExitCode::from(UNREACHABLE));
        }
    };

    let model = tourbox
        .model()
        .map_or("TourBox", |model: &roadie_tourbox::Model| model.name);
    println!("Listening to the {model} on {}.", tourbox.path());
    println!(
        "Press buttons and turn the wheels. Stops after {} of quiet.",
        counted(
            usize::try_from(args.quiet_seconds).unwrap_or(usize::MAX),
            "second",
            "seconds"
        )
    );
    println!();

    let quiet = Duration::from_secs(args.quiet_seconds);
    let mut last = Instant::now();
    let mut seen: usize = 0;
    while last.elapsed() < quiet {
        match tourbox.read_event() {
            Ok(Some(event)) => {
                seen += 1;
                println!("{}", event.describe());
                last = Instant::now();
            }
            Ok(None) => {}
            // A byte the protocol cannot explain is worth saying and is not
            // worth stopping for: the encoding has no framing, so the next
            // byte is a fresh event rather than the rest of a broken one.
            Err(SerialError::Protocol { source, .. }) => {
                seen += 1;
                println!("Something this build does not recognise: {source}");
                last = Instant::now();
            }
            Err(error) => {
                println!("{error}");
                return Ok(ExitCode::from(UNREACHABLE));
            }
        }
    }

    println!();
    if seen == 0 {
        println!("Nothing arrived. {}", nothing_heard_advice());
        return Ok(ExitCode::from(NOTHING_FOUND));
    }
    println!("Stopped after {}.", counted(seen, "event", "events"));
    Ok(ExitCode::SUCCESS)
}

/// What to say when a controller was found but would not open.
///
/// The Stream Deck taught this lesson on this desk: a device that enumerates
/// but will not open is almost always another program holding it, and the
/// error the operating system gives says nothing about which one. Naming the
/// likely culprit is the difference between a dead end and a fix.
fn unreachable_advice(error: &SerialError) -> String {
    format!(
        "{error}\n\nThe usual cause is another program already holding the port. \
         TourBox Console is the likely one; quit it and try again."
    )
}

/// What to say when the port opened and stayed silent.
fn nothing_heard_advice() -> String {
    "The port opened, so the controller is there and this build could read it. \
     Either nothing was pressed, or another program is holding the controller's \
     input. TourBox Console is the likely one; quit it and try again."
        .to_owned()
}

#[cfg(test)]
mod tests;
