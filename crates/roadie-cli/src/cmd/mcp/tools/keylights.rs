//! Elgato lights: Key Lights on the network, and the Key Light Neo on USB.
//!
//! Separate from `set_light` rather than folded into it, because the two are
//! addressed differently and pretending otherwise would make one tool with two
//! mutually exclusive halves. A Litra is a HID route through the agent; an
//! Elgato light is a name — on the local network, discovered afresh each time
//! because its address comes from a DHCP lease and changes on its own, or on
//! USB, where the Key Light Neo speaks the same protocol in HID framing.
//!
//! The tool names still say `network` although the Neo on USB answers them
//! too. The names are what assistants and saved workflows already call, and
//! a list that quietly covers more is a better change than a rename that
//! breaks every caller — the descriptions say what is actually searched.
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
use crate::cmd::light::neo;

/// How long to spend looking. Matches `roadie light`'s own default.
const WAIT: Duration = Duration::from_secs(3);

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    let kelvin = kelvin_range();
    vec![
        json!({
            "name": "list_network_lights",
            "description": "Find every Elgato light — Key Lights on the local network, \
                and a Key Light Neo plugged in over USB — and report what each one is \
                doing. Start here before changing one: a network light's address comes \
                from the router and changes on its own, so the list is looked up fresh \
                rather than remembered. Logitech Litra lights appear in list_peripherals \
                instead.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "set_network_light",
            "description": format!(
                "Change an Elgato light, on the network or on USB. Several changes can \
                 be given at once and are sent as one request, which is what the light \
                 itself expects. Brightness is a percentage from {} to {}; colour \
                 temperature is in kelvin from {} to {}, and values outside those are \
                 clamped rather than refused — except that a Key Light Neo on USB power \
                 refuses brightness above the ceiling its power source allows, and the \
                 error then names that ceiling. The result says what the light actually \
                 did, which can differ from what was asked.",
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

/// Every Elgato light — on the network and on USB — with what it is doing.
pub async fn list_network_lights() -> Result<String, String> {
    let lights = roadie_keylight::discover(WAIT).map_err(|error| error.to_string())?;
    let usb = neo::find().await;
    if lights.is_empty() && usb.is_empty() {
        return Ok(
            "no Elgato lights answered, on this network or on USB. A network light has \
             to be powered on and on the same network as this computer; a Key Light Neo \
             has to be plugged in over a data cable. A Logitech Litra appears in \
             list_peripherals instead."
                .to_string(),
        );
    }
    let listed: Vec<Value> = usb
        .iter()
        .map(usb_state_json)
        .chain(lights.iter().map(state_json))
        .collect();
    rendered(&Value::Array(listed))
}

/// One USB light's entry, whether or not it answered.
///
/// The transport is stated because the same light on Wi-Fi and USB appears
/// once per path, and two identical entries would leave no way to say which
/// one a `set` should go through.
fn usb_state_json(found: &neo::Found) -> Value {
    match &found.state {
        Ok(state) => json!({
            "name": found.name(),
            "transport": "usb",
            "serial_number": found.serial_number,
            "on": state.is_on(),
            "brightness_percent": state.brightness,
            "temperature_kelvin": state.kelvin(),
        }),
        Err(error) => json!({
            "name": found.name(),
            "transport": "usb",
            "serial_number": found.serial_number,
            "unavailable": error,
        }),
    }
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
            "transport": "network",
            "address": light.address().to_string(),
            "on": state.is_on(),
            "brightness_percent": state.brightness,
            "temperature_kelvin": state.kelvin(),
        }),
        Err(error) => json!({
            "name": light.name(),
            "transport": "network",
            "address": light.address().to_string(),
            "unavailable": error.to_string(),
        }),
    }
}

/// Change one light, wherever it is.
pub async fn set_network_light(arguments: &Value) -> Result<String, String> {
    let lights = roadie_keylight::discover(WAIT).map_err(|error| error.to_string())?;
    let usb = neo::find().await;
    match choose(&lights, &usb, arguments.get("light"))? {
        Chosen::Network(light) => {
            let before = light.read().map_err(|error| error.to_string())?;
            let after = change(before, arguments)?;
            ensure_changed(before, after)?;
            let result = light.write(after).map_err(|error| error.to_string())?;
            settled(light.name(), "network", result, after)
        }
        Chosen::Usb(found) => {
            let before = *found
                .state
                .as_ref()
                .map_err(|why| format!("{} did not answer: {why}", found.name()))?;
            let after = change(before, arguments)?;
            ensure_changed(before, after)?;
            let result = neo::apply(found, |_| after)
                .await
                .map_err(|error| error.to_string())?;
            settled(found.name(), "usb", result, after)
        }
    }
}

/// The report of what a light settled on, for either transport.
fn settled(name: &str, transport: &str, result: Light, asked: Light) -> Result<String, String> {
    rendered(&json!({
        "name": name,
        "transport": transport,
        "on": result.is_on(),
        "brightness_percent": result.brightness,
        "temperature_kelvin": result.kelvin(),
        "note": (result != asked).then(|| {
            "the light settled on different values than it was given, which it does \
             when a value is outside what it accepts"
                .to_owned()
        }),
    }))
}

/// One selected light, on whichever path it answers.
#[derive(Debug)]
enum Chosen<'a> {
    /// A light on the network.
    Network(&'a KeyLight),
    /// A Key Light Neo on USB.
    Usb(&'a neo::Found),
}

impl Chosen<'_> {
    /// The name the light goes by, for sentences and for tests.
    fn name(&self) -> &str {
        match self {
            Self::Network(light) => light.name(),
            Self::Usb(found) => found.name(),
        }
    }
}

/// The light a `light` argument names.
///
/// Omitting it is allowed only when exactly one light answered. Picking the
/// first of several would change a different light than the one meant, and on
/// a desk with a key and a fill light that is immediately visible to everyone
/// except the person who asked.
fn choose<'a>(
    lights: &'a [KeyLight],
    usb: &'a [neo::Found],
    wanted: Option<&Value>,
) -> Result<Chosen<'a>, String> {
    let all: Vec<Chosen<'a>> = lights
        .iter()
        .map(Chosen::Network)
        .chain(usb.iter().map(Chosen::Usb))
        .collect();
    let Some(wanted) = wanted else {
        return match all.len() {
            1 => Ok(all
                .into_iter()
                .next()
                .unwrap_or_else(|| unreachable!("length was checked"))),
            0 => Err("no Elgato lights answered, on this network or on USB".to_string()),
            many => Err(format!(
                "{many} lights answered, so `light` is required — call \
                 list_network_lights and pass the name of the one you mean"
            )),
        };
    };
    let wanted = wanted
        .as_str()
        .ok_or_else(|| format!("`light` must be a name, address or serial string, not {wanted}"))?
        .to_lowercase();

    let matching: Vec<Chosen<'a>> = all
        .into_iter()
        .filter(|candidate| match candidate {
            Chosen::Network(light) => {
                light.name().to_lowercase().contains(&wanted)
                    || light.address().to_string().contains(&wanted)
            }
            Chosen::Usb(found) => {
                found.name().to_lowercase().contains(&wanted)
                    || found
                        .serial_number
                        .as_deref()
                        .is_some_and(|serial| serial.to_lowercase().contains(&wanted))
            }
        })
        .collect();
    let names = |candidates: &[Chosen<'a>]| {
        candidates
            .iter()
            .map(|candidate| {
                let transport = match candidate {
                    Chosen::Network(_) => "on the network",
                    Chosen::Usb(_) => "on USB",
                };
                format!("{} {transport}", candidate.name())
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    match matching.len() {
        1 => Ok(matching
            .into_iter()
            .next()
            .unwrap_or_else(|| unreachable!("length was checked"))),
        0 => Err(format!(
            "no light's name, address or serial contains {wanted:?} — call \
             list_network_lights to see what answered"
        )),
        many => Err(format!(
            "{wanted:?} matches {many} lights: {}. Use more of a name.",
            names(&matching)
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
