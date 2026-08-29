//! Saved Stream Deck layouts, over MCP.
//!
//! An assistant that can set one key at a time can dress a deck; an assistant
//! that can name a layout can restore the whole thing someone spent an evening
//! on. "Put my streaming layout back" is a sentence, and this is what makes it
//! a single call rather than thirty-two.
//!
//! Applying is a read-only act as far as the person's files go — the layout is
//! not modified, only sent to a device whose screens are volatile anyway.
//!
//! Editing is different, and the shape it takes here is the result of changing
//! my mind. The first version of this module deliberately had no tool that
//! wrote a layout at all: a layout is something a person composed, the deck's
//! own memory is not a copy of it, and an assistant rewriting one on a
//! misunderstanding would destroy work with nothing to restore from.
//!
//! That reasoning holds against rewriting a *file*. It does not hold against
//! setting one key, which is what someone actually asks for — "put MUTE MIC on
//! the top left of my streaming layout" is a sentence, and refusing it while
//! the command line does it happily is a gap in the interface that a blind
//! user relies on most.
//!
//! So the tools here edit one key at a time, never the whole file, and every
//! change reports the key as it was. That is the same shape `set_key` takes
//! for keyboards and for the same reason: a permanent change is acceptable
//! when the answer carries what it takes to undo it.

use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_layouts",
            "description": "List the Stream Deck layouts saved on this machine, by name. \
                A layout holds a whole deck's faces — every key's label, picture and \
                colour — so applying one restores the lot at once.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "apply_layout",
            "description": "Apply a saved layout to the attached Stream Deck, setting \
                every key it names in one go. Use this in preference to setting keys one \
                at a time when the person refers to a layout by name. This does not \
                change the saved file.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The layout's name, from list_layouts.",
                    },
                },
                "required": ["name"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "set_layout_key",
            "description": "Set one key in a saved layout: its words or picture, its \
                colours, and what pressing it does. Edits the saved file, so the change \
                outlasts the deck losing power — unlike set_stream_deck_key_label, which \
                only changes what is on the deck right now. The result reports the key as \
                it was, so you can offer to put it back. Setting a key replaces it \
                entirely rather than merging, so give every property you want it to have.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "layout": {
                        "type": "string",
                        "description": "The layout's name. A layout that does not exist \
                            yet is created — the first key set is the layout.",
                    },
                    "key": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Key index, 0 at the top left, running left to \
                            right then down.",
                    },
                    "label": {
                        "type": "string",
                        "description": "Words to write on the key. A key shows words or \
                            a picture, not both.",
                    },
                    "image": {
                        "type": "string",
                        "description": "Path to a picture, relative to the layout file.",
                    },
                    "colour": {
                        "type": "string",
                        "description": "Text colour, six hex digits, no leading '#'.",
                    },
                    "background": {
                        "type": "string",
                        "description": "Background colour, six hex digits.",
                    },
                    "action": {
                        "type": "string",
                        "description": "What pressing the key does, by name: \"Copy\", \
                            \"NextTab\", \"VolumeUp\". Actions that run a program or type \
                            text have to be written in the file by the person, not set \
                            here.",
                    },
                },
                "required": ["layout", "key"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "unset_layout_key",
            "description": "Remove one key from a saved layout. The result reports the \
                key as it was, so you can offer to put it back.",
            "inputSchema": json!({
                "type": "object",
                "properties": {
                    "layout": { "type": "string", "description": "The layout's name." },
                    "key": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Key index, 0 at the top left.",
                    },
                },
                "required": ["layout", "key"],
                "additionalProperties": false,
            }),
        }),
    ]
}

/// Run `set_layout_key`.
pub fn set_layout_key(arguments: &Value) -> Result<String, String> {
    let layout = text(arguments, "layout")?;
    let index = index_of(arguments)?;
    let label = optional_text(arguments, "label");
    let image = optional_text(arguments, "image");
    if label.is_some() && image.is_some() {
        return Err("a key shows words or a picture, not both; give `label` or `image`".to_owned());
    }
    if label.is_none() && image.is_none() && arguments.get("action").is_none() {
        return Err(
            "a key needs something: `label`, `image`, or an `action`. A key with none \
             of those would be an entry that says nothing."
                .to_owned(),
        );
    }
    // Colours checked here so a typo comes back as an answer the model can
    // correct, rather than as a failure when the layout is next applied.
    for name in ["colour", "background"] {
        if let Some(value) = optional_text(arguments, name)
            && crate::cmd::streamdeck::parse_colour(&value).is_err()
        {
            return Err(format!(
                "`{name}` is \"{value}\", which is not six hex digits such as \"ff8800\""
            ));
        }
    }
    let action = match optional_text(arguments, "action") {
        Some(name) => {
            Some(crate::cmd::streamdeck::parse_action(&name).map_err(|error| format!("{error}"))?)
        }
        None => None,
    };

    let key = crate::cmd::streamdeck::layout_key(
        index,
        label,
        image,
        optional_text(arguments, "colour"),
        optional_text(arguments, "background"),
        action,
    );
    let was = crate::cmd::streamdeck::set_layout_key(&layout, &key)
        .map_err(|error| format!("{error}"))?;
    rendered(&json!({
        "layout": layout,
        "key": index,
        "was": describe_key(was.as_ref()),
        "note": "Saved to the layout file. Apply the layout to see it on the deck.",
    }))
}

/// Run `unset_layout_key`.
pub fn unset_layout_key(arguments: &Value) -> Result<String, String> {
    let layout = text(arguments, "layout")?;
    let index = index_of(arguments)?;
    let was = crate::cmd::streamdeck::unset_layout_key(&layout, index)
        .map_err(|error| format!("{error}"))?;
    if was.is_none() {
        // Told "removed", a person believes something changed and then
        // wonders why the deck looks the same.
        return Err(format!(
            "key {index} was not in the layout \"{layout}\", so nothing changed"
        ));
    }
    rendered(&json!({
        "layout": layout,
        "key": index,
        "was": describe_key(was.as_ref()),
        "note": "Removed from the layout file. Apply the layout to see the change on \
                 the deck.",
    }))
}

/// A key as the model sees it, for reporting what a change replaced.
fn describe_key(key: Option<&crate::cmd::streamdeck::LayoutKey>) -> Value {
    let Some(key) = key else {
        return json!(null);
    };
    let mut entry = serde_json::Map::new();
    if let Some(label) = &key.label {
        entry.insert("label".to_owned(), json!(label));
    }
    if let Some(image) = &key.image {
        entry.insert("image".to_owned(), json!(image.to_string_lossy()));
    }
    if let Some(colour) = &key.colour {
        entry.insert("colour".to_owned(), json!(colour));
    }
    if let Some(background) = &key.background {
        entry.insert("background".to_owned(), json!(background));
    }
    if let Some(action) = &key.action {
        entry.insert("action".to_owned(), json!(format!("{action:?}")));
    }
    Value::Object(entry)
}

/// A required string argument.
fn text(arguments: &Value, name: &str) -> Result<String, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("the `{name}` argument is missing"))
}

/// An optional string argument, absent when empty.
fn optional_text(arguments: &Value, name: &str) -> Option<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// The `key` argument as a key index.
fn index_of(arguments: &Value) -> Result<u16, String> {
    let Some(value) = arguments.get("key").and_then(Value::as_u64) else {
        return Err("the `key` argument is missing or is not a number".to_owned());
    };
    u16::try_from(value).map_err(|_| format!("key {value} is beyond any Stream Deck"))
}

/// Run `list_layouts`.
pub fn list_layouts() -> Result<String, String> {
    let directory = crate::cmd::streamdeck::layout_library().map_err(|error| format!("{error}"))?;
    let names = crate::cmd::streamdeck::saved_layouts().map_err(|error| format!("{error}"))?;
    rendered(&json!({
        "layouts": names,
        "directory": directory.to_string_lossy(),
        "note": if names.is_empty() {
            "None saved yet. A person creates one with `openlogi streamdeck example \
             <name>` and edits it; there is deliberately no tool here that writes a \
             layout file, because a layout is work someone composed and the deck's own \
             memory is not a copy of it."
        } else {
            "Apply one with apply_layout."
        },
    }))
}

/// Run `apply_layout`.
pub async fn apply_layout(arguments: &Value) -> Result<String, String> {
    let Some(name) = arguments.get("name").and_then(Value::as_str) else {
        return Err("the `name` argument is missing; list_layouts gives the names".to_owned());
    };
    // A name that is not saved is worth saying before a device is opened: the
    // name is wrong whether or not a deck is attached, and "no Stream Deck
    // found" would send the model hunting the wrong problem.
    let saved = crate::cmd::streamdeck::saved_layouts().map_err(|error| format!("{error}"))?;
    if !saved.iter().any(|known| known == name) {
        return Err(format!(
            "there is no saved layout called \"{name}\". Saved layouts: {}",
            if saved.is_empty() {
                "none".to_owned()
            } else {
                saved.join(", ")
            }
        ));
    }

    let applied = crate::cmd::streamdeck::apply_saved(name)
        .await
        .map_err(|error| format!("{error}"))?;
    rendered(&json!({
        "layout": name,
        "keys_set": applied,
        "note": "The deck's screens are volatile: this holds until the deck loses power, \
                 and applying the layout again restores it.",
    }))
}

#[cfg(test)]
mod tests {
    use super::tools;

    /// Without this steer a model will set thirty-two keys one at a time when
    /// the person said one word.
    #[test]
    fn apply_is_recommended_over_setting_keys_one_at_a_time() {
        let catalog = tools();
        let apply = catalog[1]["description"].as_str().expect("a description");
        assert!(apply.contains("in preference"), "{apply}");
        assert!(apply.contains("one at a time"), "{apply}");
    }

    /// `apply` still does not touch the file, and has to keep saying so —
    /// otherwise a model asked to "change my layout" reaches for the tool
    /// that only paints the deck, and the change is gone with the cable.
    #[test]
    fn apply_still_says_it_does_not_change_the_saved_file() {
        let catalog = tools();
        let apply = catalog[1]["description"].as_str().expect("a description");
        assert!(apply.contains("does not change the saved file"), "{apply}");
    }

    /// The rule that replaced "no writing tools at all": editing is one key at
    /// a time, never the whole file. A tool taking a list of keys, or a whole
    /// layout, would be the file rewrite this module refuses.
    #[test]
    fn editing_is_one_key_at_a_time_and_never_a_whole_layout() {
        for tool in tools() {
            assert!(
                tool["inputSchema"]["properties"].get("keys").is_none(),
                "{} takes a list of keys, which is a file rewrite",
                tool["name"]
            );
        }
    }

    /// A permanent change is acceptable when the answer carries what it takes
    /// to undo it. Both editing tools promise that where the model reads it.
    #[test]
    fn every_editing_tool_promises_to_report_what_it_replaced() {
        for tool in tools() {
            let name = tool["name"].as_str().expect("a name").to_owned();
            if !name.ends_with("_layout_key") {
                continue;
            }
            let description = tool["description"].as_str().expect("a description");
            assert!(
                description.contains("as it was"),
                "{name} does not promise the previous value: {description}"
            );
            assert!(
                description.contains("put it back"),
                "{name} does not say what it is for: {description}"
            );
        }
    }

    /// The distinction a model most needs and is least likely to infer: one
    /// tool changes the deck until it loses power, the other changes the file.
    #[test]
    fn set_layout_key_distinguishes_itself_from_painting_the_live_deck() {
        let catalog = tools();
        let set = catalog
            .iter()
            .find(|tool| tool["name"] == "set_layout_key")
            .expect("the tool exists");
        let description = set["description"].as_str().expect("a description");
        assert!(
            description.contains("set_stream_deck_key_label"),
            "{description}"
        );
        assert!(description.contains("outlasts"), "{description}");
        assert!(description.contains("replaces it"), "{description}");
    }

    /// Actions that run programs are the person's to write. A model offering
    /// to bind one is offering to do what `run` refuses by default.
    #[test]
    fn set_layout_key_is_not_the_route_for_program_running_actions() {
        let catalog = tools();
        let set = catalog
            .iter()
            .find(|tool| tool["name"] == "set_layout_key")
            .expect("the tool exists");
        let action = set["inputSchema"]["properties"]["action"]["description"]
            .as_str()
            .expect("a description");
        assert!(action.contains("run a program"), "{action}");
        assert!(action.contains("not set"), "{action}");
    }
}
