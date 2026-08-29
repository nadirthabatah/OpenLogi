//! Elgato Key Lights, over the network.
//!
//! Separate from `set_light` rather than folded into it, because the two are
//! addressed differently and pretending otherwise would make one tool with two
//! mutually exclusive halves. A Litra is a HID route through the agent; a Key
//! Light is a name on the local network, discovered afresh each time because
//! its address comes from a DHCP lease and changes on its own.
//!
//! Unlike `set_light`, this takes several changes at once. That is not a
//! looser rule but a truer one: a Key Light's whole state goes in one `PUT`,
//! so "turn it on at forty percent and warm" is a single request, and forcing
//! three calls would make three round trips and leave two intermediate states
//! visible on someone's face.

use std::time::Duration;

use roadie_keylight::state::{BRIGHTNESS, kelvin_range};
use roadie_keylight::{KeyLight, Light};
use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};

/// How long to spend looking. Matches `roadie light`'s own default.
const WAIT: Duration = Duration::from_secs(3);

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    let kelvin = kelvin_range();
    vec![
        json!({
            "name": "list_network_lights",
            "description": "Find every Elgato Key Light on the local network and report \
                what each one is doing. Start here before changing one: a light's address \
                comes from the router and changes on its own, so the list is looked up \
                fresh rather than remembered. Logitech Litra lights are on USB and appear \
                in list_peripherals instead.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "set_network_light",
            "description": format!(
                "Change an Elgato Key Light. Several changes can be given at once and are \
                 sent as one request, which is what the light itself expects. Brightness \
                 is a percentage from {} to {}; colour temperature is in kelvin from {} to \
                 {}, and values outside those are clamped rather than refused. The result \
                 says what the light actually did, which can differ from what was asked.",
                BRIGHTNESS.low, BRIGHTNESS.high, kelvin.low, kelvin.high
            ),
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "light": {
                        "type": "string",
                        "description": "Part of the light's name or address, from \
                            list_network_lights. Omit to use the only light when exactly \
                            one is found.",
                    },
                    "power": {
                        "type": "string",
                        "enum": ["on", "off"],
                    },
                    "brightness_percent": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 100,
                    },
                    "temperature_kelvin": {
                        "type": "integer",
                        "minimum": 2000,
                        "maximum": 9000,
                    },
                },
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Every Key Light on the network, with what it is doing.
pub fn list_network_lights() -> Result<String, String> {
    let lights = roadie_keylight::discover(WAIT).map_err(|error| error.to_string())?;
    if lights.is_empty() {
        return Ok(
            "no Elgato lights answered on this network. They have to be powered on \
             and on the same network as this computer; a Logitech Litra is on USB and \
             appears in list_peripherals instead."
                .to_string(),
        );
    }
    let listed: Vec<Value> = lights.iter().map(state_json).collect();
    rendered(&Value::Array(listed))
}

/// One light's entry, whether or not it answered.
///
/// A light that announced itself and then would not answer is reported rather
/// than dropped: it is nearly always one that went to sleep between the two,
/// and "it is there and asleep" is a different thing to be told than "it is
/// not there".
fn state_json(light: &KeyLight) -> Value {
    match light.read() {
        Ok(state) => json!({
            "name": light.name(),
            "address": light.address().to_string(),
            "on": state.is_on(),
            "brightness_percent": state.brightness,
            "temperature_kelvin": state.kelvin(),
        }),
        Err(error) => json!({
            "name": light.name(),
            "address": light.address().to_string(),
            "unavailable": error.to_string(),
        }),
    }
}

/// Change one light.
pub fn set_network_light(arguments: &Value) -> Result<String, String> {
    let lights = roadie_keylight::discover(WAIT).map_err(|error| error.to_string())?;
    let light = choose(&lights, arguments.get("light"))?;
    let before = light.read().map_err(|error| error.to_string())?;
    let after = change(before, arguments)?;
    ensure_changed(before, after)?;
    let result = light.write(after).map_err(|error| error.to_string())?;
    rendered(&json!({
        "name": light.name(),
        "on": result.is_on(),
        "brightness_percent": result.brightness,
        "temperature_kelvin": result.kelvin(),
        "note": (result != after).then(|| {
            "the light settled on different values than it was given, which it does \
             when a value is outside what it accepts"
                .to_owned()
        }),
    }))
}

/// The light a `light` argument names.
///
/// Omitting it is allowed only when exactly one light answered. Picking the
/// first of several would change a different light than the one meant, and on
/// a desk with a key and a fill light that is immediately visible to everyone
/// except the person who asked.
fn choose<'a>(lights: &'a [KeyLight], wanted: Option<&Value>) -> Result<&'a KeyLight, String> {
    let Some(wanted) = wanted else {
        return match lights {
            [only] => Ok(only),
            [] => Err("no Elgato lights answered on this network".to_string()),
            many => Err(format!(
                "{} lights answered, so `light` is required — call list_network_lights \
                 and pass the name of the one you mean",
                many.len()
            )),
        };
    };
    let wanted = wanted
        .as_str()
        .ok_or_else(|| format!("`light` must be a name or address string, not {wanted}"))?
        .to_lowercase();

    let matching: Vec<&KeyLight> = lights
        .iter()
        .filter(|light| {
            light.name().to_lowercase().contains(&wanted)
                || light.address().to_string().contains(&wanted)
        })
        .collect();
    match matching.as_slice() {
        [only] => Ok(only),
        [] => Err(format!(
            "no light's name or address contains {wanted:?} — call list_network_lights \
             to see what answered"
        )),
        many => Err(format!(
            "{wanted:?} matches {} lights: {}. Use more of a name.",
            many.len(),
            many.iter()
                .map(|light| light.name().to_owned())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Refuse a call that asks for nothing.
///
/// Its own function so it can be tested: reached only through
/// [`set_network_light`], it would otherwise need a light on a network to
/// exercise, and "reported success having changed nothing" is exactly the
/// failure nobody notices.
///
/// Equality is the test rather than "were any arguments given", because
/// setting a light to what it already is asks for nothing either — and saying
/// so is more useful than a cheerful report that nothing happened.
fn ensure_changed(before: Light, after: Light) -> Result<(), String> {
    if after == before {
        return Err("nothing would change: pass at least one of power, \
             brightness_percent or temperature_kelvin, with a value the light is not \
             already set to"
            .to_string());
    }
    Ok(())
}

/// `before`, with whatever the arguments ask for applied.
///
/// Clamping happens inside `roadie-keylight` rather than here, and it is not
/// silent by accident: the caller is told what the light settled on, which is
/// the honest report when a request was adjusted.
fn change(before: Light, arguments: &Value) -> Result<Light, String> {
    let mut light = before;
    if let Some(power) = arguments.get("power") {
        let power = power
            .as_str()
            .ok_or_else(|| format!("`power` must be \"on\" or \"off\", not {power}"))?;
        light = match power {
            "on" => light.set_on(true),
            "off" => light.set_on(false),
            other => return Err(format!("`power` must be \"on\" or \"off\", not {other:?}")),
        };
    }
    if let Some(percent) = arguments.get("brightness_percent") {
        let percent = percent
            .as_u64()
            .ok_or_else(|| format!("`brightness_percent` must be a number, not {percent}"))?;
        light = light.set_brightness(u16::try_from(percent).unwrap_or(u16::MAX));
    }
    if let Some(kelvin) = arguments.get("temperature_kelvin") {
        let kelvin = kelvin
            .as_u64()
            .ok_or_else(|| format!("`temperature_kelvin` must be a number, not {kelvin}"))?;
        light = light.set_kelvin(u16::try_from(kelvin).unwrap_or(u16::MAX));
    }
    Ok(light)
}

#[cfg(test)]
mod tests;
