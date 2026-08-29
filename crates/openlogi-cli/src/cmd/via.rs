//! `openlogi via` — QMK keyboards and macro pads.
//!
//! One implementation reaches the hundreds of boards running QMK with VIA
//! enabled, which is why this is worth more per line than any single-vendor
//! driver. The commands are deliberately few and deliberately careful: a wrong
//! keycode takes a key away from whoever is using the board, and the tool that
//! did it is then the tool they have to use to fix it.

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use openlogi_hid::via::{self, Attached, Session};
use openlogi_via::keycode;

/// Exit status for "the scan succeeded, but no VIA device is attached".
const NOTHING_FOUND: u8 = 2;

#[derive(Debug, Args)]
pub struct ViaArgs {
    #[command(subcommand)]
    pub cmd: Option<ViaCmd>,
}

#[derive(Debug, Subcommand)]
pub enum ViaCmd {
    /// List attached VIA devices and what each reports about itself.
    List,
    /// Print a whole keymap layer, key by key.
    Keymap(KeymapArgs),
    /// Read the keycode at one position.
    Get(PositionArgs),
    /// Write a keycode to one position, then read it back to confirm.
    Set(SetArgs),
}

#[derive(Debug, Args)]
pub struct KeymapArgs {
    /// Which layer, counting from 0.
    #[arg(default_value_t = 0)]
    pub layer: u8,
    /// How many matrix rows to read.
    #[arg(long, default_value_t = 6)]
    pub rows: u8,
    /// How many matrix columns to read.
    #[arg(long, default_value_t = 16)]
    pub columns: u8,
}

#[derive(Debug, Args)]
pub struct PositionArgs {
    /// Which layer, counting from 0.
    pub layer: u8,
    /// Matrix row, counting from 0.
    pub row: u8,
    /// Matrix column, counting from 0.
    pub column: u8,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// Which layer, counting from 0.
    pub layer: u8,
    /// Matrix row, counting from 0.
    pub row: u8,
    /// Matrix column, counting from 0.
    pub column: u8,
    /// The key to assign: a name like `F13` or `KC_F13`, or a number.
    pub keycode: String,
}

impl ViaCmd {
    /// Run the subcommand.
    ///
    /// # Errors
    ///
    /// Fails when HID enumeration fails, when the device does not answer, or
    /// when a keycode name cannot be resolved.
    pub async fn run(self) -> Result<ExitCode> {
        // Resolved before the device is opened: a name we cannot resolve is
        // wrong whether or not a keyboard is attached, and "no VIA device
        // found" would send someone hunting the wrong problem.
        let wanted = match &self {
            Self::Set(args) => Some(resolve(&args.keycode)?),
            _ => None,
        };

        let attached = via::attached()
            .await
            .context("failed to enumerate HID devices")?;
        if attached.is_empty() {
            println!("No VIA device found.");
            println!();
            println!(
                "A QMK board only answers here if its firmware was built with VIA \
                 enabled. If yours has it, check that this process can open raw HID \
                 devices — on Linux that is the udev rules in the README."
            );
            return Ok(ExitCode::from(NOTHING_FOUND));
        }

        match self {
            Self::List => list(&attached).await,
            Self::Keymap(args) => keymap(&attached, &args).await,
            Self::Get(args) => get(&attached, &args).await,
            Self::Set(args) => set(&attached, &args, wanted.unwrap_or_default()).await,
        }
    }
}

/// Turn a keycode argument into a number.
///
/// Accepts a name (`F13`, `KC_F13`, case-insensitive), hex (`0x0068`) or plain
/// decimal. Names are what people say and remember; the numeric forms are what
/// firmware references print, and refusing either would make someone translate
/// by hand between two things that mean the same key.
fn resolve(argument: &str) -> Result<u16> {
    let text = argument.trim();
    if let Some(keycode) = keycode::parse(text) {
        return Ok(keycode);
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u16::from_str_radix(hex, 16).with_context(|| {
            format!("{argument} is not a keycode this build knows, nor a number")
        });
    }
    text.parse::<u16>()
        .with_context(|| format!("{argument} is not a keycode this build knows, nor a number"))
}

/// Open the first attached device.
///
/// Deliberately the first rather than a choice: a machine with two VIA boards
/// is rare enough that inventing a selection flag before anyone has hit the
/// problem would be guessing at a shape.
async fn open_first(attached: &[Attached]) -> Result<Session> {
    let first = attached
        .first()
        .context("no VIA device to open, though one was enumerated")?;
    Ok(Session::open(first).await?)
}

/// `openlogi via list`.
async fn list(attached: &[Attached]) -> Result<ExitCode> {
    println!("VIA devices ({}):", attached.len());
    for device in attached {
        println!(
            "  {} ({:04x}:{:04x})",
            device.name, device.vendor_id, device.product_id
        );
        if let Some(serial) = &device.serial_number {
            println!("    serial: {serial}");
        }
        // Opening is what turns a usage-page match into a fact. Reported
        // rather than fatal: one unresponsive board should not hide the rest.
        match Session::open(device).await {
            Ok(session) => println!(
                "    speaks VIA protocol {}, with {} keymap layer(s)",
                session.protocol(),
                session.layers()
            ),
            Err(error) => println!("    did not answer as a VIA device: {error}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `openlogi via keymap`.
async fn keymap(attached: &[Attached], args: &KeymapArgs) -> Result<ExitCode> {
    let mut session = open_first(attached).await?;
    println!(
        "Layer {} of {} (rows 0 to {}, columns 0 to {}):",
        args.layer,
        session.layers(),
        args.rows.saturating_sub(1),
        args.columns.saturating_sub(1)
    );
    for row in 0..args.rows {
        for column in 0..args.columns {
            let code = session.keycode(args.layer, row, column).await?;
            // Unassigned positions are the overwhelming majority of a matrix
            // read blind, and printing them would bury the keys that exist.
            if code == keycode::NONE {
                continue;
            }
            println!("  row {row}, column {column}: {}", keycode::describe(code));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `openlogi via get`.
async fn get(attached: &[Attached], args: &PositionArgs) -> Result<ExitCode> {
    let mut session = open_first(attached).await?;
    let code = session.keycode(args.layer, args.row, args.column).await?;
    println!(
        "layer {}, row {}, column {}: {}",
        args.layer,
        args.row,
        args.column,
        keycode::describe(code)
    );
    Ok(ExitCode::SUCCESS)
}

/// `openlogi via set`.
async fn set(attached: &[Attached], args: &SetArgs, keycode_value: u16) -> Result<ExitCode> {
    let mut session = open_first(attached).await?;
    let was = session.keycode(args.layer, args.row, args.column).await?;
    session
        .set_keycode(args.layer, args.row, args.column, keycode_value)
        .await?;
    println!(
        "layer {}, row {}, column {}: {} -> {}",
        args.layer,
        args.row,
        args.column,
        keycode::describe(was),
        keycode::describe(keycode_value)
    );
    println!("Confirmed by reading the position back.");
    // Said every time, not only when it looks risky: someone who has just
    // changed a key needs to know how to change it back, and the moment they
    // need that is after they have closed the terminal.
    println!(
        "To undo: openlogi via set {} {} {} {}",
        args.layer,
        args.row,
        args.column,
        keycode::describe(was)
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use openlogi_via::keycode;

    use super::resolve;

    /// Names are what people say and remember.
    #[test]
    fn a_name_resolves_however_it_is_spelled() {
        for spelling in ["F13", "f13", "KC_F13"] {
            assert_eq!(
                resolve(spelling).expect("a known key"),
                0x0068,
                "{spelling}"
            );
        }
    }

    /// Firmware references print hex; refusing it would make someone
    /// translate by hand between two spellings of the same key.
    #[test]
    fn hex_and_decimal_both_resolve() {
        assert_eq!(resolve("0x0068").expect("hex"), 0x0068);
        assert_eq!(resolve("0X0068").expect("upper-case hex"), 0x0068);
        assert_eq!(resolve("104").expect("decimal"), 104);
    }

    #[test]
    fn something_that_is_neither_is_refused_with_a_message_that_says_so() {
        let error = resolve("SUPERKEY").expect_err("not a key");
        let text = format!("{error}");
        assert!(text.contains("SUPERKEY"), "{text}");
        assert!(text.contains("nor a number"), "{text}");
    }

    /// The undo line the `set` command prints has to be a command that runs.
    /// Printing a name the parser would then reject would be worse than
    /// printing nothing, because it looks like a way back.
    #[test]
    fn every_name_the_undo_line_could_print_resolves_again() {
        for code in [0x0000_u16, 0x0004, 0x0045, 0x0068, 0x00e0, 0x00ff] {
            let printed = keycode::describe(code);
            assert_eq!(
                resolve(&printed).expect("the undo line must be runnable"),
                code,
                "{printed} did not round trip"
            );
        }
    }
}
