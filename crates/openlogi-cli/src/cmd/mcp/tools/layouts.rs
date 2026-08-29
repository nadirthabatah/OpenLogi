//! Saved Stream Deck layouts, over MCP.
//!
//! An assistant that can set one key at a time can dress a deck; an assistant
//! that can name a layout can restore the whole thing someone spent an evening
//! on. "Put my streaming layout back" is a sentence, and this is what makes it
//! a single call rather than thirty-two.
//!
//! Applying is a read-only act as far as the person's files go — the layout is
//! not modified, only sent to a device whose screens are volatile anyway. What
//! is deliberately *not* here is a tool that writes or edits a layout file. A
//! layout is something a person composed; an assistant rewriting one on a
//! misunderstanding would destroy work with no copy anywhere, because the
//! deck's own memory is not a copy — it goes when the cable does.

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
    ]
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

    /// The tools here read and apply; nothing writes a layout file. Saying so
    /// where the model reads it is how it stops offering to.
    #[test]
    fn nothing_here_claims_to_change_a_saved_layout() {
        let catalog = tools();
        let apply = catalog[1]["description"].as_str().expect("a description");
        assert!(apply.contains("does not change the saved file"), "{apply}");
        assert_eq!(
            catalog.len(),
            2,
            "a writing tool would need its own reasons"
        );
    }
}
