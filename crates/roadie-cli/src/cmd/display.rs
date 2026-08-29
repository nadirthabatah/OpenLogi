//! `roadie display` — the monitor's own menu, from the keyboard.
//!
//! Every desk monitor has a menu behind four unlabelled buttons on its bezel.
//! For someone who can see it that is an annoyance. For someone who cannot it
//! is not usable at all: the menu is drawn on the screen, in a font the
//! monitor renders itself, where no screen reader can reach it. DDC/CI is the
//! same menu over the video cable, and this is that.
//!
//! It is called `display` rather than `monitor` for a reason worth keeping:
//! `roadie`'s MCP server already has a `monitor` module, and there "monitor"
//! is the verb — it watches for a button press. Two unrelated things under one
//! word is bad on screen and worse aloud.
//!
//! # Everything here can be driven with no monitor attached
//!
//! The functions that decide what to say take values, not devices, and the
//! tests hand them straight to `spoken::assert_listenable` and
//! `spoken::assert_agrees`. Output that needs hardware to produce cannot be
//! swept for the patterns that break a screen reader, so none of it does.

use std::fmt::Write as _;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use roadie_ddc::vcp::{InputSource, PowerMode};
use roadie_ddc::{Capabilities, Feature, Value};
use roadie_display::{Acknowledged, Display, DisplayError, Risk};

use crate::spoken::counted;

#[derive(Debug, Args)]
pub struct DisplayArgs {
    #[command(subcommand)]
    pub cmd: Option<DisplayCmd>,
}

#[derive(Debug, Subcommand)]
pub enum DisplayCmd {
    /// List the monitors this build can see, and say which answer.
    List,
    /// Read one setting.
    Get(GetArgs),
    /// Change one setting.
    Set(SetArgs),
    /// Read or set brightness as a percentage, the setting most reached for.
    Brightness(BrightnessArgs),
    /// Ask a monitor what it says it can do.
    Capabilities(SelectArgs),
    /// Commit the current settings to the monitor's own memory.
    Save(SaveArgs),
}

/// Which monitor to act on, when more than one is attached.
#[derive(Debug, Args)]
pub struct SelectArgs {
    /// Case-insensitive part of the monitor's name, as `roadie display list`
    /// says it. Not needed when only one monitor is attached.
    #[arg(long)]
    display: Option<String>,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    #[command(flatten)]
    select: SelectArgs,
    /// The setting to read: brightness, contrast, volume, input, power, or a
    /// feature code such as 0xe2.
    feature: String,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    #[command(flatten)]
    select: SelectArgs,
    /// The setting to change.
    feature: String,
    /// The new value: a number, a percentage such as 80%, or a name such as
    /// hdmi2 for the input or standby for the power.
    value: String,
    /// Go ahead with a change that cannot be undone from the keyboard.
    #[arg(long)]
    r#yes: bool,
}

#[derive(Debug, Args)]
pub struct BrightnessArgs {
    #[command(flatten)]
    select: SelectArgs,
    /// The percentage to set. Left out, the current brightness is read.
    #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
    percent: Option<u8>,
}

#[derive(Debug, Args)]
pub struct SaveArgs {
    #[command(flatten)]
    select: SelectArgs,
    /// Go ahead. Required, because this writes to memory with a limited
    /// number of rewrites.
    #[arg(long)]
    r#yes: bool,
}

impl DisplayCmd {
    /// Dispatch, defaulting to the list — the only subcommand that is useful
    /// before you know what is attached.
    pub fn run(self) -> Result<()> {
        match self {
            Self::List => list(),
            Self::Get(args) => get(&args),
            Self::Set(args) => set(&args),
            Self::Brightness(args) => brightness(&args),
            Self::Capabilities(args) => capabilities(&args),
            Self::Save(args) => save(&args),
        }
    }
}

/// Whether a display answered a probe, and what to say if it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reach {
    /// It answered. A monitor that answers anything speaks DDC.
    Answers,
    /// It is attached and would not answer, with the reason in full.
    Silent(String),
}

/// Ask each display the cheapest question there is.
///
/// The MCCS version is the right probe: every monitor that speaks DDC has one,
/// nothing is changed by asking, and a monitor that answers it at all is a
/// monitor the rest of these commands will work on.
fn probe(displays: &mut [Display]) -> Vec<(String, Reach)> {
    displays
        .iter_mut()
        .map(|display| {
            let name = display.describe();
            let reach = match display.get(Feature::McssVersion) {
                Ok(_) => Reach::Answers,
                Err(error) => Reach::Silent(sentence(&error)),
            };
            (name, reach)
        })
        .collect()
}

/// An error as a sentence, with the full stop it needs to be read as one.
fn sentence(error: &DisplayError) -> String {
    let text = error.to_string();
    if text.ends_with('.') {
        text
    } else {
        format!("{text}.")
    }
}

fn list() -> Result<()> {
    let mut displays = roadie_display::enumerate()?;
    print!("{}", render_list(&probe(&mut displays)));
    Ok(())
}

/// What `roadie display list` says.
fn render_list(rows: &[(String, Reach)]) -> String {
    if rows.is_empty() {
        return "No monitors found. A monitor has to be connected by a cable that carries \
                DDC, and switched on. A laptop's own screen never speaks DDC, so it does \
                not appear here.\n"
            .to_owned();
    }

    let mut out = format!("{} attached.\n", counted(rows.len(), "monitor", "monitors"));
    for (name, reach) in rows {
        match reach {
            Reach::Answers => {
                let _ = writeln!(out, "  {name} answers and can be controlled.");
            }
            Reach::Silent(why) => {
                let _ = writeln!(out, "  {name} does not answer: {why}");
            }
        }
    }

    let answering = rows
        .iter()
        .filter(|(_, reach)| *reach == Reach::Answers)
        .count();
    if answering == 0 {
        out.push_str(
            "Nothing here can be controlled yet. The commonest cause is DDC/CI switched \
             off in the monitor's own menu, where many ship it off.\n",
        );
    } else {
        out.push_str("Read a setting with: roadie display get brightness\n");
    }
    out
}

/// The one display to act on, or an error naming the choice.
fn select(args: &SelectArgs) -> Result<Display> {
    let mut displays = roadie_display::enumerate()?;
    if let Some(wanted) = args.display.as_deref() {
        let found = displays.iter().position(|display| display.matches(wanted));
        return match found {
            Some(at) => Ok(displays.swap_remove(at)),
            None => bail!(
                "No attached monitor's name contains {wanted:?}. Run roadie display list \
                 to see what is attached."
            ),
        };
    }
    match displays.len() {
        0 => bail!(
            "No monitors found. Run roadie display list, which explains what has to be \
             true for one to appear."
        ),
        1 => Ok(displays.swap_remove(0)),
        many => bail!(
            "{} attached, so which one has to be said: add --display and part of a name. \
             Run roadie display list to see them.",
            counted(many, "monitor is", "monitors are")
        ),
    }
}

fn get(args: &GetArgs) -> Result<()> {
    let feature = parse_feature(&args.feature)?;
    let mut display = select(&args.select)?;
    let name = display.describe();
    let value = display.get(feature)?;
    print!("{}", render_reading(&name, feature, value));
    Ok(())
}

/// What a reading of one feature says.
///
/// A percentage on its own hides the thing that actually bites: a monitor's
/// maximum is not always 100, and is not always honest. So both are said, and
/// the raw pair is what someone reports when a panel misbehaves.
fn render_reading(name: &str, feature: Feature, value: Value) -> String {
    let setting = describe_feature(feature);
    match feature {
        Feature::InputSource => {
            let source = InputSource::from_code(low_byte(value.current));
            let named = source
                .name()
                .map_or_else(|| format!("value {}", value.current), str::to_owned);
            format!("{name} input source is {named}.\n")
        }
        Feature::PowerMode => {
            let mode = PowerMode::from_code(low_byte(value.current));
            let named = mode
                .name()
                .map_or_else(|| format!("value {}", value.current), str::to_owned);
            format!("{name} power is {named}.\n")
        }
        _ => match value.percent() {
            Some(percent) => format!(
                "{name} {setting} is {percent} percent, or {} out of a maximum of {}.\n",
                value.current, value.maximum
            ),
            // A maximum of zero is a broken answer rather than a real range,
            // and dividing by it is how a percentage becomes a crash.
            None => format!(
                "{name} {setting} is {}, and the monitor reports no maximum for it.\n",
                value.current
            ),
        },
    }
}

/// A feature's name, or a spoken form of its code when it has none.
fn describe_feature(feature: Feature) -> String {
    feature
        .name()
        .map_or_else(|| format!("feature {:#04x}", feature.code()), str::to_owned)
}

/// The low byte of a value whose meaning is a one-byte code.
///
/// Input source and power mode are both single bytes in a sixteen-bit field.
/// A monitor that puts something in the high byte has said something outside
/// the standard, and the low byte is still the part that means anything.
fn low_byte(value: u16) -> u8 {
    u8::try_from(value & 0xFF).unwrap_or(0)
}

fn set(args: &SetArgs) -> Result<()> {
    let feature = parse_feature(&args.feature)?;
    let mut display = select(&args.select)?;
    let name = display.describe();

    // Read first. It costs one exchange and buys two things: the maximum, so
    // a percentage can be turned into the monitor's own units, and a before
    // value to compare the write against.
    let before = display.get(feature).ok();
    let value = parse_value(feature, &args.value, before)?;

    let outcome = if args.r#yes {
        match Risk::of(feature, value) {
            Some(risk) => display.set_acknowledging(feature, value, Acknowledged::of(risk)),
            None => display.set(feature, value),
        }
    } else {
        display.set(feature, value)
    };

    if let Err(DisplayError::Refused(risk)) = outcome {
        print!("{}", render_refusal(risk, &args.feature, &args.value));
        return Ok(());
    }
    outcome?;

    let after = display.get(feature).ok();
    print!("{}", render_write(&name, feature, value, after));
    Ok(())
}

/// What a refusal says, and how to go ahead if it was meant.
fn render_refusal(risk: Risk, feature: &str, value: &str) -> String {
    format!(
        "Not done. {}\nTo do it anyway: roadie display set {} {} --yes\n",
        risk.spoken(),
        crate::spoken::shell_argument(feature),
        crate::spoken::shell_argument(value),
    )
}

/// What a write says, including what the monitor did with it.
///
/// The read-back is the point. A monitor answers nothing after a write, and
/// panels that report a maximum they then clamp below are common enough that
/// "set to 90" with no check is a claim rather than a fact.
fn render_write(name: &str, feature: Feature, asked: u16, after: Option<Value>) -> String {
    let setting = describe_feature(feature);
    let Some(after) = after else {
        return format!(
            "{name} {setting} set to {asked}. The monitor did not answer when it was read \
             back, so whether it took cannot be confirmed here.\n"
        );
    };
    if after.current == asked {
        return format!("{name} {setting} set to {asked}, and reads back the same.\n");
    }
    format!(
        "{name} {setting} was set to {asked} but reads back as {}. Some panels clamp a \
         setting below the maximum they report, and this looks like one of them.\n",
        after.current
    )
}

fn brightness(args: &BrightnessArgs) -> Result<()> {
    let mut display = select(&args.select)?;
    let name = display.describe();
    let Some(percent) = args.percent else {
        let value = display.get(Feature::Brightness)?;
        print!("{}", render_reading(&name, Feature::Brightness, value));
        return Ok(());
    };

    let before = display.get(Feature::Brightness)?;
    let value = Value::from_percent(percent, before.maximum);
    display.set(Feature::Brightness, value)?;
    let after = display.get(Feature::Brightness).ok();
    print!("{}", render_write(&name, Feature::Brightness, value, after));
    Ok(())
}

fn capabilities(args: &SelectArgs) -> Result<()> {
    let mut display = select(args)?;
    let name = display.describe();
    let capabilities = display.capabilities()?;
    print!("{}", render_capabilities(&name, &capabilities));
    Ok(())
}

/// What a monitor says it can do.
fn render_capabilities(name: &str, capabilities: &Capabilities) -> String {
    let mut out = String::new();
    match capabilities.model.as_deref() {
        Some(model) => {
            let _ = writeln!(out, "{name} calls itself {model}.");
        }
        None => {
            let _ = writeln!(out, "{name} does not give a model name.");
        }
    }
    if let Some(version) = capabilities.mccs_version.as_deref() {
        let _ = writeln!(out, "  It speaks MCCS version {version}.");
    }

    let named: Vec<Feature> = Feature::NAMED
        .into_iter()
        .filter(|feature| capabilities.supports(*feature))
        .collect();
    if named.is_empty() {
        out.push_str("  It lists no setting this build knows how to name.\n");
    } else {
        let _ = writeln!(
            out,
            "  {} this build can name: {}.",
            counted(named.len(), "setting", "settings"),
            named
                .iter()
                .map(|feature| describe_feature(*feature))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let inputs = capabilities.inputs();
    if !inputs.is_empty() {
        let listed: Vec<String> = inputs
            .iter()
            .map(|input| {
                input
                    .name()
                    .map_or_else(|| format!("value {}", input.code()), str::to_owned)
            })
            .collect();
        let _ = writeln!(
            out,
            "  {} to switch between: {}.",
            counted(listed.len(), "input", "inputs"),
            listed.join(", ")
        );
    }

    for warning in &capabilities.warnings {
        let _ = writeln!(
            out,
            "  Something in what it sent had to be worked around: {warning}."
        );
    }
    out
}

fn save(args: &SaveArgs) -> Result<()> {
    if !args.r#yes {
        print!(
            "Not done. {}\nTo do it anyway: roadie display save --yes\n",
            Risk::SaveSettings.spoken()
        );
        return Ok(());
    }
    let mut display = select(&args.select)?;
    let name = display.describe();
    display.save_settings(Acknowledged::of(Risk::SaveSettings))?;
    println!("{name} was told to keep its current settings.");
    Ok(())
}

/// Parse a feature a person typed or dictated.
///
/// Forgiving about case and separators for the same reason
/// [`InputSource::parse`] is: this is what sits behind `roadie display get
/// input-source`, and someone dictating it will say "input source" and get
/// whatever their speech engine writes.
fn parse_feature(text: &str) -> Result<Feature> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))
        && let Ok(code) = u8::from_str_radix(hex, 16)
    {
        return Ok(Feature::from_code(code));
    }

    let squashed = squash(text);
    // The short words people actually use. Each maps to the one feature it
    // could mean, and none of them is ambiguous against another.
    let short = match squashed.as_str() {
        "input" | "source" => Some(Feature::InputSource),
        "power" => Some(Feature::PowerMode),
        "mccs" | "version" => Some(Feature::McssVersion),
        "osd" | "language" => Some(Feature::OsdLanguage),
        "colour" | "color" | "preset" => Some(Feature::ColorPreset),
        _ => None,
    };
    if let Some(feature) = short {
        return Ok(feature);
    }

    Feature::NAMED
        .into_iter()
        .find(|feature| feature.name().is_some_and(|name| squash(name) == squashed))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "There is no setting called {text:?}. The ones with names are: {}. \
                 A feature code such as 0xe2 also works.",
                Feature::NAMED
                    .iter()
                    .filter_map(|feature| feature.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Lowercase, letters and digits only.
fn squash(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Parse a value for `feature`, in whatever form suits it.
///
/// `current` is the reading taken just before, which is what makes a
/// percentage meaningful: the monitor's own maximum is the denominator, and it
/// is not always 100.
fn parse_value(feature: Feature, text: &str, current: Option<Value>) -> Result<u16> {
    match feature {
        Feature::InputSource => {
            if let Some(source) = InputSource::parse(text) {
                return Ok(u16::from(source.code()));
            }
        }
        Feature::PowerMode => {
            if let Some(mode) = parse_power(text) {
                return Ok(u16::from(mode.code()));
            }
        }
        Feature::Mute => match squash(text).as_str() {
            "mute" | "muted" | "on" | "yes" => return Ok(0x01),
            "unmute" | "unmuted" | "off" | "no" => return Ok(0x02),
            _ => {}
        },
        _ => {}
    }

    if let Some(percent) = text.strip_suffix('%') {
        let percent: u8 = percent
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("{percent:?} is not a percentage between 0 and 100."))?;
        let Some(current) = current else {
            bail!(
                "A percentage needs the monitor's own maximum, and it did not answer when \
                 it was read. Give the value the monitor uses instead of a percentage."
            );
        };
        return Ok(Value::from_percent(percent, current.maximum));
    }

    text.parse::<u16>().map_err(|_| {
        anyhow::anyhow!(
            "{text:?} is not a value for the {}. Give a number, a percentage such as 80%, \
             or one of the names this setting accepts.",
            describe_feature(feature)
        )
    })
}

/// Parse a power state by name.
///
/// "off" and "screen off" are deliberately different words for deliberately
/// different things, and getting them the wrong way round is the one mistake
/// here that costs someone a walk to the monitor.
fn parse_power(text: &str) -> Option<PowerMode> {
    Some(match squash(text).as_str() {
        "on" => PowerMode::On,
        "standby" => PowerMode::Standby,
        "suspend" => PowerMode::Suspend,
        "screenoff" | "blank" | "activeoff" => PowerMode::ActiveOff,
        "off" | "poweredoff" | "poweroff" => PowerMode::Off,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
