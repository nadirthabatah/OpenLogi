//! Pointer resolution and scroll-wheel behavior.

use openlogi_core::hid::{
    Dpi, SmartShiftAutoDisengage, SmartShiftMode, SmartShiftStatus, SmartShiftThreshold,
};
use serde_json::{Value, json};
use tarpc::context;

use super::{agent, rendered, route_argument, route_only_schema, route_schema, rpc};

/// The two wheel modes, as the strings the schema advertises.
const FREE: &str = "free";
/// See [`FREE`].
const RATCHET: &str = "ratchet";

/// The `auto_disengage` value that means "never auto-release the ratchet".
const PERMANENT: &str = "permanent";

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "read_dpi",
            "description": "Read a mouse's current pointer resolution (DPI) and the \
                values its sensor supports.",
            "inputSchema": route_only_schema(),
        }),
        json!({
            "name": "set_dpi",
            "description": "Set a mouse's pointer resolution (DPI). Read read_dpi first \
                when unsure which values the sensor supports.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "route": route_schema(),
                    "dpi": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 65535,
                        "description": "The DPI value to apply, e.g. 800 or 1600.",
                    },
                },
                "required": ["route", "dpi"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "read_smartshift",
            "description": "Read a mouse's scroll-wheel mode: whether the wheel is \
                ratcheted (clicky) or free-spinning, and the speed at which it \
                switches over.",
            "inputSchema": route_only_schema(),
        }),
        json!({
            "name": "set_smartshift",
            "description": "Set a mouse's scroll-wheel behavior. `mode` picks the \
                resting mode; `auto_disengage` sets how fast the wheel must spin \
                before the ratchet releases, or \"permanent\" to keep it ratcheted at \
                any speed. Omitted fields keep their current value, and the wheel's \
                tunable torque is always preserved.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "route": route_schema(),
                    "mode": {
                        "type": "string",
                        "enum": [FREE, RATCHET],
                        "description": "\"ratchet\" is the clicky, notched mode; \
                            \"free\" spins freely.",
                    },
                    "auto_disengage": {
                        "description": "A speed threshold from 1 (releases almost \
                            immediately) to 254 (releases only when spun hard), or the \
                            string \"permanent\" to never auto-release.",
                        "anyOf": [
                            { "type": "integer", "minimum": 1, "maximum": 254 },
                            { "type": "string", "enum": [PERMANENT] },
                        ],
                    },
                },
                "required": ["route"],
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Current and supported DPI for the routed device.
pub async fn read_dpi(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let client = agent().await?;
    let info = rpc(client.read_dpi(context::current(), route))
        .await?
        .map_err(|error| format!("reading DPI failed: {error}"))?;
    rendered(&serde_json::to_value(info).map_err(|error| format!("failed to encode DPI: {error}"))?)
}

/// Apply a DPI value to the routed device.
pub async fn set_dpi(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let requested = arguments
        .get("dpi")
        .and_then(Value::as_u64)
        .ok_or_else(|| "the `dpi` argument must be a positive integer".to_string())?;
    let dpi = u32::try_from(requested)
        .ok()
        .and_then(|value| Dpi::try_from(value).ok())
        .ok_or_else(|| format!("{requested} is not a representable DPI value"))?;
    let client = agent().await?;
    rpc(client.set_dpi(context::current(), route.clone(), dpi))
        .await?
        .map_err(|error| format!("setting DPI failed: {error}"))?;
    Ok(format!("DPI set to {dpi} for {route}"))
}

/// The routed device's current wheel configuration.
pub async fn read_smartshift(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let client = agent().await?;
    let status = rpc(client.read_smartshift(context::current(), route))
        .await?
        .map_err(|error| format!("reading the wheel configuration failed: {error}"))?;
    rendered(&describe_smartshift(status))
}

/// Change the routed device's wheel configuration, preserving what was not
/// asked about.
///
/// The current status is read first and used as the base. That is not an
/// optimization: the wire type carries the wheel's tunable torque, and
/// sending a fresh value for it would change the wheel's physical resistance
/// as a side effect of adjusting the mode.
pub async fn set_smartshift(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let mode = parse_mode(arguments)?;
    let auto_disengage = parse_auto_disengage(arguments)?;
    if mode.is_none() && auto_disengage.is_none() {
        return Err("nothing to change: pass `mode`, `auto_disengage`, or both".to_string());
    }

    let client = agent().await?;
    let current = rpc(client.read_smartshift(context::current(), route.clone()))
        .await?
        .map_err(|error| {
            format!("could not read the current wheel configuration to build on: {error}")
        })?;
    let status = SmartShiftStatus {
        mode: mode.unwrap_or(current.mode),
        auto_disengage: auto_disengage.unwrap_or(current.auto_disengage),
        tunable_torque: current.tunable_torque,
    };
    rpc(client.set_smartshift(context::current(), route.clone(), status))
        .await?
        .map_err(|error| format!("setting the wheel configuration failed: {error}"))?;
    Ok(format!(
        "wheel configuration for {route} is now {}",
        summarize_smartshift(status)
    ))
}

/// Read the optional `mode` argument.
fn parse_mode(arguments: &Value) -> Result<Option<SmartShiftMode>, String> {
    match arguments.get("mode") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(mode)) if mode == FREE => Ok(Some(SmartShiftMode::Free)),
        Some(Value::String(mode)) if mode == RATCHET => Ok(Some(SmartShiftMode::Ratchet)),
        Some(other) => Err(format!(
            "`mode` must be \"{FREE}\" or \"{RATCHET}\", not {other}"
        )),
    }
}

/// Read the optional `auto_disengage` argument, which is either a threshold
/// or the permanent-ratchet sentinel.
fn parse_auto_disengage(arguments: &Value) -> Result<Option<SmartShiftAutoDisengage>, String> {
    match arguments.get("auto_disengage") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value == PERMANENT => {
            Ok(Some(SmartShiftAutoDisengage::Permanent))
        }
        Some(Value::Number(number)) => {
            let threshold = number
                .as_u64()
                .and_then(|value| u8::try_from(value).ok())
                .and_then(|value| SmartShiftThreshold::try_new(value).ok())
                .ok_or_else(|| {
                    format!("`auto_disengage` must be between 1 and 254, not {number}")
                })?;
            Ok(Some(SmartShiftAutoDisengage::Threshold(threshold)))
        }
        Some(other) => Err(format!(
            "`auto_disengage` must be a number from 1 to 254 or \"{PERMANENT}\", not {other}"
        )),
    }
}

/// Render a wheel configuration as JSON the model can read without knowing
/// the firmware's encoding.
fn describe_smartshift(status: SmartShiftStatus) -> Value {
    json!({
        "mode": mode_name(status.mode),
        "auto_disengage": match status.auto_disengage {
            SmartShiftAutoDisengage::Permanent => Value::String(PERMANENT.to_string()),
            SmartShiftAutoDisengage::Threshold(threshold) => {
                Value::from(u8::from(threshold))
            }
        },
        "summary": summarize_smartshift(status),
    })
}

/// One sentence describing a wheel configuration, for a spoken answer.
fn summarize_smartshift(status: SmartShiftStatus) -> String {
    match status.auto_disengage {
        SmartShiftAutoDisengage::Permanent => {
            format!(
                "{} mode, staying ratcheted at any speed",
                mode_name(status.mode)
            )
        }
        SmartShiftAutoDisengage::Threshold(threshold) => format!(
            "{} mode, releasing the ratchet at speed threshold {}",
            mode_name(status.mode),
            u8::from(threshold)
        ),
    }
}

/// The schema-facing name of a wheel mode.
fn mode_name(mode: SmartShiftMode) -> &'static str {
    match mode {
        SmartShiftMode::Free => FREE,
        SmartShiftMode::Ratchet => RATCHET,
    }
}

#[cfg(test)]
mod tests {
    use openlogi_core::hid::{SmartShiftAutoDisengage, SmartShiftMode};
    use serde_json::json;

    use super::{parse_auto_disengage, parse_mode};

    #[test]
    fn an_absent_mode_leaves_the_current_one_alone() {
        assert!(parse_mode(&json!({})).expect("absent is valid").is_none());
        assert!(
            parse_mode(&json!({ "mode": null }))
                .expect("null is valid")
                .is_none()
        );
    }

    #[test]
    fn both_wheel_modes_parse() {
        assert_eq!(
            parse_mode(&json!({ "mode": "free" })).expect("valid"),
            Some(SmartShiftMode::Free)
        );
        assert_eq!(
            parse_mode(&json!({ "mode": "ratchet" })).expect("valid"),
            Some(SmartShiftMode::Ratchet)
        );
    }

    #[test]
    fn an_unknown_mode_is_rejected_rather_than_defaulted() {
        parse_mode(&json!({ "mode": "clicky" })).expect_err("not a mode");
        parse_mode(&json!({ "mode": 2 })).expect_err("not a string");
    }

    #[test]
    fn permanent_and_thresholds_both_parse() {
        assert_eq!(
            parse_auto_disengage(&json!({ "auto_disengage": "permanent" })).expect("valid"),
            Some(SmartShiftAutoDisengage::Permanent)
        );
        let parsed = parse_auto_disengage(&json!({ "auto_disengage": 40 })).expect("valid");
        let Some(SmartShiftAutoDisengage::Threshold(threshold)) = parsed else {
            panic!("expected a threshold");
        };
        assert_eq!(u8::from(threshold), 40);
    }

    #[test]
    fn thresholds_outside_the_firmware_range_are_rejected() {
        // 0 and 255 are the firmware's own sentinels and must not be
        // reachable through this tool.
        parse_auto_disengage(&json!({ "auto_disengage": 0 })).expect_err("0 is a sentinel");
        parse_auto_disengage(&json!({ "auto_disengage": 255 })).expect_err("255 is a sentinel");
        parse_auto_disengage(&json!({ "auto_disengage": 9000 })).expect_err("out of range");
        parse_auto_disengage(&json!({ "auto_disengage": "fast" })).expect_err("not a number");
    }

    #[test]
    fn the_boundary_thresholds_are_accepted() {
        for value in [1, 254] {
            let parsed =
                parse_auto_disengage(&json!({ "auto_disengage": value })).expect("in range");
            let Some(SmartShiftAutoDisengage::Threshold(threshold)) = parsed else {
                panic!("expected a threshold");
            };
            assert_eq!(u8::from(threshold), value);
        }
    }
}
