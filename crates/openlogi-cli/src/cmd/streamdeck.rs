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

use crate::spoken::counted;

mod layout;
mod library;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use clap::{Args, Subcommand};
use openlogi_hid::streamdeck::{self, Attached, Session};
use openlogi_streamdeck::font;
use openlogi_streamdeck::render;
use openlogi_streamdeck::report::{Brightness, KeyAction};

/// Exit status for "the scan succeeded, but no Stream Deck is attached".
const NOTHING_FOUND: u8 = 2;

/// Exit status for "the layout binds actions that run programs, and they were
/// not accepted". Distinct from a read or parse failure so a script can tell
/// them apart — the same status `openlogi profile import` uses for the same
/// reason.
const UNTRUSTED: u8 = 3;

/// Exit status for "that layout already exists and was not overwritten".
const EXISTS: u8 = 4;

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
    /// Apply a whole deck layout, by library name or file path.
    Apply(PathArgs),
    /// Apply a layout, then run it: pressing a key performs its action.
    Run(RunArgs),
    /// Write an example layout to start from, into the library or a file.
    Example(PathArgs),
    /// List the layouts saved in the library.
    Layouts,
    /// Set one key in a saved layout, without opening a text editor.
    Set(SetArgs),
    /// Remove one key from a saved layout.
    Unset(UnsetArgs),
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// A layout: a name in your library, or a path to a file.
    pub layout: String,
    /// Key index, counting from 0 at the top left.
    pub key: u16,
    /// Words to write on the key.
    #[arg(long)]
    pub label: Option<String>,
    /// A picture to show on it instead of words.
    #[arg(long)]
    pub image: Option<PathBuf>,
    /// Text colour, six hex digits, no leading '#'. Defaults to white.
    #[arg(long)]
    pub colour: Option<String>,
    /// Background colour, six hex digits. Defaults to black.
    #[arg(long)]
    pub background: Option<String>,
    /// What pressing the key does, as an action name such as `Copy`.
    #[arg(long)]
    pub action: Option<String>,
}

#[derive(Debug, Args)]
pub struct UnsetArgs {
    /// A layout: a name in your library, or a path to a file.
    pub layout: String,
    /// Key index, counting from 0 at the top left.
    pub key: u16,
}

#[derive(Debug, Args)]
pub struct PathArgs {
    /// A layout: a name in your library, or a path to a file.
    ///
    /// A bare word is a name — `streaming` means the layout you saved under
    /// that name. Anything with a slash or a `.toml` on the end is a path.
    pub layout: String,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A layout: a name in your library, or a path to a file.
    pub layout: String,
    /// Accept actions that run a program or type text.
    ///
    /// Without this, a layout carrying any such action is refused and nothing
    /// is applied. Inspect the file first — the refusal lists exactly what it
    /// found and where.
    #[arg(long)]
    pub accept_actions: bool,
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
        // Listing the library needs no device either, and is the answer to
        // "what did I call that layout".
        if matches!(self, Self::Layouts) {
            return list_layouts();
        }

        // A bare name means the library; a path means that path. Resolved
        // once here so every use below sees a real file.
        let target = match &self {
            Self::Apply(args) | Self::Example(args) => Some(library::resolve(&args.layout)?),
            Self::Run(args) => Some(library::resolve(&args.layout)?),
            Self::Set(args) => Some(library::resolve(&args.layout)?),
            Self::Unset(args) => Some(library::resolve(&args.layout)?),
            _ => None,
        };

        // Editing a layout needs no device, and is most of what someone does
        // with one — a deck that is not plugged in yet is still a deck whose
        // layout can be written.
        match &self {
            Self::Set(args) => {
                let path = target.ok_or_else(|| anyhow!("the layout path was not resolved"))?;
                return set_key(&path, args);
            }
            Self::Unset(args) => {
                let path = target.ok_or_else(|| anyhow!("the layout path was not resolved"))?;
                return unset_key(&path, args.key);
            }
            _ => {}
        }

        // Writing an example layout needs no device, and wanting one before
        // the hardware arrives — or on a machine that will never have it — is
        // the ordinary case, not an edge one.
        if let Self::Example(args) = &self {
            let path = target.ok_or_else(|| anyhow!("the example path was not resolved"))?;
            return write_example(&args.layout, &path);
        }

        // Likewise, a layout that does not parse is worth saying so about
        // before demanding hardware: the file is wrong either way, and "no
        // Stream Deck found" would send someone hunting the wrong problem.
        let parsed = match (&self, target.as_deref()) {
            (Self::Apply(_) | Self::Run(_), Some(path)) => Some(read_layout(path)?),
            _ => None,
        };

        // A layout that would run programs is refused before the device is
        // even opened, so nothing is applied and nothing is bound.
        if let Self::Run(args) = &self
            && !args.accept_actions
            && let Some(layout) = &parsed
            && refuse_untrusted(layout)?
        {
            return Ok(ExitCode::from(UNTRUSTED));
        }

        let collections = streamdeck::attached()
            .await
            .context("failed to enumerate HID devices")?;
        if collections.is_empty() {
            report_nothing_found().await?;
            return Ok(ExitCode::from(NOTHING_FOUND));
        }

        self.dispatch(&collections, parsed, target.as_deref()).await
    }

    /// The half of the command tree that needs an attached device.
    ///
    /// Split from [`Self::run`], which first handles everything that does
    /// not: writing an example, reading a layout, and refusing one that
    /// would run programs.
    async fn dispatch(
        self,
        collections: &[Attached],
        parsed: Option<layout::Layout>,
        target: Option<&Path>,
    ) -> Result<ExitCode> {
        match self {
            Self::List => list(collections),
            Self::Verify => return verify(collections).await,
            Self::Brightness(args) => {
                let brightness =
                    Brightness::new(args.percent).map_err(|error| anyhow!("{error}"))?;
                let mut session = open_preferred(collections).await?;
                session.set_brightness(brightness).await?;
                println!("brightness set to {}%", args.percent);
            }
            Self::Reset => {
                let mut session = open_preferred(collections).await?;
                session.reset().await?;
                println!("device reset to its standby screen");
            }
            Self::Fill(args) => {
                let colour = parse_colour(&args.colour)?;
                draw(collections, args.key, &Drawing::Fill(colour)).await?;
            }
            Self::Label(args) => {
                let ink = parse_colour(&args.colour)?;
                let paper = parse_colour(&args.background)?;
                draw(
                    collections,
                    args.key,
                    &Drawing::Label(&args.text, ink, paper),
                )
                .await?;
            }
            Self::Image(args) => {
                let picture = image::open(&args.file)
                    .with_context(|| format!("failed to read {}", args.file.display()))?;
                draw(
                    collections,
                    args.key,
                    &Drawing::Picture(&picture, &args.file),
                )
                .await?;
            }
            Self::Clear => {
                let mut session = open_preferred(collections).await?;
                let model = session.model();
                let black = render::solid(model, 0, 0, 0).map_err(|e| anyhow!("{e}"))?;
                let encoded = render::key_image(model, &black).map_err(|e| anyhow!("{e}"))?;
                for key in 0..model.key_count() {
                    session.set_key_image(key, &encoded).await?;
                }
                println!("cleared all {} keys", model.key_count());
            }
            Self::Apply(_) => {
                let layout = parsed.ok_or_else(|| anyhow!("the layout was not read"))?;
                let path = target.ok_or_else(|| anyhow!("the layout path was not resolved"))?;
                return apply(collections, path, &layout).await;
            }
            Self::Run(_) => {
                let layout = parsed.ok_or_else(|| anyhow!("the layout was not read"))?;
                let path = target.ok_or_else(|| anyhow!("the layout path was not resolved"))?;
                return run_layout(collections, path, &layout).await;
            }
            // Handled before the device scan above.
            Self::Example(_) | Self::Layouts | Self::Set(_) | Self::Unset(_) => {
                unreachable!("handled before the device scan")
            }
            Self::Watch => {
                let mut session = open_preferred(collections).await?;
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

# `action` says what pressing the key does, drawn from the same catalogue
# every other device here uses — so a Stream Deck key and a mouse button are
# bound the same way. Apply the faces with `apply`; act on presses with `run`.
#
# [[keys]]
# index = 3
# label = "COPY"
# action = "Copy"
#
# An action that runs a program or types text needs `run --accept-actions`,
# which is your decision to trust where the layout came from.
#
# [[keys]]
# index = 4
# label = "BUILD"
# action = { RunShellCommand = "make -C ~/project" }
"#;

/// Where this machine keeps its Stream Deck layouts.
///
/// Re-exported for the profile bundle, which has to gather them: the library's
/// location is one fact, and two modules deciding it separately is how a
/// bundle ends up carrying nothing from a directory that is full.
///
/// # Errors
///
/// Fails when the configuration directory cannot be determined.
pub fn layout_library() -> Result<std::path::PathBuf> {
    library::directory()
}

/// Every saved layout's name.
///
/// Re-exported for the MCP server so an assistant offers names that exist
/// rather than ones it inferred from the conversation.
///
/// # Errors
///
/// Fails when the configuration directory cannot be determined.
pub fn saved_layouts() -> Result<Vec<String>> {
    library::list()
}

/// Apply a saved layout by name, returning how many keys it set.
///
/// The MCP server's route in. Shares [`apply`] with the CLI rather than
/// repeating it, so an assistant and a person applying the same layout get
/// the same result — including the same refusals.
///
/// # Errors
///
/// Fails when the layout cannot be read or parsed, no deck is attached, or
/// the device rejects a write.
// The three functions below are the layout surface an assistant drives, and
// each resolves its argument with `resolve_saved_name` rather than
// `library::resolve`. The difference is the whole point: the command line
// takes a name *or* a path, because a person who types a path means it, while
// a name arriving over MCP came from a model that can be steered by whatever
// it has been reading. Names only, inside the library, or nothing.
pub async fn apply_saved(name: &str) -> Result<usize> {
    let path = library::resolve_saved_name(name)?;
    let parsed = read_layout(&path)?;
    let collections = streamdeck::attached()
        .await
        .context("failed to enumerate HID devices")?;
    if collections.is_empty() {
        return Err(anyhow!(
            "no Stream Deck is attached. `openlogi doctor` says whether that is a \
             permissions problem rather than an absent device."
        ));
    }
    let keys = parsed.keys.len();
    apply(&collections, &path, &parsed).await?;
    Ok(keys)
}

/// One key of a layout, re-exported so the MCP server can name the type.
pub type LayoutKey = layout::Key;

/// Build a layout key from its parts.
///
/// A constructor rather than a struct literal at each call site, so the MCP
/// server does not need the layout module public and the two ways of setting a
/// key cannot drift in what they consider a key to be.
#[must_use]
pub fn layout_key(
    index: u16,
    label: Option<String>,
    image: Option<String>,
    colour: Option<String>,
    background: Option<String>,
    action: Option<openlogi_core::binding::Action>,
) -> LayoutKey {
    layout::Key {
        index,
        label,
        image: image.map(PathBuf::from),
        colour,
        background,
        action,
    }
}

/// Set one key in a saved layout, for the MCP server.
///
/// Returns the key as it was, or `None` if the key was not in the layout, so
/// a caller can offer to put it back. A file write is permanent in a way a
/// deck's own screens are not — they go when the cable does — so the previous
/// value travelling with the answer is what keeps a mistaken change
/// reversible.
///
/// # Errors
///
/// Fails when the layout cannot be read, the key description is not usable,
/// or the file cannot be written.
pub fn set_layout_key(name: &str, key: &layout::Key) -> Result<Option<layout::Key>> {
    let path = library::resolve_saved_name(name)?;
    let (source, was) = if path.exists() {
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        // Parsed only to report what the edit replaced, and to refuse a file
        // that is not a layout before writing to it. The edit itself is
        // applied to the text, so the person's comments survive it.
        let parsed = layout::Layout::parse(&path, &source).map_err(|error| anyhow!("{error}"))?;
        let was = parsed
            .keys
            .iter()
            .find(|held| held.index == key.index)
            .cloned();
        (source, was)
    } else {
        (String::new(), None)
    };
    let edited = layout::edit::set_key(&source, key).map_err(|error| anyhow!("{error}"))?;
    write_layout_text(&path, &edited)?;
    Ok(was)
}

/// Remove one key from a saved layout, for the MCP server.
///
/// Returns the key that was removed, or `None` when it was not there.
///
/// # Errors
///
/// Fails when the layout does not exist, cannot be read, or cannot be written.
pub fn unset_layout_key(name: &str, index: u16) -> Result<Option<layout::Key>> {
    let path = library::resolve_saved_name(name)?;
    if !path.exists() {
        return Err(anyhow!("there is no layout at {}", path.display()));
    }
    let source = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = layout::Layout::parse(&path, &source).map_err(|error| anyhow!("{error}"))?;
    let was = parsed.keys.iter().find(|key| key.index == index).cloned();
    let (edited, removed) =
        layout::edit::remove_key(&source, index).map_err(|error| anyhow!("{error}"))?;
    if removed {
        write_layout_text(&path, &edited)?;
    }
    Ok(was)
}

/// `openlogi streamdeck set`.
///
/// The point of this command is that configuring a deck should never require
/// opening a text editor and getting TOML right. That is friction for anyone
/// and a wall for someone working by dictation, which is the case this project
/// is built around.
///
/// Writes to a layout that does not exist yet rather than refusing: the first
/// key someone sets is the layout, and making them run `example` first to get
/// a file full of things they did not ask for is a step with no purpose.
fn set_key(path: &Path, args: &SetArgs) -> Result<ExitCode> {
    if args.label.is_some() && args.image.is_some() {
        eprintln!("A key shows words or a picture, not both. Pass one of --label or --image.");
        return Ok(ExitCode::from(EXISTS));
    }
    // Colours are checked here rather than at apply time, so a typo is caught
    // while the person is still thinking about that key.
    for (name, value) in [
        ("--colour", &args.colour),
        ("--background", &args.background),
    ] {
        if let Some(value) = value {
            parse_colour(value).with_context(|| format!("{name} is not a colour"))?;
        }
    }
    let action = args.action.as_deref().map(parse_action).transpose()?;

    // Read as text and edited as text, so the comments and spacing in a file
    // someone wrote survive an edit to one key of it. Parsed as well, but only
    // to refuse a file that is not a layout and to say whether the key was
    // already there.
    let source = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };
    let existing = layout::Layout::parse(path, &source)
        .map_err(|error| anyhow!("{error}"))?
        .keys
        .iter()
        .any(|key| key.index == args.key);
    let edited = layout::edit::set_key(
        &source,
        &layout::Key {
            index: args.key,
            label: args.label.clone(),
            image: args.image.clone(),
            colour: args.colour.clone(),
            background: args.background.clone(),
            action,
        },
    )
    .map_err(|error| anyhow!("{error}"))?;

    write_layout_text(path, &edited)?;
    println!(
        "key {} {} in {}",
        args.key,
        if existing { "replaced" } else { "added" },
        path.display()
    );
    if let Some(label) = &args.label {
        warn_about_undrawable(label);
    }
    // Said because a key set with only an action shows nothing, and someone
    // who cannot see the deck has no other way to notice.
    if args.label.is_none() && args.image.is_none() {
        println!("That key will stay blank — it has an action but nothing to show.");
    }
    Ok(ExitCode::SUCCESS)
}

/// `openlogi streamdeck unset`.
fn unset_key(path: &Path, key: u16) -> Result<ExitCode> {
    if !path.exists() {
        eprintln!("There is no layout at {}.", path.display());
        return Ok(ExitCode::from(NOTHING_FOUND));
    }
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    // Parsed to refuse a file that is not a layout before writing to it.
    layout::Layout::parse(path, &source).map_err(|error| anyhow!("{error}"))?;
    let (edited, removed) =
        layout::edit::remove_key(&source, key).map_err(|error| anyhow!("{error}"))?;
    if !removed {
        // Told "removed", someone believes something changed, and the next
        // thing they do is wonder why the deck looks the same.
        eprintln!(
            "Key {key} was not in {}, so nothing changed.",
            path.display()
        );
        return Ok(ExitCode::from(NOTHING_FOUND));
    }
    write_layout_text(path, &edited)?;
    println!("key {key} removed from {}", path.display());
    Ok(ExitCode::SUCCESS)
}

/// Turn an action name into an action.
///
/// Goes through the same serde representation the layout file uses, so
/// whatever a file accepts the command line accepts, and the two cannot come
/// to disagree about what an action is called.
pub fn parse_action(name: &str) -> Result<openlogi_core::binding::Action> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).map_err(|_| {
        anyhow!(
            "{name} is not an action this build knows. Actions are named the way they \
             are in a layout file — Copy, Paste, NextTab, VolumeUp and so on. An action \
             that takes a value, such as RunShellCommand, has to be written in the file \
             rather than passed here."
        )
    })
}

/// Write a layout back to its file, creating the library if it is new.
fn write_layout_text(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, body).with_context(|| format!("failed to write {}", path.display()))
}

/// `openlogi streamdeck layouts`.
///
/// An empty library is not a failure and does not read like one: on a fresh
/// machine there is nothing saved yet, and the useful answer is how to make
/// the first one rather than a bare "none".
fn list_layouts() -> Result<ExitCode> {
    let names = library::list()?;
    let directory = library::directory()?;
    if names.is_empty() {
        println!("No layouts saved yet.");
        println!();
        println!("Layouts live in {}.", directory.display());
        println!("Start one with: openlogi streamdeck example streaming");
        return Ok(ExitCode::SUCCESS);
    }
    println!("Layouts ({}), in {}:", names.len(), directory.display());
    for name in &names {
        println!("  {name}");
    }
    println!();
    println!("Apply one with: openlogi streamdeck apply <name>");
    Ok(ExitCode::SUCCESS)
}

/// `openlogi streamdeck example`.
///
/// Refuses to write over an existing layout. Overwriting the layout someone
/// spent an evening on with the stock example, because they reached for
/// `example` to remind themselves of the syntax, is not a mistake they can
/// undo — the deck's own memory is not a copy, it goes when the cable does.
fn write_example(argument: &str, path: &Path) -> Result<ExitCode> {
    if path.exists() {
        eprintln!(
            "{} already exists. Not overwriting it — the deck's own memory is not \
             a copy of it, so there would be nothing to restore from.",
            path.display()
        );
        eprintln!("Pick another name, or delete that file first.");
        return Ok(ExitCode::from(EXISTS));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, EXAMPLE_LAYOUT)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("example layout written to {}", path.display());
    println!("Edit it, then: openlogi streamdeck apply {argument}");
    Ok(ExitCode::SUCCESS)
}

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
        "applied {} from {}",
        counted(layout.keys.len(), "key", "keys"),
        file.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// Report a layout's program-running actions, if it has any.
///
/// Returns whether the layout was refused. Same rule, same audit and the same
/// wording as importing a profile: whether a layout's source is trustworthy is
/// a judgement about provenance, and it belongs to the person at the keyboard.
fn refuse_untrusted(layout: &layout::Layout) -> Result<bool> {
    let findings =
        crate::profile::audit_serializable(layout).context("failed to audit the layout")?;
    if findings.is_empty() {
        return Ok(false);
    }
    eprintln!(
        "this layout binds {} that would run a program or type text on your \
         machine. Nothing has been applied. Review them, then re-run with \
         --accept-actions if you trust the source:",
        counted(findings.len(), "action", "actions")
    );
    for finding in &findings {
        eprintln!("  {finding}");
    }
    Ok(true)
}

/// Apply a layout, then act on key presses until interrupted.
///
/// The two halves of a macro pad: the face, and what pressing it does. Actions
/// come from the same catalogue every other device here uses, so a Stream Deck
/// key and a mouse button are bound the same way and mean the same thing.
async fn run_layout(
    collections: &[Attached],
    file: &Path,
    layout: &layout::Layout,
) -> Result<ExitCode> {
    apply(collections, file, layout).await?;

    let bound: std::collections::BTreeMap<u16, &openlogi_core::binding::Action> = layout
        .keys
        .iter()
        .filter_map(|key| key.action.as_ref().map(|action| (key.index, action)))
        .collect();
    if bound.is_empty() {
        println!();
        println!("No key in this layout has an action, so there is nothing to run.");
        println!("Add an `action` to a key — `openlogi streamdeck example` shows how.");
        return Ok(ExitCode::SUCCESS);
    }

    let mut session = open_preferred(collections).await?;
    let model = session.model();
    println!();
    println!(
        "Running {}. Interrupt to stop.",
        counted(bound.len(), "bound key", "bound keys")
    );
    for (index, action) in &bound {
        println!(
            "  key {index} ({}) -> {action:?}",
            describe_key_position(model, *index)
        );
    }
    println!();

    loop {
        for event in session.next_events().await? {
            // Act on the press, not the release: a key that fires twice per
            // push would double every action bound to it.
            if event.action != KeyAction::Pressed {
                continue;
            }
            let Some(action) = bound.get(&event.key) else {
                println!(
                    "  key {} pressed ({}) — nothing bound",
                    event.key,
                    describe_key_position(model, event.key)
                );
                continue;
            };
            println!(
                "  key {} pressed ({}) -> {action:?}",
                event.key,
                describe_key_position(model, event.key)
            );
            openlogi_inject::execute(action);
        }
    }
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
    if let Drawing::Label(text, ..) = drawing {
        warn_about_undrawable(text);
    }
    Ok(())
}

/// Say which characters of a label the key font cannot draw.
///
/// They render as hollow boxes, which is visible on the key and says nothing
/// about what went wrong — and says nothing at all to whoever cannot see the
/// key. The command reports success either way, so without this the only
/// signal is the one signal this project cannot rely on.
fn warn_about_undrawable(text: &str) {
    let missing = font::missing_from(text);
    if missing.is_empty() {
        return;
    }
    let listed: Vec<String> = missing.iter().map(|c| format!("{c:?}")).collect();
    // No count in front of the sentence, and "each" rather than "it" or
    // "they", so the wording is right whether one character is missing or
    // twenty.
    println!(
        "  The key font cannot draw: {}. Each becomes a hollow box on the key.",
        listed.join(", ")
    );
    println!(
        "  It carries capitals, digits and common punctuation; lowercase is drawn as \
         capitals. Use a picture for anything else."
    );
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
        println!();
        println!("`openlogi doctor` works out which of those it is and says what to do.");
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
pub fn parse_colour(text: &str) -> Result<(u8, u8, u8)> {
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
    use super::{EXAMPLE_LAYOUT, layout, parse_colour};

    /// The example is the first layout most people ever run, and until this
    /// test it was the one layout nothing checked. A stray word in it is a
    /// parse error handed to someone on their first attempt.
    #[test]
    fn the_shipped_example_parses_and_validates() {
        let parsed = layout::Layout::parse(std::path::Path::new("example.toml"), EXAMPLE_LAYOUT)
            .expect("the example we hand people must parse");
        // Every model with key screens, so the example is not quietly sized
        // for whichever deck happened to be on the author's desk. The Pedal
        // has keys but no screens, so a face layout cannot apply to it.
        for model in openlogi_streamdeck::model::MODELS
            .iter()
            .filter(|model| model.screens.is_some())
        {
            parsed
                .validate(model)
                .unwrap_or_else(|error| panic!("example rejected on the {}: {error}", model.name));
        }
    }

    /// The example documents `action` in comments, and a comment cannot fail
    /// to compile. These are those exact snippets, uncommented: if the action
    /// schema ever moves, the example stops teaching a shape that works.
    #[test]
    fn the_action_forms_the_example_documents_are_real() {
        let source = "\
[[keys]]
index = 3
label = \"COPY\"
action = \"Copy\"

[[keys]]
index = 4
label = \"BUILD\"
action = { RunShellCommand = \"make -C ~/project\" }

[[keys]]
index = 5
label = \"SHOT\"
action = { CustomShortcut = \"cmd+shift+4\" }
";
        let parsed = layout::Layout::parse(std::path::Path::new("actions.toml"), source)
            .expect("the documented action forms must parse");
        let actions: Vec<_> = parsed.keys.iter().map(|key| key.action.clone()).collect();
        assert_eq!(
            actions,
            vec![
                Some(openlogi_core::binding::Action::Copy),
                Some(openlogi_core::binding::Action::RunShellCommand(
                    "make -C ~/project".to_owned()
                )),
                Some(openlogi_core::binding::Action::CustomShortcut(
                    "cmd+shift+4".parse().expect("a chord the docs offer")
                )),
            ]
        );
    }

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
