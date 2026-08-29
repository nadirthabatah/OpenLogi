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

mod layout;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use clap::{Args, Subcommand};
use openlogi_hid::streamdeck::{self, Attached, Session};
use openlogi_streamdeck::render;
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
    /// Fill one key with a solid colour.
    Fill(FillArgs),
    /// Show a picture on one key. Any common image format is accepted.
    Image(ImageArgs),
    /// Write a text label on one key, sized to fit.
    Label(LabelArgs),
    /// Clear every key back to black.
    Clear,
    /// Apply a whole deck layout from a file.
    Apply(PathArgs),
    /// Write an example layout file to get started from.
    Example(PathArgs),
}

#[derive(Debug, Args)]
pub struct PathArgs {
    /// The layout file.
    pub file: PathBuf,
}

#[derive(Debug, Args)]
pub struct FillArgs {
    /// Key index, counting from 0 at the top left.
    pub key: u16,
    /// Six hex digits, "RRGGBB", with no leading '#'.
    pub colour: String,
}

#[derive(Debug, Args)]
pub struct LabelArgs {
    /// Key index, counting from 0 at the top left.
    pub key: u16,
    /// The words to write. They wrap, and are sized to fill the key.
    pub text: String,
    /// Text colour, six hex digits.
    #[arg(long, default_value = "ffffff")]
    pub colour: String,
    /// Background colour, six hex digits.
    #[arg(long, default_value = "000000")]
    pub background: String,
}

#[derive(Debug, Args)]
pub struct ImageArgs {
    /// Key index, counting from 0 at the top left.
    pub key: u16,
    /// The picture to show. It is scaled and rotated to fit the key.
    pub file: PathBuf,
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
        // Writing an example layout needs no device, and wanting one before
        // the hardware arrives — or on a machine that will never have it — is
        // the ordinary case, not an edge one.
        if let Self::Example(args) = &self {
            std::fs::write(&args.file, EXAMPLE_LAYOUT)
                .with_context(|| format!("failed to write {}", args.file.display()))?;
            println!("example layout written to {}", args.file.display());
            println!(
                "Edit it, then: openlogi streamdeck apply {}",
                args.file.display()
            );
            return Ok(ExitCode::SUCCESS);
        }

        // Likewise, a layout that does not parse is worth saying so about
        // before demanding hardware: the file is wrong either way, and "no
        // Stream Deck found" would send someone hunting the wrong problem.
        let parsed = match &self {
            Self::Apply(args) => Some(read_layout(&args.file)?),
            _ => None,
        };

        let collections = streamdeck::attached()
            .await
            .context("failed to enumerate HID devices")?;
        if collections.is_empty() {
            report_nothing_found().await?;
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
            Self::Fill(args) => {
                let colour = parse_colour(&args.colour)?;
                draw(&collections, args.key, &Drawing::Fill(colour)).await?;
            }
            Self::Label(args) => {
                let ink = parse_colour(&args.colour)?;
                let paper = parse_colour(&args.background)?;
                draw(
                    &collections,
                    args.key,
                    &Drawing::Label(&args.text, ink, paper),
                )
                .await?;
            }
            Self::Image(args) => {
                let picture = image::open(&args.file)
                    .with_context(|| format!("failed to read {}", args.file.display()))?;
                draw(
                    &collections,
                    args.key,
                    &Drawing::Picture(&picture, &args.file),
                )
                .await?;
            }
            Self::Clear => {
                let mut session = open_preferred(&collections).await?;
                let model = session.model();
                let black = render::solid(model, 0, 0, 0).map_err(|e| anyhow!("{e}"))?;
                let encoded = render::key_image(model, &black).map_err(|e| anyhow!("{e}"))?;
                for key in 0..model.key_count() {
                    session.set_key_image(key, &encoded).await?;
                }
                println!("cleared all {} keys", model.key_count());
            }
            Self::Apply(args) => {
                let layout = parsed.ok_or_else(|| anyhow!("the layout was not read"))?;
                return apply(&collections, &args.file, &layout).await;
            }
            // Handled before the device scan above.
            Self::Example(_) => unreachable!("example returns before the device scan"),
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

/// A layout to start from, written by `openlogi streamdeck example`.
const EXAMPLE_LAYOUT: &str = r#"# A Stream Deck layout.
#
# Keys count from 0 at the top left, running left to right then down.
# Apply it with: openlogi streamdeck apply this-file.toml
#
# Nothing a deck shows survives unplugging it, so this file is where a
# layout actually lives — keep it in git, carry it between machines, and
# re-apply it whenever the deck is plugged back in.

brightness = 80

[[keys]]
index = 0
label = "MUTE MIC"
background = "802020"

[[keys]]
index = 1
label = "REC"
colour = "ff4040"

# A key can show a picture instead of words. The path is relative to this
# file, so a layout and its icons travel together.
# [[keys]]
# index = 2
# image = "icons/camera.png"
"#;

/// Read and parse a layout file, without needing a device.
fn read_layout(file: &Path) -> Result<layout::Layout> {
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    layout::Layout::parse(file, &source).map_err(|error| anyhow!("{error}"))
}

/// Apply a parsed layout to the attached deck.
async fn apply(collections: &[Attached], file: &Path, layout: &layout::Layout) -> Result<ExitCode> {
    let mut session = open_preferred(collections).await?;
    let model = session.model();
    // Validated against the model before anything is written, so a layout
    // naming a key this deck does not have fails with nothing half-applied.
    layout.validate(model).map_err(|error| anyhow!("{error}"))?;

    if let Some(percent) = layout.brightness {
        let brightness = Brightness::new(percent).map_err(|error| anyhow!("{error}"))?;
        session.set_brightness(brightness).await?;
        println!("brightness set to {percent}%");
    }

    for key in &layout.keys {
        let ink = key
            .colour
            .as_deref()
            .map_or(Ok((255, 255, 255)), parse_colour)?;
        let paper = key
            .background
            .as_deref()
            .map_or(Ok((0, 0, 0)), parse_colour)?;
        let picture = if let Some(text) = &key.label {
            render::label(model, text, ink, paper).map_err(|e| anyhow!("{e}"))?
        } else if let Some(relative) = &key.image {
            let path = layout::Layout::resolve(file, relative);
            image::open(&path).with_context(|| format!("failed to read {}", path.display()))?
        } else {
            // `validate` rejects an entry with neither, so this is unreachable
            // unless that rule and this loop disagree.
            unreachable!("validate rejects a key with nothing to draw");
        };
        let encoded = render::key_image(model, &picture).map_err(|e| anyhow!("{e}"))?;
        session.set_key_image(key.index, &encoded).await?;
        println!(
            "  key {} ({})",
            key.index,
            describe_key_position(model, key.index)
        );
    }
    println!(
        "applied {} key(s) from {}",
        layout.keys.len(),
        file.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// What to put on a key.
///
/// The three drawing commands differ only in what they produce and what they
/// say afterwards; naming that difference keeps one open-encode-write path
/// instead of three near-copies that could drift apart.
enum Drawing<'a> {
    /// A solid colour.
    Fill((u8, u8, u8)),
    /// Text, with its ink and paper colours.
    Label(&'a str, (u8, u8, u8), (u8, u8, u8)),
    /// A picture read from `path`.
    Picture(&'a image::DynamicImage, &'a std::path::Path),
}

/// Open the preferred deck, encode `drawing` for it, and write it to `key`.
async fn draw(collections: &[Attached], key: u16, drawing: &Drawing<'_>) -> Result<()> {
    let mut session = open_preferred(collections).await?;
    let model = session.model();
    let picture = match drawing {
        Drawing::Fill((red, green, blue)) => {
            render::solid(model, *red, *green, *blue).map_err(|e| anyhow!("{e}"))?
        }
        Drawing::Label(text, ink, paper) => {
            render::label(model, text, *ink, *paper).map_err(|e| anyhow!("{e}"))?
        }
        Drawing::Picture(picture, _) => (*picture).clone(),
    };
    let encoded = render::key_image(model, &picture).map_err(|e| anyhow!("{e}"))?;
    session.set_key_image(key, &encoded).await?;

    let what = match drawing {
        Drawing::Fill((red, green, blue)) => format!("filled with {red:02x}{green:02x}{blue:02x}"),
        Drawing::Label(text, ..) => format!("now reads {text:?}"),
        Drawing::Picture(_, path) => format!("now shows {}", path.display()),
    };
    println!("key {key} {what} ({})", describe_key_position(model, key));
    Ok(())
}

/// Explain an empty scan: nothing attached, or something attached that this
/// build does not know. They look identical to a user and have opposite
/// answers.
async fn report_nothing_found() -> Result<()> {
    println!("No Stream Deck found.");
    println!();
    let strangers = streamdeck::unrecognized()
        .await
        .context("failed to enumerate HID devices")?;
    if strangers.is_empty() {
        println!("No Elgato device is visible to this program at all. If one is");
        println!("plugged in, the usual causes are:");
        println!("  - on Linux, no permission on its hidraw node (see the udev rules)");
        println!("  - on macOS, this program has not been granted Input Monitoring");
        return Ok(());
    }
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
    Ok(())
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

/// Parse a six-hex-digit colour.
fn parse_colour(text: &str) -> Result<(u8, u8, u8)> {
    let packed = (text.len() == 6)
        .then(|| u32::from_str_radix(text, 16).ok())
        .flatten()
        .ok_or_else(|| {
            anyhow!("colour must be 6 hex digits, \"RRGGBB\", with no leading '#' — got {text:?}")
        })?;
    // Each shift-and-mask selects one byte, so none of these can truncate.
    Ok((
        u8::try_from((packed >> 16) & 0xff).unwrap_or_default(),
        u8::try_from((packed >> 8) & 0xff).unwrap_or_default(),
        u8::try_from(packed & 0xff).unwrap_or_default(),
    ))
}

/// Where a key sits, phrased for reading aloud.
fn describe_key_position(model: &openlogi_streamdeck::model::Model, key: u16) -> String {
    model.key_position(key).map_or_else(
        |_| "out of range".to_string(),
        |position| format!("row {}, column {}", position.row, position.column),
    )
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
    println!(
        "Opened: {} ({} keys)",
        session.model().name,
        session.model().key_count()
    );
    println!();

    check_brightness(&mut session).await;
    let painted = check_key_image(&mut session).await;
    check_key_press(&mut session, painted).await;
    Ok(ExitCode::SUCCESS)
}

/// Dim the screens and restore them — a visible change that needs no
/// interpretation, so it separates "the device is listening" from everything
/// subtler that follows.
async fn check_brightness(session: &mut Session) {
    print!("Setting brightness to {}%... ", Brightness::DIM.percent());
    match session.set_brightness(Brightness::DIM).await {
        Ok(()) => println!("accepted — the screens should have dimmed."),
        Err(error) => println!("FAILED: {error}"),
    }
    print!("Restoring full brightness... ");
    match session.set_brightness(Brightness::FULL).await {
        Ok(()) => println!("accepted."),
        Err(error) => println!("FAILED: {error}"),
    }
    println!();
}

/// Paint one key and say what a wrong result would look like.
///
/// Returns whether the write was accepted, so the key-press step can tell the
/// reader whether the two paths agreed.
async fn check_key_image(session: &mut Session) -> bool {
    let model = session.model();
    print!("Painting the top-left key orange... ");
    let encoded = match render::solid(model, 0xff, 0x88, 0x00)
        .and_then(|picture| render::key_image(model, &picture))
    {
        Ok(encoded) => encoded,
        Err(error) => {
            println!("FAILED to encode: {error}");
            return false;
        }
    };
    if let Err(error) = session.set_key_image(0, &encoded).await {
        println!("FAILED to write: {error}");
        return false;
    }
    println!("accepted.");
    println!();
    println!("  Look at the device. The TOP-LEFT key should now be orange.");
    println!("  If a *different* key changed colour, key numbering is wrong for");
    println!("  this model. If the colour is there but the image looks rotated or");
    println!("  mirrored, the catalogue's rotation for this model is wrong.");
    println!("  Either is worth reporting, with the collection list above.");
    println!();
    true
}

/// Ask for the top-left key and report whether it arrived where the catalogue
/// says it should — the question this whole command exists to answer.
async fn check_key_press(session: &mut Session, painted: bool) {
    let model = session.model();
    println!(
        "Now press the TOP-LEFT key on the {} — the one you were just looking at.",
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

    let event = match observed {
        Ok(Ok(event)) => event,
        Ok(Err(error)) => {
            println!("Reading key events FAILED: {error}");
            return;
        }
        Err(_) => {
            println!("No key press seen in {} seconds.", WATCH.as_secs());
            println!("Either no key was pressed, or this collection does not carry key");
            println!("events — in which case the usage-page choice above is wrong.");
            return;
        }
    };

    println!("Saw: {}", describe(model, event));
    match model.key_position(event.key) {
        Ok(p) if p.row == 1 && p.column == 1 => {
            println!();
            println!("CORRECT — the top-left key reported as row 1, column 1.");
            println!("Key ordering for this model is right.");
            if painted {
                println!("If that was also the key that turned orange, the write and");
                println!("read paths agree and this model is fully confirmed.");
            }
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

#[cfg(test)]
mod tests {
    use super::parse_colour;

    #[test]
    fn a_six_digit_colour_splits_into_its_channels() {
        assert_eq!(parse_colour("ff8800").expect("valid"), (0xff, 0x88, 0x00));
        assert_eq!(parse_colour("000000").expect("valid"), (0, 0, 0));
        assert_eq!(parse_colour("ffffff").expect("valid"), (255, 255, 255));
        // Channel order is the one thing here that can be silently wrong.
        assert_eq!(parse_colour("010203").expect("valid"), (1, 2, 3));
    }

    #[test]
    fn upper_and_lower_case_hex_both_parse() {
        assert_eq!(
            parse_colour("AbCdEf").expect("valid"),
            parse_colour("abcdef").expect("valid")
        );
    }

    #[test]
    fn anything_that_is_not_six_hex_digits_is_refused() {
        for bad in ["#ff8800", "ff880", "ff88000", "", "gggggg", "ff 880"] {
            let error = parse_colour(bad).expect_err(bad);
            assert!(
                error.to_string().contains("6 hex digits"),
                "the message must say what is wanted: {error}"
            );
        }
    }
}
