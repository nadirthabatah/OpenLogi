//! QMK keyboards and macro pads, over MCP.
//!
//! Reads a keymap and changes one key. Reaches hardware directly, like the
//! camera and Stream Deck tools, because the agent does not own VIA devices.
//!
//! Keys go out and come back by *name* — `F13`, not `0x0068`. That is not
//! decoration: an assistant relaying "your key is zero x zero zero six eight"
//! has relayed nothing usable, and one asked to "make this key F13" should not
//! have to know the number to do it.
//!
//! Writing is the careful half. A wrong keycode takes a key away from whoever
//! is using the board, and the assistant that did it is then the assistant
//! they have to ask to fix it — so an unknown protocol revision is refused
//! before anything is written, every write is read back and confirmed by
//! `openlogi_hid::via`, and the result of a change always names the key that
//! was there before, so the model can offer to put it back.

use openlogi_hid::via::{self, Attached, Session};
use openlogi_via::keycode;
use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};

/// The `layer`, `row` and `column` a key is addressed by.
fn position_properties() -> Value {
    json!({
        "layer": {
            "type": "integer",
            "minimum": 0,
            "description": "Keymap layer, counting from 0. list_keyboards gives how \
                many the board has.",
        },
        "row": {
            "type": "integer",
            "minimum": 0,
            "description": "Matrix row, counting from 0. This is the wiring position, \
                not a visual row; read_keymap gives the positions that exist.",
        },
        "column": {
            "type": "integer",
            "minimum": 0,
            "description": "Matrix column, counting from 0.",
        },
    })
}

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_keyboards",
            "description": "List attached QMK keyboards and macro pads with VIA \
                enabled, and how many keymap layers each holds. Start here before \
                reading or changing a key.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "read_keymap",
            "description": "Read one keymap layer and report what each assigned key \
                sends, by name. Unassigned and pass-through positions are omitted and \
                counted — on any layer above the first they are nearly the whole \
                matrix.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "layer": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Which layer, counting from 0.",
                    },
                    "rows": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "How many matrix rows to read. Defaults to 6. \
                            Raise it if a key you can see is missing from the result.",
                    },
                    "columns": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "How many matrix columns to read. Defaults to 16.",
                    },
                },
                "required": ["layer"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "set_key",
            "description": "Change what one key sends. The key is given by name — \
                \"F13\", \"COPY\", \"LEFT_CTRL\" — and the result reports what was \
                there before, so the change can be offered back. The write is read \
                back and confirmed; a board speaking a VIA revision this build does \
                not implement is refused rather than written to.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "layer": position_properties()["layer"].clone(),
                    "row": position_properties()["row"].clone(),
                    "column": position_properties()["column"].clone(),
                    "key": {
                        "type": "string",
                        "description": "The key to assign, by name: \"F13\", \"KC_F13\" \
                            and \"f13\" all work. A hex or decimal keycode is accepted \
                            too, but a name is what a person can check.",
                    },
                },
                "required": ["layer", "row", "column", "key"],
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Run `list_keyboards`.
pub async fn list_keyboards() -> Result<String, String> {
    let attached = enumerate().await?;
    let mut boards = Vec::new();
    for device in &attached {
        let mut entry = serde_json::Map::new();
        entry.insert("name".to_owned(), json!(device.name));
        entry.insert(
            "vendor_id".to_owned(),
            json!(format!("{:04x}", device.vendor_id)),
        );
        entry.insert(
            "product_id".to_owned(),
            json!(format!("{:04x}", device.product_id)),
        );
        // Opening is what turns a usage-page match into a fact. Reported
        // rather than fatal: one unresponsive board must not hide the rest.
        match Session::open(device).await {
            Ok(session) => {
                entry.insert("via_protocol".to_owned(), json!(session.protocol()));
                entry.insert("layers".to_owned(), json!(session.layers()));
            }
            Err(error) => {
                entry.insert("usable".to_owned(), json!(false));
                entry.insert(
                    "note".to_owned(),
                    json!(format!(
                        "did not answer as a VIA device ({error}). It may be a device \
                         that uses the same HID collection for something else."
                    )),
                );
            }
        }
        boards.push(Value::Object(entry));
    }
    rendered(&json!({ "keyboards": boards }))
}

/// Run `read_keymap`.
pub async fn read_keymap(arguments: &Value) -> Result<String, String> {
    let layer = number(arguments, "layer")?;
    let rows = optional_number(arguments, "rows")?.unwrap_or(6);
    let columns = optional_number(arguments, "columns")?.unwrap_or(16);

    let attached = enumerate().await?;
    let mut session = open_first(&attached).await?;
    let mut keys = Vec::new();
    let mut quiet = 0_u32;
    for row in 0..rows {
        for column in 0..columns {
            let code = session
                .keycode(layer, row, column)
                .await
                .map_err(|error| format!("{error}"))?;
            if code == keycode::NONE || code == keycode::TRANSPARENT {
                quiet += 1;
                continue;
            }
            keys.push(json!({
                "row": row,
                "column": column,
                "key": keycode::describe(code),
            }));
        }
    }
    rendered(&json!({
        "layer": layer,
        "layers": session.layers(),
        "keys": keys,
        "unassigned_or_passthrough": quiet,
        "read_area": { "rows": rows, "columns": columns },
    }))
}

/// Run `set_key`.
pub async fn set_key(arguments: &Value) -> Result<String, String> {
    let layer = number(arguments, "layer")?;
    let row = number(arguments, "row")?;
    let column = number(arguments, "column")?;
    let Some(wanted) = arguments.get("key").and_then(Value::as_str) else {
        return Err("the `key` argument is missing; give a key name such as \"F13\"".to_owned());
    };
    // Resolved before a device is opened. The name is wrong whether or not a
    // keyboard is attached, and "no VIA device found" would send the model —
    // and the person — hunting the wrong problem.
    let keycode_value = resolve(wanted)?;

    let attached = enumerate().await?;
    let mut session = open_first(&attached).await?;
    let was = session
        .keycode(layer, row, column)
        .await
        .map_err(|error| format!("{error}"))?;
    session
        .set_keycode(layer, row, column, keycode_value)
        .await
        .map_err(|error| format!("{error}"))?;

    rendered(&json!({
        "layer": layer,
        "row": row,
        "column": column,
        "was": keycode::describe(was),
        "now": keycode::describe(keycode_value),
        "confirmed": "the position was read back and holds the new key",
        "to_undo": format!(
            "set_key with layer {layer}, row {row}, column {column}, key \"{}\"",
            keycode::describe(was)
        ),
    }))
}

/// Turn a key argument into a keycode.
fn resolve(argument: &str) -> Result<u16, String> {
    let text = argument.trim();
    if let Some(found) = keycode::parse(text) {
        return Ok(found);
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u16::from_str_radix(hex, 16).map_err(|_| unknown_key(argument));
    }
    text.parse::<u16>().map_err(|_| unknown_key(argument))
}

/// Say a key name was not recognised, in a way the model can act on.
fn unknown_key(argument: &str) -> String {
    format!(
        "\"{argument}\" is not a key name this build knows, nor a number. Names are the \
         USB HID keyboard ones: letters, digits, F1 to F24, ENTER, ESCAPE, TAB, SPACE, \
         the arrow keys, and the modifiers such as LEFT_CTRL. Media keys and QMK's own \
         quantum keycodes are not named here yet."
    )
}

/// Read a required whole number argument.
fn number(arguments: &Value, name: &str) -> Result<u8, String> {
    let Some(value) = arguments.get(name).and_then(Value::as_u64) else {
        return Err(format!(
            "the `{name}` argument is missing or is not a number"
        ));
    };
    u8::try_from(value).map_err(|_| format!("`{name}` is {value}, which no keyboard has"))
}

/// Read an optional whole number argument.
fn optional_number(arguments: &Value, name: &str) -> Result<Option<u8>, String> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => number(arguments, name).map(Some),
    }
}

/// Every attached VIA candidate, or a message saying none is.
async fn enumerate() -> Result<Vec<Attached>, String> {
    let attached = via::attached()
        .await
        .map_err(|error| format!("failed to enumerate HID devices: {error}"))?;
    if attached.is_empty() {
        return Err(
            "no QMK keyboard or macro pad with VIA enabled is attached. A board only \
             answers if its firmware was built with VIA support; if one is plugged in \
             and this build cannot see it, the next thing to check is whether this \
             process can open raw HID devices at all."
                .to_owned(),
        );
    }
    Ok(attached)
}

/// Open the first attached board.
async fn open_first(attached: &[Attached]) -> Result<Session, String> {
    let first = attached
        .first()
        .ok_or_else(|| "no keyboard to open".to_owned())?;
    Session::open(first)
        .await
        .map_err(|error| format!("{error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{number, optional_number, resolve, tools, unknown_key};

    #[test]
    fn a_key_name_resolves_however_it_is_spelled() {
        for spelling in ["F13", "f13", "KC_F13", " F13 "] {
            assert_eq!(resolve(spelling), Ok(0x0068), "{spelling}");
        }
    }

    #[test]
    fn a_number_resolves_too() {
        assert_eq!(resolve("0x0068"), Ok(0x0068));
        assert_eq!(resolve("200"), Ok(200));
    }

    /// `set_key` hands back a `to_undo` instruction built from
    /// `keycode::describe`. If a name it can print does not resolve again, the
    /// undo is not a way back — it is something that looks like one, which is
    /// worse than offering nothing. The CLI has this check; the tool an
    /// assistant drives needs it more, because the assistant will repeat that
    /// string verbatim and trust it.
    #[test]
    fn every_key_the_undo_instruction_can_name_resolves_again() {
        for code in [
            0x0000_u16, 0x0001, 0x0004, 0x001e, 0x0045, 0x0068, 0x00e0, 0x00ff, 0xffff,
        ] {
            let printed = openlogi_via::keycode::describe(code);
            assert_eq!(
                resolve(&printed),
                Ok(code),
                "the undo instruction would say {printed:?}, which does not resolve"
            );
        }
    }

    /// A model that gets "unknown key" and nothing else will guess again. The
    /// message has to say what the vocabulary is, and what is missing from it.
    #[test]
    fn an_unknown_key_says_what_names_exist_and_what_does_not() {
        let message = resolve("MEDIA_PLAY").expect_err("not a known key");
        assert!(message.contains("MEDIA_PLAY"), "{message}");
        assert!(message.contains("F1 to F24"), "{message}");
        assert!(message.contains("quantum"), "{message}");
        assert_eq!(message, unknown_key("MEDIA_PLAY"));
    }

    #[test]
    fn a_missing_position_argument_says_which_one() {
        let error = number(&json!({ "row": 1 }), "layer").expect_err("layer is missing");
        assert!(error.contains("`layer`"), "{error}");
    }

    /// A layer index past what a byte holds is a model mistake, not a device
    /// one, and saying so beats an arithmetic error from deep in the driver.
    #[test]
    fn an_impossible_position_is_refused_before_it_reaches_a_device() {
        let error = number(&json!({ "layer": 9999 }), "layer").expect_err("no keyboard has that");
        assert!(error.contains("9999"), "{error}");
        assert!(error.contains("no keyboard has"), "{error}");
    }

    #[test]
    fn an_absent_optional_argument_is_not_an_error() {
        assert_eq!(optional_number(&json!({}), "rows"), Ok(None));
        assert_eq!(optional_number(&json!({ "rows": null }), "rows"), Ok(None));
        assert_eq!(optional_number(&json!({ "rows": 8 }), "rows"), Ok(Some(8)));
    }

    /// The description is the only thing steering a model towards reading
    /// before writing, and towards raising the read area when a key is
    /// missing rather than concluding it does not exist.
    #[test]
    fn the_descriptions_say_how_to_use_the_tools_together() {
        let catalog = tools();
        let list = catalog[0]["description"].as_str().expect("a description");
        assert!(list.contains("Start here"), "{list}");
        let read = catalog[1]["description"].as_str().expect("a description");
        assert!(read.contains("counted"), "{read}");
        let set = catalog[2]["description"].as_str().expect("a description");
        assert!(set.contains("what was there before"), "{set}");
        assert!(set.contains("read back"), "{set}");
    }
}
