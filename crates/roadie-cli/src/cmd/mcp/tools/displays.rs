//! Monitors, over DDC/CI.
//!
//! Like the camera tools, these reach the hardware directly rather than
//! through the agent: DDC is a standard the host exposes per display, not
//! agent-owned state.
//!
//! This is the category where an assistant is worth the most. Every other
//! peripheral here has some other way in — a button, a switch, a vendor app.
//! A monitor's settings live behind four unlabelled buttons and a menu the
//! monitor draws itself, which no screen reader can read, so "make the left
//! screen dimmer" has no other answer at all.
//!
//! # Two things are deliberately not exposed
//!
//! **Saving to the monitor's memory.** `roadie display save` exists and this
//! does not. That memory has a finite number of rewrites, the benefit is
//! marginal — an ordinary write already lasts until the monitor loses power —
//! and a tool that spends a limited resource is a bad thing to put within
//! reach of something that retries on failure.
//!
//! **Powering a monitor off without being asked to.** It is reachable, but
//! only with `confirm_irreversible` set, because a monitor in that state may
//! stop answering the computer at all and the way back is the button on the
//! bezel. The refusal is enforced in `roadie-display`, not here, so it holds
//! whatever calls it.

use roadie_ddc::edid::SUMMARY_FEATURES;
use roadie_ddc::vcp::{InputSource, PowerMode};
use roadie_ddc::{Feature, Value};
use roadie_display::{Acknowledged, Display, DisplayError, Risk};
use serde_json::{Value as Json, json};

use super::{no_arguments_schema, rendered};

/// The `display` argument, shared by every tool here that acts on one.
fn display_argument() -> Json {
    json!({
        "type": "string",
        "description": "Part of the monitor's name or id, from list_displays. \
            Case-insensitive. Omit to use the only monitor when exactly one \
            answers.",
    })
}

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Json> {
    vec![
        json!({
            "name": "list_displays",
            "description": "List every monitor attached, whether each one answers over \
                DDC, and why not when it does not. Start here before changing a monitor \
                setting. A laptop's built-in screen never appears: it has no DDC channel, \
                and its brightness is a system setting rather than a monitor one.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "read_display_settings",
            "description": "Read a monitor's current settings — brightness, input source, \
                contrast and volume — each with its value, the maximum that monitor \
                reports, and a percentage. Read this before setting anything: a monitor's \
                maximum is often not 100, so a percentage cannot be turned into a value \
                without it.",
            "inputSchema": json!({
                "type": "object",
                "properties": { "display": display_argument() },
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "set_display_setting",
            "description": "Change one monitor setting. `setting` is brightness, contrast, \
                volume, mute, input_source or power. `value` is a number in the monitor's \
                own units — read_display_settings gives the maximum — or a name for the \
                ones that have names: hdmi1, hdmi2, dp1, dp2, vga1, dvi1 for input_source, \
                and on, standby, suspend, screen_off or off for power. The value is read \
                back afterwards and the result says what the monitor actually took, which \
                can be lower than asked: panels that report a maximum they then clamp below \
                are common. Setting power to off may stop the monitor answering the \
                computer at all, leaving the button on its bezel as the only way back, so \
                that one value needs confirm_irreversible and should never be sent without \
                the person having asked for it in those terms.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "display": display_argument(),
                    "setting": {
                        "type": "string",
                        "enum": ["brightness", "contrast", "volume", "mute", "input_source", "power"],
                    },
                    "value": {
                        "type": ["string", "integer"],
                        "description": "A number in the monitor's units, or a name for \
                            input_source and power.",
                    },
                    "confirm_irreversible": {
                        "type": "boolean",
                        "description": "Required only to power a monitor off, which may \
                            leave it unreachable from the computer.",
                    },
                },
                "required": ["setting", "value"],
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Every monitor, and whether it answers.
pub fn list_displays() -> Result<String, String> {
    let mut displays = roadie_display::enumerate().map_err(|error| error.to_string())?;
    if displays.is_empty() {
        return Ok(
            "no monitors are attached, or none of them speaks DDC over its cable. \
             A laptop's built-in screen never does."
                .to_string(),
        );
    }
    rendered(&Json::Array(
        displays.iter_mut().map(probe_json).collect::<Vec<_>>(),
    ))
}

/// One display's entry in `list_displays`.
///
/// The MCCS version is the probe: every monitor that speaks DDC has one,
/// asking changes nothing, and a monitor that answers it will answer the rest.
fn probe_json(display: &mut Display) -> Json {
    let answered = display.get(Feature::McssVersion);
    json!({
        "id": display.id().as_str(),
        "name": display.describe(),
        "answers": answered.is_ok(),
        "why_not": answered.err().map(|error| error.to_string()),
    })
}

/// The one display a `display` argument names.
///
/// Omitting it is allowed only when exactly one monitor is attached. Silently
/// picking the first of several would change a different screen than the one
/// meant, and nobody watching would have any way to tell which.
fn choose(arguments: &Json) -> Result<Display, String> {
    let mut displays = roadie_display::enumerate().map_err(|error| error.to_string())?;
    if let Some(wanted) = arguments.get("display") {
        let wanted = wanted
            .as_str()
            .ok_or_else(|| format!("`display` must be a name or id string, not {wanted}"))?;
        let found = displays.iter().position(|display| display.matches(wanted));
        return match found {
            Some(at) => Ok(displays.swap_remove(at)),
            None => Err(format!(
                "no attached monitor's name or id contains {wanted:?} — call \
                 list_displays and pass one of the names it gives"
            )),
        };
    }
    match displays.len() {
        1 => Ok(displays.swap_remove(0)),
        0 => Err("no monitors are attached".to_string()),
        count => Err(format!(
            "{count} monitors are attached, so `display` is required — call \
             list_displays and pass the name of the one you mean"
        )),
    }
}

/// A monitor's current settings.
pub fn read_display_settings(arguments: &Json) -> Result<String, String> {
    let mut display = choose(arguments)?;
    let name = display.describe();
    let readings: Vec<Json> = SUMMARY_FEATURES
        .into_iter()
        .map(|feature| reading_json(feature, display.get(feature)))
        .collect();
    rendered(&json!({
        "display": name,
        "settings": readings,
    }))
}

/// One feature's reading, or the reason there is not one.
///
/// A feature the monitor does not have is reported rather than omitted: "this
/// panel has no speakers" is a useful answer to "set the volume", and an
/// absent key would leave that to be guessed at.
fn reading_json(feature: Feature, value: Result<Value, DisplayError>) -> Json {
    let name = setting_name(feature);
    match value {
        Ok(value) => json!({
            "setting": name,
            "value": value.current,
            "maximum": value.maximum,
            "percent": value.percent(),
            "meaning": meaning(feature, value),
        }),
        Err(error) => json!({
            "setting": name,
            "unavailable": error.to_string(),
        }),
    }
}

/// What a value means, for the features whose numbers are codes.
fn meaning(feature: Feature, value: Value) -> Option<String> {
    let code = u8::try_from(value.current & 0xFF).unwrap_or(0);
    match feature {
        Feature::InputSource => InputSource::from_code(code).name().map(str::to_owned),
        Feature::PowerMode => PowerMode::from_code(code).name().map(str::to_owned),
        _ => None,
    }
}

/// Change one setting.
pub fn set_display_setting(arguments: &Json) -> Result<String, String> {
    let setting = arguments
        .get("setting")
        .and_then(Json::as_str)
        .ok_or("`setting` is required")?;
    let feature = feature_named(setting)?;
    let requested = arguments.get("value").ok_or("`value` is required")?;

    let mut display = choose(arguments)?;
    let name = display.describe();
    let before = display.get(feature).ok();
    let value = value_for(feature, requested, before)?;

    let confirmed = arguments
        .get("confirm_irreversible")
        .and_then(Json::as_bool)
        .unwrap_or(false);
    let outcome = match (Risk::of(feature, value), confirmed) {
        (Some(risk), true) => display.set_acknowledging(feature, value, Acknowledged::of(risk)),
        _ => display.set(feature, value),
    };

    if let Err(DisplayError::Refused(risk)) = outcome {
        return Err(format!(
            "not done, and this needs the person's agreement rather than a retry. {} \
             Ask them in those words, and only if they say yes, call this again with \
             confirm_irreversible set to true.",
            risk.spoken()
        ));
    }
    outcome.map_err(|error| error.to_string())?;

    let after = display.get(feature).ok();
    rendered(&json!({
        "display": name,
        "setting": setting_name(feature),
        "asked_for": value,
        "reads_back_as": after.map(|value| value.current),
        "note": after.and_then(|after| {
            (after.current != value).then(|| {
                "the monitor took a different value than it was given; some panels clamp \
                 below the maximum they report".to_owned()
            })
        }),
    }))
}

/// The tool's name for a feature.
fn setting_name(feature: Feature) -> &'static str {
    match feature {
        Feature::Brightness => "brightness",
        Feature::Contrast => "contrast",
        Feature::Volume => "volume",
        Feature::Mute => "mute",
        Feature::InputSource => "input_source",
        Feature::PowerMode => "power",
        _ => "other",
    }
}

/// The feature a tool argument names.
fn feature_named(setting: &str) -> Result<Feature, String> {
    match setting {
        "brightness" => Ok(Feature::Brightness),
        "contrast" => Ok(Feature::Contrast),
        "volume" => Ok(Feature::Volume),
        "mute" => Ok(Feature::Mute),
        "input_source" => Ok(Feature::InputSource),
        "power" => Ok(Feature::PowerMode),
        other => Err(format!(
            "`setting` must be brightness, contrast, volume, mute, input_source or \
             power, not {other:?}"
        )),
    }
}

/// The number to write, from whatever form the argument arrived in.
fn value_for(feature: Feature, requested: &Json, before: Option<Value>) -> Result<u16, String> {
    if let Some(number) = requested.as_u64() {
        return u16::try_from(number)
            .map_err(|_| format!("{number} is too large for a monitor setting"));
    }
    let text = requested
        .as_str()
        .ok_or_else(|| format!("`value` must be a number or a name, not {requested}"))?;

    match feature {
        Feature::InputSource => {
            if let Some(source) = InputSource::parse(text) {
                return Ok(u16::from(source.code()));
            }
        }
        Feature::PowerMode => {
            if let Some(mode) = power_named(text) {
                return Ok(u16::from(mode.code()));
            }
        }
        Feature::Mute => match text {
            "mute" | "muted" | "on" => return Ok(0x01),
            "unmute" | "unmuted" | "off" => return Ok(0x02),
            _ => {}
        },
        _ => {}
    }

    if let Some(percent) = text.strip_suffix('%') {
        let percent: u8 = percent
            .trim()
            .parse()
            .map_err(|_| format!("{text:?} is not a percentage between 0 and 100"))?;
        let before = before.ok_or(
            "a percentage needs the monitor's own maximum, and it did not answer when it \
             was read — call read_display_settings, then pass a number instead",
        )?;
        return Ok(Value::from_percent(percent, before.maximum));
    }

    text.parse::<u16>()
        .map_err(|_| format!("{text:?} is not a value for {}", setting_name(feature)))
}

/// A power state by the names this tool advertises.
fn power_named(text: &str) -> Option<PowerMode> {
    Some(match text {
        "on" => PowerMode::On,
        "standby" => PowerMode::Standby,
        "suspend" => PowerMode::Suspend,
        "screen_off" => PowerMode::ActiveOff,
        "off" => PowerMode::Off,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
