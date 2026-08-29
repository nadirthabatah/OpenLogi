//! Stream Decks, over MCP.
//!
//! Like the camera tools, these reach hardware directly rather than through
//! the agent: the agent does not own Stream Decks yet, and routing through it
//! would mean inventing a wire contract before there is anything to carry.
//!
//! Keys are addressed by index, but every answer also gives the row and column,
//! because an index alone is not something anyone can act on while looking at a
//! physical grid — and is not something a screen reader can make sense of at
//! all. "Key 7" and "row 2, column 3" always travel together here.

use openlogi_hid::streamdeck::{self, Attached, Session};
use openlogi_streamdeck::model::Model;
use openlogi_streamdeck::render;
use openlogi_streamdeck::report::Brightness;
use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};

/// The `serial` argument, shared by every tool that addresses one deck.
fn deck_argument() -> Value {
    json!({
        "type": "string",
        "description": "The deck's `serial` from list_stream_decks. Omit when only \
            one Stream Deck is attached.",
    })
}

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_stream_decks",
            "description": "List attached Elgato Stream Decks, with each one's key \
                count and grid shape. Start here before addressing a key.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "set_stream_deck_brightness",
            "description": "Set a Stream Deck's key screen brightness, as a percentage.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "deck": deck_argument(),
                    "percent": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 100,
                        "description": "0 turns the screens off without powering the \
                            device down; 100 is full brightness.",
                    },
                },
                "required": ["percent"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "set_stream_deck_key_colour",
            "description": "Fill one Stream Deck key with a solid colour. Keys are \
                numbered from 0 at the top left, running left to right then down; \
                list_stream_decks gives the grid shape so a position can be turned \
                into an index.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "deck": deck_argument(),
                    "key": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Key index, 0 at the top left.",
                    },
                    "colour": {
                        "type": "string",
                        "pattern": "^[0-9a-fA-F]{6}$",
                        "description": "Six hex digits, \"RRGGBB\", no leading '#'.",
                    },
                },
                "required": ["key", "colour"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "clear_stream_deck",
            "description": "Turn every key on a Stream Deck black.",
            "inputSchema": json!({
                "type": "object",
                "properties": { "deck": deck_argument() },
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Describe an attached deck for a tool result.
fn describe(attached: &Attached) -> Value {
    json!({
        "serial": attached.serial_number,
        "model": attached.model.name,
        "name": attached.name,
        "keys": attached.model.key_count(),
        "columns": attached.model.grid.columns,
        "rows": attached.model.grid.rows,
        "has_key_screens": attached.model.screens.is_some(),
    })
}

/// Every attached deck, one entry per physical device.
pub async fn list_stream_decks() -> Result<String, String> {
    let collections = streamdeck::attached()
        .await
        .map_err(|error| format!("could not enumerate HID devices: {error}"))?;
    let decks = streamdeck::preferred(&collections);
    if decks.is_empty() {
        let strangers = streamdeck::unrecognized()
            .await
            .map_err(|error| format!("could not enumerate HID devices: {error}"))?;
        if strangers.is_empty() {
            return Ok(
                "no Stream Deck is attached, or this program cannot see HID \
                       devices (on macOS that means Input Monitoring has not been \
                       granted; on Linux, the udev rules)"
                    .to_string(),
            );
        }
        return Ok(format!(
            "an Elgato device is attached but is not in this build's catalogue: {}. \
             Adding it needs only its product id.",
            strangers
                .iter()
                .map(|s| format!("product {:#06x} ({:?})", s.product_id, s.name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    rendered(&Value::Array(
        decks.into_iter().map(describe).collect::<Vec<_>>(),
    ))
}

/// Open the deck the `deck` argument names.
///
/// Omitting it resolves only when exactly one deck is attached: silently
/// picking one of several would light up a different device than the caller
/// meant, with nothing to signal it.
async fn open(arguments: &Value) -> Result<Session, String> {
    let collections = streamdeck::attached()
        .await
        .map_err(|error| format!("could not enumerate HID devices: {error}"))?;
    let decks = streamdeck::preferred(&collections);
    if decks.is_empty() {
        return Err("no Stream Deck is attached".to_string());
    }

    let chosen = match arguments.get("deck") {
        None | Some(Value::Null) if decks.len() == 1 => decks[0],
        None | Some(Value::Null) => {
            return Err(format!(
                "{} Stream Decks are attached, so `deck` is required — call \
                 list_stream_decks and pass the serial of the one you mean",
                decks.len()
            ));
        }
        Some(Value::String(serial)) => decks
            .iter()
            .find(|deck| deck.serial_number.as_deref() == Some(serial.as_str()))
            .copied()
            .ok_or_else(|| format!("no attached Stream Deck reports the serial {serial:?}"))?,
        Some(other) => return Err(format!("`deck` must be a serial string, not {other}")),
    };

    Session::open(chosen)
        .await
        .map_err(|error| format!("could not open the {}: {error}", chosen.model.name))
}

/// Set the key screens' brightness.
pub async fn set_stream_deck_brightness(arguments: &Value) -> Result<String, String> {
    let percent = arguments
        .get("percent")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| "`percent` must be a whole number from 0 to 100".to_string())?;
    let brightness = Brightness::new(percent).map_err(|error| error.to_string())?;
    let mut session = open(arguments).await?;
    let name = session.model().name;
    session
        .set_brightness(brightness)
        .await
        .map_err(|error| format!("setting brightness failed: {error}"))?;
    Ok(format!("{name} brightness set to {percent}%"))
}

/// Fill one key with a colour.
pub async fn set_stream_deck_key_colour(arguments: &Value) -> Result<String, String> {
    let key = arguments
        .get("key")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "`key` must be a whole number, 0 at the top left".to_string())?;
    let (red, green, blue) = colour(arguments)?;

    let mut session = open(arguments).await?;
    let model = session.model();
    let encoded = encode_solid(model, red, green, blue)?;
    session
        .set_key_image(key, &encoded)
        .await
        .map_err(|error| format!("setting the key image failed: {error}"))?;
    Ok(format!(
        "key {key} on the {} is now that colour ({})",
        model.name,
        position(model, key)
    ))
}

/// Turn every key black.
pub async fn clear_stream_deck(arguments: &Value) -> Result<String, String> {
    let mut session = open(arguments).await?;
    let model = session.model();
    let encoded = encode_solid(model, 0, 0, 0)?;
    for key in 0..model.key_count() {
        session
            .set_key_image(key, &encoded)
            .await
            .map_err(|error| format!("clearing key {key} failed: {error}"))?;
    }
    Ok(format!("all {} keys cleared", model.key_count()))
}

/// A solid-colour key image for this model.
fn encode_solid(model: &Model, red: u8, green: u8, blue: u8) -> Result<Vec<u8>, String> {
    let picture = render::solid(model, red, green, blue).map_err(|error| error.to_string())?;
    render::key_image(model, &picture).map_err(|error| error.to_string())
}

/// Where a key sits, for an answer someone can act on.
fn position(model: &Model, key: u16) -> String {
    model.key_position(key).map_or_else(
        |_| "out of range".to_string(),
        |at| format!("row {}, column {}", at.row, at.column),
    )
}

/// Read the six-hex-digit `colour` argument.
fn colour(arguments: &Value) -> Result<(u8, u8, u8), String> {
    let text = arguments
        .get("colour")
        .and_then(Value::as_str)
        .ok_or_else(|| "`colour` must be six hex digits, \"RRGGBB\"".to_string())?;
    let packed = (text.len() == 6)
        .then(|| u32::from_str_radix(text, 16).ok())
        .flatten()
        .ok_or_else(|| {
            format!("`colour` must be six hex digits, \"RRGGBB\", with no '#' — got {text:?}")
        })?;
    // Each shift-and-mask selects exactly one byte.
    Ok((
        u8::try_from((packed >> 16) & 0xff).unwrap_or_default(),
        u8::try_from((packed >> 8) & 0xff).unwrap_or_default(),
        u8::try_from(packed & 0xff).unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{colour, position, tools};
    use openlogi_streamdeck::model::{ELGATO_VENDOR_ID, identify};

    #[test]
    fn colours_split_into_their_channels_in_the_right_order() {
        assert_eq!(
            colour(&json!({ "colour": "010203" })).expect("valid"),
            (1, 2, 3)
        );
        assert_eq!(
            colour(&json!({ "colour": "FF8800" })).expect("valid"),
            (0xff, 0x88, 0)
        );
    }

    #[test]
    fn a_malformed_colour_says_what_is_wanted() {
        for bad in [
            json!({}),
            json!({ "colour": "#ff8800" }),
            json!({ "colour": 16 }),
        ] {
            let error = colour(&bad).expect_err("not a colour");
            assert!(error.contains("RRGGBB"), "unhelpful message: {error}");
        }
    }

    #[test]
    fn a_key_is_always_reported_with_its_place_on_the_grid() {
        let mk2 = identify(ELGATO_VENDOR_ID, 0x0080).expect("catalogued");
        assert_eq!(position(mk2, 0), "row 1, column 1");
        assert_eq!(position(mk2, 6), "row 2, column 2");
        assert_eq!(position(mk2, 14), "row 3, column 5");
        assert_eq!(position(mk2, 99), "out of range");
    }

    #[test]
    fn addressing_one_deck_is_optional_but_a_key_and_colour_are_not() {
        let tools = tools();
        let fill = tools
            .iter()
            .find(|tool| tool["name"] == "set_stream_deck_key_colour")
            .expect("advertised");
        let required: Vec<&str> = fill["inputSchema"]["required"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|value| value.as_str().expect("a string"))
            .collect();
        assert_eq!(required, vec!["key", "colour"]);
        assert!(
            fill["inputSchema"]["properties"]["deck"].is_object(),
            "a deck may still be named when several are attached"
        );
    }
}
