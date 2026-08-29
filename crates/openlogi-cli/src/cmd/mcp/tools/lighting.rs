//! Device backlighting and standalone lights.

use std::str::FromStr as _;

use openlogi_core::color::Rgb;
use openlogi_core::config::Lighting;
use openlogi_core::hid::LightCommand;
use serde_json::{Value, json};
use tarpc::context;

use super::{agent, route_argument, route_schema, rpc};

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "set_lighting",
            "description": "Set the backlighting on a device that has it: master \
                on/off, colour, and brightness. Colour and brightness persist while \
                lighting is off, so turning it back on restores the previous look.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "route": route_schema(),
                    "enabled": {
                        "type": "boolean",
                        "description": "Master on/off for the device's lighting.",
                    },
                    "color": {
                        "type": "string",
                        "pattern": "^[0-9a-fA-F]{6}$",
                        "description": "Six hex digits, \"RRGGBB\", with no leading '#'.",
                    },
                    "brightness": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 100,
                        "description": "Brightness as a percentage.",
                    },
                },
                "required": ["route", "enabled", "color", "brightness"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "set_light",
            "description": "Control a standalone light such as a Litra: turn it on or \
                off, set its brightness, or set its colour temperature. Give exactly \
                one of `power`, `brightness_percent`, or `temperature_kelvin` per call.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "route": route_schema(),
                    "power": {
                        "type": "string",
                        "enum": ["on", "off"],
                        "description": "Turn the light on or off.",
                    },
                    "brightness_percent": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 100,
                        "description": "Brightness as a percentage.",
                    },
                    "temperature_kelvin": {
                        "type": "integer",
                        "minimum": 1000,
                        "maximum": 10000,
                        "description": "Colour temperature in Kelvin; warmer is lower.",
                    },
                },
                "required": ["route"],
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Apply a full lighting configuration to the routed device.
pub async fn set_lighting(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let enabled = arguments
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| "the `enabled` argument must be true or false".to_string())?;
    let color = arguments
        .get("color")
        .and_then(Value::as_str)
        .ok_or_else(|| "the `color` argument must be a \"RRGGBB\" string".to_string())?;
    let color = Rgb::from_str(color).map_err(|error| error.to_string())?;
    let brightness = percentage(arguments, "brightness")?
        .ok_or_else(|| "the `brightness` argument is required".to_string())?;

    let client = agent().await?;
    rpc(client.set_lighting(
        context::current(),
        route.clone(),
        Lighting {
            enabled,
            color,
            brightness,
        },
    ))
    .await?
    .map_err(|error| format!("setting lighting failed: {error}"))?;
    let (r, g, b) = color.components();
    Ok(format!(
        "lighting for {route} is now {}, colour {r:02x}{g:02x}{b:02x} at {brightness}%",
        if enabled { "on" } else { "off" }
    ))
}

/// Apply one command to a standalone light.
pub async fn set_light(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let (command, description) = light_command(arguments)?;
    let client = agent().await?;
    rpc(client.set_light(context::current(), route.clone(), command))
        .await?
        .map_err(|error| format!("setting the light failed: {error}"))?;
    Ok(format!("{description} for {route}"))
}

/// Pick the single light command the arguments describe.
///
/// Exactly one is required. Accepting several would leave the order they are
/// applied in — and therefore the light's final state — up to this function's
/// internals rather than the caller's intent.
fn light_command(arguments: &Value) -> Result<(LightCommand, String), String> {
    let mut chosen: Vec<(LightCommand, String)> = Vec::new();

    match arguments.get("power") {
        None | Some(Value::Null) => {}
        Some(Value::String(value)) if value == "on" => {
            chosen.push((LightCommand::Power(true), "light turned on".to_string()));
        }
        Some(Value::String(value)) if value == "off" => {
            chosen.push((LightCommand::Power(false), "light turned off".to_string()));
        }
        Some(other) => return Err(format!("`power` must be \"on\" or \"off\", not {other}")),
    }

    if let Some(percent) = percentage(arguments, "brightness_percent")? {
        chosen.push((
            LightCommand::BrightnessPercent(percent),
            format!("brightness set to {percent}%"),
        ));
    }

    match arguments.get("temperature_kelvin") {
        None | Some(Value::Null) => {}
        Some(Value::Number(number)) => {
            let kelvin = number
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .ok_or_else(|| format!("`temperature_kelvin` is out of range: {number}"))?;
            chosen.push((
                LightCommand::TemperatureKelvin(kelvin),
                format!("colour temperature set to {kelvin}K"),
            ));
        }
        Some(other) => {
            return Err(format!(
                "`temperature_kelvin` must be a number, not {other}"
            ));
        }
    }

    match chosen.len() {
        1 => Ok(chosen.remove(0)),
        0 => Err("give one of `power`, `brightness_percent`, or `temperature_kelvin`".to_string()),
        count => Err(format!(
            "give exactly one of `power`, `brightness_percent`, or `temperature_kelvin`; \
             {count} were given, and applying them in an arbitrary order would leave the \
             light in an unpredictable state"
        )),
    }
}

/// Read an optional 0..=100 percentage argument.
fn percentage(arguments: &Value, field: &str) -> Result<Option<u8>, String> {
    match arguments.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .filter(|percent| *percent <= 100)
            .map(Some)
            .ok_or_else(|| format!("`{field}` must be between 0 and 100, not {number}")),
        Some(other) => Err(format!("`{field}` must be a number, not {other}")),
    }
}

#[cfg(test)]
mod tests {
    use openlogi_core::hid::LightCommand;
    use serde_json::json;

    use super::{light_command, percentage};

    #[test]
    fn each_single_command_is_recognized() {
        let (command, _) = light_command(&json!({ "power": "on" })).expect("valid");
        assert_eq!(command, LightCommand::Power(true));

        let (command, _) = light_command(&json!({ "power": "off" })).expect("valid");
        assert_eq!(command, LightCommand::Power(false));

        let (command, _) = light_command(&json!({ "brightness_percent": 40 })).expect("valid");
        assert_eq!(command, LightCommand::BrightnessPercent(40));

        let (command, _) = light_command(&json!({ "temperature_kelvin": 5600 })).expect("valid");
        assert_eq!(command, LightCommand::TemperatureKelvin(5600));
    }

    #[test]
    fn no_command_at_all_is_refused() {
        light_command(&json!({})).expect_err("nothing to do");
    }

    #[test]
    fn several_commands_at_once_are_refused_rather_than_ordered_arbitrarily() {
        light_command(&json!({ "power": "on", "brightness_percent": 50 }))
            .expect_err("ambiguous ordering");
        light_command(&json!({ "brightness_percent": 50, "temperature_kelvin": 4000 }))
            .expect_err("ambiguous ordering");
    }

    #[test]
    fn a_malformed_power_value_is_refused() {
        light_command(&json!({ "power": "bright" })).expect_err("not a power value");
        light_command(&json!({ "power": true })).expect_err("not a string");
    }

    #[test]
    fn percentages_are_bounded_at_both_ends() {
        assert_eq!(percentage(&json!({ "b": 0 }), "b").expect("valid"), Some(0));
        assert_eq!(
            percentage(&json!({ "b": 100 }), "b").expect("valid"),
            Some(100)
        );
        assert_eq!(percentage(&json!({}), "b").expect("absent is valid"), None);
        percentage(&json!({ "b": 101 }), "b").expect_err("above full");
        percentage(&json!({ "b": -1 }), "b").expect_err("below empty");
        percentage(&json!({ "b": "half" }), "b").expect_err("not a number");
    }
}
