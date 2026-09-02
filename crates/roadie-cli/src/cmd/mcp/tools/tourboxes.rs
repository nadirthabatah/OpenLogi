//! TourBox controllers, over their serial port.
//!
//! Reached directly rather than through the agent, for the same reason
//! Stream Decks are: the agent does not own TourBoxes yet, and routing
//! through it would mean inventing a wire contract before there is anything
//! to carry.
//!
//! `watch_tourbox` is the unusual one here. Most tools in this surface ask a
//! device a question and get an answer; this one waits for a person to do
//! something. It exists because a controller with seventeen unlabelled
//! controls is genuinely hard to learn by touch, and "press the one you mean
//! and I will tell you what it was" is the shortest path from a hand to a
//! name. It is bounded so it cannot hold the server open indefinitely.

use std::time::{Duration, Instant};

use roadie_tourbox::serial::{SerialError, TourBox, ports};

use super::no_arguments_schema;
use serde_json::{Value, json};

/// Default seconds to watch for.
const DEFAULT_SECONDS: u64 = 10;

/// Longest watch this tool will accept.
///
/// A bound rather than a preference: this call blocks the server for its
/// whole duration, so an unbounded one would be a way to wedge the session.
const MAX_SECONDS: u64 = 60;

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_tourboxes",
            "description": "Find every TourBox controller attached to this computer and \
                report which model it is, which serial port it is on, and how many \
                buttons and wheels it has. Start here before watching one. A TourBox is \
                not a HID device, so it does not appear in list_peripherals. Only the \
                TourBox Elite can be recognised automatically; a Neo connects through a \
                general-purpose serial adapter that cannot be told apart from any other \
                device using the same chip.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "watch_tourbox",
            "description": format!(
                "Wait for someone to press buttons and turn wheels on a TourBox, then \
                 report what each one was, in order. Use this to help someone identify a \
                 control by touch, or to check that a controller is working. This blocks \
                 until the person stops for a moment or the time runs out, so tell them \
                 to start pressing before calling it. Waits {DEFAULT_SECONDS} seconds by \
                 default and at most {MAX_SECONDS}."
            ),
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_SECONDS,
                        "description": format!(
                            "How long to wait with nothing pressed before giving up. \
                             Defaults to {DEFAULT_SECONDS}."
                        ),
                    },
                    "port": {
                        "type": "string",
                        "description": "The serial port to open, for a controller that \
                            list_tourboxes cannot recognise on its own. Omit to use the \
                            one that was found.",
                    },
                },
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Every TourBox attached.
///
/// # Errors
///
/// When the host's serial ports cannot be listed at all.
pub fn list_tourboxes() -> Result<String, String> {
    let found = ports().map_err(|error| error.to_string())?;
    if found.is_empty() {
        return Ok(
            "No TourBox is attached. The most common cause is a charge-only \
                   USB-C cable, which carries power but no data, so the controller \
                   lights up and never appears to this computer. A TourBox Neo also \
                   will not appear here even when it is working, because it cannot be \
                   recognised by its USB identity; its port has to be named."
                .to_owned(),
        );
    }

    let lines: Vec<String> = found
        .iter()
        .map(|port| {
            let model = port.model;
            format!(
                "{}: {} buttons, {} wheels, haptics {}.",
                port.describe(),
                model.buttons.len(),
                model.wheels.len(),
                if model.haptics {
                    "supported"
                } else {
                    "not supported"
                },
            )
        })
        .collect();
    Ok(lines.join("\n"))
}

/// Watch one controller and name what was pressed.
///
/// # Errors
///
/// When no controller is attached, when the named port will not open, or
/// when the read fails partway through.
pub fn watch_tourbox(arguments: &Value) -> Result<String, String> {
    let seconds = arguments
        .get("seconds")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_SECONDS)
        .clamp(1, MAX_SECONDS);

    let mut tourbox = if let Some(path) = arguments.get("port").and_then(Value::as_str) {
        TourBox::open_path(path, None).map_err(|error| error.to_string())?
    } else {
        let found = ports().map_err(|error| error.to_string())?;
        let port = found.first().ok_or_else(|| {
            "No TourBox is attached, so there is nothing to watch. Call \
             list_tourboxes for what to check."
                .to_owned()
        })?;
        TourBox::open(port).map_err(|error| {
            format!(
                "{error}. The usual cause is another program holding the port; \
                 TourBox Console is the likely one."
            )
        })?
    };

    let quiet = Duration::from_secs(seconds);
    let mut last = Instant::now();
    let mut seen = Vec::new();
    let mut unrecognised = Vec::new();
    while last.elapsed() < quiet {
        match tourbox.read_event() {
            Ok(Some(event)) => {
                seen.push(event.describe());
                last = Instant::now();
            }
            Ok(None) => {}
            // Reported rather than fatal: the encoding has no framing, so
            // the next byte is a fresh event. A model with a control this
            // build has never met would show up exactly here.
            Err(SerialError::Protocol { source, .. }) => {
                unrecognised.push(source.to_string());
                last = Instant::now();
            }
            Err(error) => return Err(error.to_string()),
        }
    }

    if seen.is_empty() && unrecognised.is_empty() {
        return Ok(format!(
            "Nothing was pressed in {seconds} seconds. The port opened, so the \
             controller is there and could be read. Either nothing was touched, or \
             another program is holding the controller's input; TourBox Console is \
             the likely one."
        ));
    }

    let mut lines = vec![format!("{} events, in order:", seen.len())];
    lines.extend(seen.iter().map(|event| format!("  {event}")));
    if !unrecognised.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "{} bytes this build could not explain, which is what a control from a \
             model it has never met looks like:",
            unrecognised.len()
        ));
        lines.extend(unrecognised.iter().map(|problem| format!("  {problem}")));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every schema in this surface has to be an object and has to close
    /// itself to unknown properties; the registry test checks it globally,
    /// and this checks it where it is written.
    #[test]
    fn both_schemas_are_closed_objects() {
        for tool in tools() {
            let schema = &tool["inputSchema"];
            assert_eq!(schema["type"], "object", "{}", tool["name"]);
            assert_eq!(
                schema["additionalProperties"], false,
                "{} lets unknown properties through",
                tool["name"]
            );
        }
    }

    /// A caller asking for an hour gets a minute, not an hour. The clamp is
    /// what stops this tool from wedging the server.
    #[test]
    fn an_unreasonable_watch_is_clamped_rather_than_refused() {
        let asked = json!({ "seconds": 3600 });
        let seconds = asked
            .get("seconds")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_SECONDS)
            .clamp(1, MAX_SECONDS);
        assert_eq!(seconds, MAX_SECONDS);
    }

    /// Zero would be a watch that ends before it starts, and would report
    /// "nothing was pressed" about a controller nobody had a chance to
    /// touch.
    #[test]
    fn a_watch_of_no_time_at_all_becomes_the_shortest_real_one() {
        let asked = json!({ "seconds": 0 });
        let seconds = asked
            .get("seconds")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_SECONDS)
            .clamp(1, MAX_SECONDS);
        assert_eq!(seconds, 1);
    }

    /// The advice for an empty list has to name the cable, because that is
    /// the cause that presents as a controller which does not exist.
    #[test]
    fn an_empty_list_names_the_cable() {
        let Ok(report) = list_tourboxes() else {
            return;
        };
        if report.starts_with("No TourBox") {
            assert!(report.contains("cable"), "{report}");
        }
    }
}
