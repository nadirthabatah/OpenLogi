//! Focusrite audio interfaces, over the Model Context Protocol.
//!
//! Gain and mute are ordinary settings and are offered plainly. Phantom
//! power is not, and the asymmetry is deliberate.
//!
//! # Why an assistant can switch 48 volts off but not on
//!
//! What phantom power can damage is at the far end of a cable no software
//! can see, so the decision needs a person who knows what is plugged in —
//! and an assistant passing a `confirm` flag is not that person, it is the
//! flag being passed. `roadie-scarlett`'s acknowledgement is built to make
//! that hard on purpose: it has to be constructed where the risk is in hand.
//!
//! So this surface takes the safe direction and refuses the risky one,
//! answering it with the sentence to say out loud and the exact command for
//! a person to type. That is not a smaller capability than the command line
//! has; it is the same gate, with the assistant on the correct side of it.

use roadie_focusrite::{Attached, Session, attached};
use roadie_scarlett::risk::Risk;
use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_audio_interfaces",
            "description": "Find every Focusrite Scarlett or Vocaster audio interface \
                plugged into this computer and report what each input is doing: preamp \
                gain, whether it is muted, and whether 48 volt phantom power is on. \
                Start here before changing anything. The audio itself is handled by the \
                operating system and is never interrupted by this.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "set_audio_input",
            "description": "Change the preamp gain or the mute switch on one input of a \
                Focusrite interface. Inputs are counted from one, the way they are \
                labelled on the box. Phantom power is not settable here; use \
                set_phantom_power, which explains why.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "interface": {
                        "type": "string",
                        "description": "Part of the interface's name or serial number, \
                            from list_audio_interfaces. Omit when only one is attached.",
                    },
                    "input": {"type": "integer", "minimum": 1},
                    "gain": {"type": "integer", "minimum": 0, "maximum": 255},
                    "muted": {"type": "boolean"},
                },
                "required": ["input"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "set_phantom_power",
            "description": "Switch 48 volt phantom power OFF on one input of a Focusrite \
                interface. Switching it ON is deliberately not available here: it can \
                damage a ribbon or vintage microphone, nothing in software can see what \
                is plugged in, so that decision belongs to the person at the desk. Asking \
                to switch it on returns the warning to read to them and the exact command \
                for them to type.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "interface": {"type": "string"},
                    "input": {"type": "integer", "minimum": 1},
                    "on": {
                        "type": "boolean",
                        "description": "Only false is carried out. True returns the \
                            warning and the command a person must type themselves.",
                    },
                },
                "required": ["input", "on"],
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Every interface and what it is doing.
pub fn list_audio_interfaces() -> Result<String, String> {
    let interfaces = usable()?;
    if interfaces.is_empty() {
        return Ok(
            "no Focusrite interface is plugged into this computer. It has to be \
                   connected with a cable that carries data, not only power."
                .to_owned(),
        );
    }
    let mut listed = Vec::new();
    for interface in &interfaces {
        let mut entry = json!({
            "name": interface.name,
            "serial_number": interface.serial_number,
        });
        match Session::open(interface).and_then(|mut session| session.snapshot()) {
            Ok(snapshot) => {
                let inputs: Vec<Value> = snapshot
                    .inputs
                    .iter()
                    .map(|input| {
                        json!({
                            "input": input.input,
                            "gain": input.gain,
                            "muted": input.muted,
                            "phantom_power": input.phantom,
                        })
                    })
                    .collect();
                if let Some(object) = entry.as_object_mut() {
                    object.insert("firmware".to_owned(), json!(snapshot.firmware));
                    object.insert("mass_storage_mode".to_owned(), json!(snapshot.msd_mode));
                    object.insert("inputs".to_owned(), Value::Array(inputs));
                }
            }
            // Reported rather than dropped: an interface that is present and
            // will not answer is a different thing to be told than one that
            // is not there, and the difference is what to do next.
            Err(error) => {
                if let Some(object) = entry.as_object_mut() {
                    object.insert("unavailable".to_owned(), json!(error.to_string()));
                }
            }
        }
        listed.push(entry);
    }
    rendered(&Value::Array(listed))
}

/// Change gain or mute on one input.
pub fn set_audio_input(arguments: &Value) -> Result<String, String> {
    let input = number(arguments, "input")?;
    let mut session = open(arguments)?;
    let mut changed = Vec::new();

    if let Some(gain) = arguments.get("gain") {
        let gain = gain
            .as_u64()
            .ok_or_else(|| format!("`gain` must be a number, not {gain}"))?;
        let gain = u8::try_from(gain).map_err(|_| format!("`gain` of {gain} is out of range"))?;
        session.set_gain(input, gain).map_err(|e| e.to_string())?;
        changed.push("gain");
    }
    if let Some(muted) = arguments.get("muted") {
        let muted = muted
            .as_bool()
            .ok_or_else(|| format!("`muted` must be true or false, not {muted}"))?;
        session.set_muted(input, muted).map_err(|e| e.to_string())?;
        changed.push("muted");
    }
    if changed.is_empty() {
        return Err("nothing would change: pass `gain`, `muted`, or both".to_owned());
    }

    // Read back rather than echo. A write that silently did not take leaves
    // someone speaking into a microphone that is doing the old thing while
    // the tool insists it changed.
    rendered(&json!({
        "input": input,
        "changed": changed,
        "gain": session.gain(input).ok(),
        "muted": session.muted(input).ok(),
    }))
}

/// Switch phantom power off, or explain why on is not available here.
pub fn set_phantom_power(arguments: &Value) -> Result<String, String> {
    let input = number(arguments, "input")?;
    let on = arguments
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(|| "`on` must be true or false".to_owned())?;

    if on {
        let risk = Risk::PhantomPower { pair: input };
        return rendered(&json!({
            "carried_out": false,
            "why": "switching 48 volt phantom power on is not available to an assistant, \
                    because what it can damage is at the end of a cable no software can see",
            "read_this_to_them": risk.spoken(),
            "command_for_them_to_type": format!("roadie audio phantom {input} on --yes"),
        }));
    }

    let mut session = open(arguments)?;
    session
        .set_phantom(input, false, None)
        .map_err(|e| e.to_string())?;
    rendered(&json!({
        "carried_out": true,
        "input": input,
        "phantom_power": session.phantom(input).ok(),
    }))
}

/// The interfaces this build can drive.
fn usable() -> Result<Vec<Attached>, String> {
    attached()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(interface) => Some(Ok(interface)),
            // A Focusrite with no table in this build is not a failure of the
            // call; it simply cannot be listed as drivable.
            Err(_) => None,
        })
        .collect()
}

/// A required whole number argument.
fn number(arguments: &Value, field: &str) -> Result<u16, String> {
    arguments
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| format!("`{field}` must be a whole number, counted from one"))
}

/// Open the interface an `interface` argument names.
///
/// Omitting it is allowed only when exactly one is attached. Picking the
/// first of several would change a different interface than the one meant.
fn open(arguments: &Value) -> Result<Session, String> {
    let interfaces = usable()?;
    let wanted = arguments.get("interface").and_then(Value::as_str);
    let chosen = match wanted {
        None => match interfaces.len() {
            1 => interfaces
                .into_iter()
                .next()
                .ok_or_else(|| "unreachable".to_owned())?,
            0 => return Err("no Focusrite interface is plugged into this computer".to_owned()),
            many => {
                return Err(format!(
                    "{many} interfaces are attached, so `interface` is required — call \
                     list_audio_interfaces and pass part of the name of the one you mean"
                ));
            }
        },
        Some(query) => {
            let query = query.to_lowercase();
            let mut matching: Vec<Attached> = interfaces
                .into_iter()
                .filter(|interface| {
                    interface.name.to_lowercase().contains(&query)
                        || interface
                            .serial_number
                            .as_deref()
                            .is_some_and(|serial| serial.to_lowercase().contains(&query))
                })
                .collect();
            match matching.len() {
                1 => matching.remove(0),
                0 => {
                    return Err(format!(
                        "no interface's name or serial number contains {query:?} — call \
                         list_audio_interfaces to see what is attached"
                    ));
                }
                many => {
                    return Err(format!(
                        "{query:?} matches {many} interfaces: {}. Use more of a name.",
                        matching
                            .iter()
                            .map(Attached::describe)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }
    };
    Session::open(&chosen).map_err(|error| error.to_string())
}
