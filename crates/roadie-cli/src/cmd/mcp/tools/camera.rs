//! Webcam controls, vendor-neutral.
//!
//! These reach the camera directly rather than through the agent, matching
//! what `roadie camera` already does: UVC controls are a class standard the
//! host exposes per camera, not agent-owned state. One consequence worth
//! knowing on macOS: the grant that matters is *this* process's, not the
//! agent's.
//!
//! Enumeration is deliberately not filtered by vendor. The same UVC registers
//! answer on an Elgato, an Obsbot, or a built-in camera, so filtering to one
//! manufacturer would hide devices that are in fact controllable.

use roadie_camera::{AutoToggle, CameraControl, ControlRange};
use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};

/// Every control name a caller may pass, auto toggles included.
fn control_names() -> Vec<String> {
    CameraControl::ALL
        .iter()
        .map(|control| control.name().to_string())
        .chain(
            AutoToggle::ALL
                .iter()
                .map(|toggle| toggle.name().to_string()),
        )
        .collect()
}

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    let camera_argument = json!({
        "type": "string",
        "description": "The camera's `id`, from list_cameras. Omit to use the \
            only camera when exactly one is attached.",
    });
    vec![
        json!({
            "name": "list_cameras",
            "description": "List every connected webcam, of any brand, with the id \
                the other camera tools take. Start here before changing a camera \
                setting.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "read_camera_controls",
            "description": "Read a webcam's adjustable controls — brightness, \
                contrast, exposure, focus, zoom, white balance and the rest — with \
                each one's current value and the range it accepts, plus the state \
                of its automatic modes.",
            "inputSchema": json!({
                "type": "object",
                "properties": { "camera": camera_argument },
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "set_camera_control",
            "description": "Set one webcam control. The change is written to the \
                camera itself, so every application sees it, and it survives after \
                this program exits. Read read_camera_controls first: values are \
                checked against the range that camera actually reports. Automatic \
                modes (focus_auto, exposure_auto, white_balance_auto) take 1 for on \
                and 0 for off, and a manual control that an automatic mode governs \
                is ignored by the camera until that mode is switched off.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "camera": camera_argument,
                    "control": {
                        "type": "string",
                        "enum": control_names(),
                        "description": "Which control to set.",
                    },
                    "value": {
                        "type": "integer",
                        "description": "The value to write; must be within the \
                            range read_camera_controls reports for this camera.",
                    },
                },
                "required": ["control", "value"],
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Every attached camera, whatever its vendor.
pub fn list_cameras() -> Result<String, String> {
    let cameras = roadie_camera::enumerate_all_cameras();
    if cameras.is_empty() {
        return Ok(
            "no cameras are attached, or this platform exposes no camera backend".to_string(),
        );
    }
    let listed: Vec<Value> = cameras
        .iter()
        .map(|camera| {
            json!({
                "id": camera.unique_id,
                "name": camera.name,
                "vendor_id": format!("{:04x}", camera.vendor_id),
                "product_id": format!("{:04x}", camera.product_id),
                "serial_number": camera.serial_number,
            })
        })
        .collect();
    rendered(&Value::Array(listed))
}

/// Resolve the `camera` argument to a camera id.
///
/// Omitting it is only allowed when exactly one camera is attached: silently
/// picking the first of several would change a different device than the one
/// meant, and the caller would have no way to notice.
fn camera_id(arguments: &Value) -> Result<String, String> {
    if let Some(id) = arguments.get("camera") {
        return id
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("`camera` must be a camera id string, not {id}"));
    }
    let cameras = roadie_camera::enumerate_all_cameras();
    match cameras.len() {
        1 => Ok(cameras[0].unique_id.clone()),
        0 => Err("no cameras are attached".to_string()),
        count => Err(format!(
            "{count} cameras are attached, so `camera` is required — call list_cameras \
             and pass the id of the one you mean"
        )),
    }
}

/// One camera's controls, ranges and automatic modes.
pub fn read_camera_controls(arguments: &Value) -> Result<String, String> {
    let id = camera_id(arguments)?;
    let state = roadie_camera::read_camera_state(&id)
        .map_err(|error| format!("could not read the controls for camera {id}: {error}"))?;
    if state.controls.is_empty() && state.autos.is_empty() {
        return Ok(format!(
            "camera {id} reports no adjustable controls. Some cameras expose none, \
             and on macOS this is also what a missing camera permission looks like."
        ));
    }
    let controls: Vec<Value> = state
        .controls
        .iter()
        .map(|(control, range)| {
            json!({
                "control": control.name(),
                "current": range.current,
                "min": range.min,
                "max": range.max,
                "default": range.default,
                "governed_by": control.auto_toggle().map(AutoToggle::name),
            })
        })
        .collect();
    let autos: Vec<Value> = state
        .autos
        .iter()
        .map(|(toggle, auto)| {
            json!({
                "control": toggle.name(),
                "current": i32::from(auto.current),
                "default": i32::from(auto.default),
            })
        })
        .collect();
    rendered(&json!({ "camera": id, "controls": controls, "auto_modes": autos }))
}

/// Write one control, after checking the value against what the camera says
/// it accepts.
pub fn set_camera_control(arguments: &Value) -> Result<String, String> {
    let id = camera_id(arguments)?;
    let name = arguments
        .get("control")
        .and_then(Value::as_str)
        .ok_or_else(|| "the `control` argument must name a control".to_string())?
        .to_ascii_lowercase();
    let value = arguments
        .get("value")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| "the `value` argument must be a whole number".to_string())?;

    let control = match resolve(&name)? {
        Target::Auto(toggle) => {
            roadie_camera::set_auto(&id, toggle, value != 0)
                .map_err(|error| format!("setting {name} failed: {error}"))?;
            return Ok(format!(
                "{name} is now {} on camera {id}",
                if value == 0 { "off" } else { "on" }
            ));
        }
        Target::Control(control) => control,
    };

    // Check against the camera's own reported range before writing. A device
    // handed an out-of-range value may clamp it, ignore it, or fail opaquely,
    // and none of those tell the caller what would have worked.
    let range = current_range(&id, control)?;
    if !range.supports(value) {
        return Err(format!(
            "{value} is not accepted for {name} on camera {id}: it takes {} to {} \
             (currently {}, default {})",
            range.min, range.max, range.current, range.default
        ));
    }

    roadie_camera::set_control(&id, control, value)
        .map_err(|error| format!("setting {name} failed: {error}"))?;
    let governed = control.auto_toggle().map_or(String::new(), |toggle| {
        format!(
            ". Note that {} governs this control — if it is on, the camera may \
             override this value",
            toggle.name()
        )
    });
    Ok(format!("{name} set to {value} on camera {id}{governed}"))
}

/// What a control name refers to.
///
/// The two are not interchangeable: an automatic mode is a boolean the camera
/// applies itself, a control is a value within a range, and they are written
/// through different calls. Naming the distinction keeps a caller from
/// silently taking the wrong branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// An automatic mode: focus, exposure or white balance.
    Auto(AutoToggle),
    /// A value control.
    Control(CameraControl),
}

/// Resolve a control name to what it addresses.
///
/// Automatic modes are checked first: their names end in `_auto` and so cannot
/// collide with a control's, but resolving them first makes that independent
/// of the naming convention holding.
fn resolve(name: &str) -> Result<Target, String> {
    if let Some(toggle) = AutoToggle::ALL.iter().find(|t| t.name() == name) {
        return Ok(Target::Auto(*toggle));
    }
    CameraControl::ALL
        .iter()
        .find(|c| c.name() == name)
        .map(|control| Target::Control(*control))
        .ok_or_else(|| {
            format!(
                "unknown control {name:?}; this camera's controls are listed by \
                 read_camera_controls"
            )
        })
}

/// The camera's reported range for one control.
fn current_range(id: &str, control: CameraControl) -> Result<ControlRange, String> {
    let state = roadie_camera::read_camera_state(id)
        .map_err(|error| format!("could not read camera {id} to check the value: {error}"))?;
    state
        .controls
        .iter()
        .find(|(candidate, _)| *candidate == control)
        .map(|(_, range)| *range)
        .ok_or_else(|| {
            format!(
                "camera {id} does not expose {}; read_camera_controls lists what it does",
                control.name()
            )
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{camera_id, control_names, tools};

    #[test]
    fn every_control_and_auto_toggle_is_offered() {
        let names = control_names();
        // 11 controls plus 3 automatic modes.
        assert_eq!(names.len(), 14);
        for expected in [
            "brightness",
            "contrast",
            "exposure",
            "focus",
            "zoom",
            "white_balance",
            "power_line_frequency",
            "focus_auto",
            "exposure_auto",
            "white_balance_auto",
        ] {
            assert!(names.contains(&expected.to_string()), "{expected} missing");
        }
    }

    #[test]
    fn the_set_schema_constrains_the_control_to_known_names() {
        let tools = tools();
        let set = tools
            .iter()
            .find(|tool| tool["name"] == "set_camera_control")
            .expect("set_camera_control is advertised");
        let allowed = set["inputSchema"]["properties"]["control"]["enum"]
            .as_array()
            .expect("the control argument is an enum");
        assert_eq!(allowed.len(), control_names().len());
    }

    #[test]
    fn every_advertised_name_resolves_to_something() {
        // The schema offers these names; each must reach a real target, or a
        // caller following the schema gets "unknown control".
        for name in control_names() {
            super::resolve(&name).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    #[test]
    fn automatic_modes_and_value_controls_resolve_to_different_targets() {
        assert!(matches!(
            super::resolve("focus_auto").expect("a known name"),
            super::Target::Auto(_)
        ));
        assert!(matches!(
            super::resolve("focus").expect("a known name"),
            super::Target::Control(_)
        ));
    }

    #[test]
    fn an_unknown_name_says_where_to_find_the_real_ones() {
        let error = super::resolve("no_such_control").expect_err("not a control");
        assert!(error.contains("read_camera_controls"));
    }

    #[test]
    fn a_non_string_camera_argument_is_refused() {
        // Exercised without hardware: the type check runs before any
        // enumeration, so this path is reachable on a machine with no camera.
        camera_id(&json!({ "camera": 7 })).expect_err("a camera id is a string");
    }
}
