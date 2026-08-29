//! `openlogi streamdeck` — drive an Elgato Stream Deck, and check the driver
//! against real hardware.
//!
//! `verify` is the reason this subcommand exists in this shape. The protocol
//! layer is unit-tested but has never met a device, and two things genuinely
//! cannot be settled without one: which HID collection carries the key
//! traffic, and whether the original Stream Deck reports its keys mirrored
//! within each row. `verify` exercises both and prints an answer someone can
//! paste into an issue, which is a far better way to close that gap than
//! asking a user to describe what happened.

use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use clap::{Args, Subcommand};
use openlogi_hid::streamdeck::{self, Attached, Session};
use openlogi_streamdeck::report::{Brightness, KeyAction};

/// Exit status for "the scan succeeded, but no Stream Deck is attached".
const NOTHING_FOUND: u8 = 2;

/// How long `verify` and `watch` wait for a key press.
const WATCH: Duration = Duration::from_secs(15);

/// `openlogi streamdeck`, whose bare form defaults to `list`.
#[derive(Debug, Args)]
pub struct StreamDeckArgs {
    #[command(subcommand)]
    pub cmd: Option<StreamDeckCmd>,
}

#[derive(Debug, Subcommand)]
pub enum StreamDeckCmd {
    /// List every Stream Deck collection the OS reports (the default action).
    List,
    /// Check the driver against attached hardware and print what it found.
    Verify,
    /// Set the key screens' brightness, as a percentage.
    Brightness(BrightnessArgs),
    /// Reset the device to its stock standby screen.
    Reset,
    /// Print key presses until interrupted.
    Watch,
}

#[derive(Debug, Args)]
pub struct BrightnessArgs {
    /// Brightness percentage, 0 to 100.
    pub percent: u8,
}

impl StreamDeckCmd {
    /// Run the chosen Stream Deck operation.
    ///
    /// # Errors
    ///
    /// Propagates enumeration and I/O failures. "No device attached" is not an
    /// error: it exits [`NOTHING_FOUND`].
    pub async fn run(self) -> Result<ExitCode> {
        let collections = streamdeck::attached()
            .await
            .context("failed to enumerate HID devices")?;
        if collections.is_empty() {
            println!("No Stream Deck found.");
            println!();

            // Distinguish "nothing is plugged in" from "something Elgato is
            // plugged in that this build does not know". They look identical
            // to a user and have completely different answers.
            let strangers = streamdeck::unrecognized()
                .await
                .context("failed to enumerate HID devices")?;
            if strangers.is_empty() {
                println!("No Elgato device is visible to this program at all. If one is");
                println!("plugged in, the usual causes are:");
                println!("  - on Linux, no permission on its hidraw node (see the udev rules)");
                println!("  - on macOS, this program has not been granted Input Monitoring");
            } else {
                println!("An Elgato device IS attached, but this build does not recognize it:");
                for stranger in &strangers {
                    println!(
                        "  product {:#06x} — {:?}, usage {:#06x}:{:#06x}",
                        stranger.product_id, stranger.name, stranger.usage_page, stranger.usage_id
                    );
                }
                println!();
                println!("Adding a model to the catalogue needs only the product id above.");
                println!("Please open an issue with these lines.");
            }
            return Ok(ExitCode::from(NOTHING_FOUND));
        }

        match self {
            Self::List => list(&collections),
            Self::Verify => return verify(&collections).await,
            Self::Brightness(args) => {
                let brightness =
                    Brightness::new(args.percent).map_err(|error| anyhow!("{error}"))?;
                let mut session = open_preferred(&collections).await?;
                session.set_brightness(brightness).await?;
                println!("brightness set to {}%", args.percent);
            }
            Self::Reset => {
                let mut session = open_preferred(&collections).await?;
                session.reset().await?;
                println!("device reset to its standby screen");
            }
            Self::Watch => {
                let mut session = open_preferred(&collections).await?;
                println!(
                    "watching {} — press its keys; interrupt to stop",
                    session.model().name
                );
                loop {
                    for event in session.next_events().await? {
                        println!("  {}", describe(session.model(), event));
                    }
                }
            }
        }
        Ok(ExitCode::SUCCESS)
    }
}

/// Print every collection, marking the one the driver would open.
fn list(collections: &[Attached]) {
    println!("Stream Deck collections reported by this host:");
    for attached in collections {
        println!(
            "  {} — usage {:#06x}:{:#06x}{}",
            attached.model.name,
            attached.usage_page,
            attached.usage_id,
            if attached.is_preferred_collection() {
                "  <- the driver opens this one"
            } else {
                ""
            }
        );
        println!(
            "      product name {:?}, serial {}",
            attached.name,
            attached
                .serial_number
                .as_deref()
                .unwrap_or("(none reported)")
        );
        println!(
            "      {} keys, {} columns x {} rows",
            attached.model.key_count(),
            attached.model.grid.columns,
            attached.model.grid.rows
        );
    }
}

/// Open the collection the driver prefers.
async fn open_preferred(collections: &[Attached]) -> Result<Session> {
    let preferred = streamdeck::preferred(collections);
    let chosen = preferred
        .first()
        .ok_or_else(|| anyhow!("no Stream Deck collection could be selected"))?;
    Session::open(chosen)
        .await
        .with_context(|| format!("failed to open the {}", chosen.model.name))
}

/// Describe a key event by where the key is, not only by its index.
fn describe(
    model: &openlogi_streamdeck::model::Model,
    event: openlogi_streamdeck::report::KeyEvent,
) -> String {
    let action = match event.action {
        KeyAction::Pressed => "pressed",
        KeyAction::Released => "released",
    };
    model.key_position(event.key).map_or_else(
        |_| format!("key {} {action}", event.key),
        |position| {
            format!(
                "key {} {action} (row {}, column {})",
                event.key, position.row, position.column
            )
        },
    )
}

/// Exercise the driver against real hardware and report what happened.
async fn verify(collections: &[Attached]) -> Result<ExitCode> {
    println!("Stream Deck driver check");
    println!("========================");
    println!();
    list(collections);
    println!();

    let mut session = open_preferred(collections).await?;
    let model = session.model();
    println!("Opened: {} ({} keys)", model.name, model.key_count());
    println!();

    print!("Setting brightness to 30%... ");
    match session
        .set_brightness(Brightness::new(30).unwrap_or(Brightness::FULL))
        .await
    {
        Ok(()) => println!("accepted — the screens should have dimmed."),
        Err(error) => println!("FAILED: {error}"),
    }
    print!("Restoring full brightness... ");
    match session.set_brightness(Brightness::FULL).await {
        Ok(()) => println!("accepted."),
        Err(error) => println!("FAILED: {error}"),
    }
    println!();

    println!(
        "Now press the TOP-LEFT key on the {} and hold it briefly.",
        model.name
    );
    println!("Waiting up to {} seconds...", WATCH.as_secs());
    println!();

    let observed = tokio::time::timeout(WATCH, async {
        loop {
            let events = session.next_events().await?;
            if let Some(event) = events
                .iter()
                .find(|event| event.action == KeyAction::Pressed)
            {
                return Ok::<_, anyhow::Error>(*event);
            }
        }
    })
    .await;

    match observed {
        Ok(Ok(event)) => {
            let position = model.key_position(event.key);
            println!("Saw: {}", describe(model, event));
            match position {
                Ok(p) if p.row == 1 && p.column == 1 => {
                    println!();
                    println!("CORRECT — the top-left key reported as row 1, column 1.");
                    println!("Key ordering for this model is right.");
                }
                Ok(p) => {
                    println!();
                    println!(
                        "MISMATCH — the top-left key reported as row {}, column {}.",
                        p.row, p.column
                    );
                    println!("The key ordering for this model is wrong in the catalogue.");
                    println!("Please open an issue with the two lines above and the");
                    println!("collection list at the top of this output.");
                }
                Err(error) => println!("The reported key is out of range: {error}"),
            }
        }
        Ok(Err(error)) => println!("Reading key events FAILED: {error}"),
        Err(_) => {
            println!("No key press seen in {} seconds.", WATCH.as_secs());
            println!("Either no key was pressed, or this collection does not carry key");
            println!("events — in which case the usage-page choice above is wrong.");
        }
    }
    Ok(ExitCode::SUCCESS)
}
