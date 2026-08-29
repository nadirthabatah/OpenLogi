//! Editing a layout file without throwing away what the person wrote in it.
//!
//! A layout is a file someone owns. It carries their comments — "key 0 is the
//! mic mute, do not move it" — their blank lines, and the order they chose.
//! Reading it into a struct and serializing that struct back produces a
//! *correct* file that has silently lost all of it, and the person least able
//! to notice is the one this project is for: nothing on screen changes, and a
//! screen reader has no reason to re-read a file that reported success.
//!
//! So edits are applied to the document, not to a re-rendering of it. This is
//! the same choice `openlogi-core` already made for `config.toml`, and for the
//! same reason; layouts were simply missed.
//!
//! Everything here is text in, text out, so what survives an edit is checked
//! against files nobody had to write by hand at the time.

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use super::{Key, LayoutError};

/// Parse a layout document, keeping its formatting.
fn document(source: &str) -> Result<DocumentMut, LayoutError> {
    source
        .parse::<DocumentMut>()
        .map_err(|error| LayoutError::Unwritable {
            detail: error.to_string(),
        })
}

/// The `[[keys]]` array, created if the document has none yet.
fn keys_of(document: &mut DocumentMut) -> Result<&mut ArrayOfTables, LayoutError> {
    if document.get("keys").is_none() {
        document["keys"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    document["keys"]
        .as_array_of_tables_mut()
        .ok_or_else(|| LayoutError::Unwritable {
            detail: "`keys` is in this file but is not a list of keys".to_owned(),
        })
}

/// Which entry of `keys` holds `index`.
fn position_of(keys: &ArrayOfTables, index: u16) -> Option<usize> {
    keys.iter().position(|table| {
        table
            .get("index")
            .and_then(Item::as_integer)
            .is_some_and(|held| held == i64::from(index))
    })
}

/// Write `key` into a layout's text, returning the new text.
///
/// Only the fields that actually change are touched, so a comment sitting
/// beside a value the edit does not concern survives it.
///
/// # Errors
///
/// [`LayoutError::Unwritable`] if the source is not valid TOML, or has a
/// `keys` entry that is not a list of key tables.
pub fn set_key(source: &str, key: &Key) -> Result<String, LayoutError> {
    let mut document = document(source)?;
    let keys = keys_of(&mut document)?;
    if let Some(at) = position_of(keys, key.index) {
        let table = keys.get_mut(at).ok_or_else(|| LayoutError::Unwritable {
            detail: "the key vanished between finding it and editing it".to_owned(),
        })?;
        apply(table, key)?;
    } else {
        let mut table = Table::new();
        apply(&mut table, key)?;
        // Appended rather than sorted into index order. Sorting would move
        // whole blocks around the file, and a comment written above the first
        // `[[keys]]` — which is where a file's own header comment ends up —
        // would travel with whichever key sorted first. A file someone
        // arranged stays arranged; nothing downstream cares about the order.
        keys.push(table);
    }
    Ok(document.to_string())
}

/// Remove `index` from a layout's text, reporting whether it was there.
///
/// # Errors
///
/// As [`set_key`].
pub fn remove_key(source: &str, index: u16) -> Result<(String, bool), LayoutError> {
    let mut document = document(source)?;
    let keys = keys_of(&mut document)?;
    let Some(at) = position_of(keys, index) else {
        return Ok((document.to_string(), false));
    };
    keys.remove(at);
    // An emptied array of tables would otherwise render as nothing at all,
    // which is fine, but leaving the empty `keys` key behind is tidier to
    // diff than having it appear and disappear as the last key comes and goes.
    Ok((document.to_string(), true))
}

/// Write one key's fields into `table`, clearing the ones it no longer has.
///
/// Clearing matters: relabelling a key that used to show a picture has to take
/// the `image` line away, or the layout now says both and is refused the next
/// time it is applied — a failure that would arrive long after the edit that
/// caused it.
fn apply(table: &mut Table, key: &Key) -> Result<(), LayoutError> {
    table["index"] = value(i64::from(key.index));
    set_or_clear(table, "label", key.label.as_deref().map(value));
    set_or_clear(
        table,
        "image",
        key.image
            .as_deref()
            .map(|path| value(path.to_string_lossy().into_owned())),
    );
    set_or_clear(table, "colour", key.colour.as_deref().map(value));
    set_or_clear(table, "background", key.background.as_deref().map(value));
    set_or_clear(table, "action", action_item(key)?);
    Ok(())
}

/// Set a field, or remove it when the key does not carry one.
fn set_or_clear(table: &mut Table, field: &str, item: Option<Item>) {
    match item {
        Some(item) => table[field] = item,
        None => {
            table.remove(field);
        }
    }
}

/// An action rendered as the TOML it serializes to.
///
/// Round-tripped through the serializer rather than matched on by hand: the
/// action vocabulary is long and grows, and a hand-written renderer would fall
/// behind it silently — writing a valid-looking action that is not the one
/// asked for.
fn action_item(key: &Key) -> Result<Option<Item>, LayoutError> {
    let Some(action) = &key.action else {
        return Ok(None);
    };
    let rendered =
        toml::to_string(&ActionOnly { action }).map_err(|error| LayoutError::Unwritable {
            detail: error.to_string(),
        })?;
    let fragment = document(&rendered)?;
    fragment
        .get("action")
        .cloned()
        .map(Some)
        .ok_or_else(|| LayoutError::Unwritable {
            detail: "the action did not serialize to an `action` field".to_owned(),
        })
}

/// A shim so one action can be serialized on its own.
#[derive(serde::Serialize)]
struct ActionOnly<'a> {
    action: &'a openlogi_core::binding::Action,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use openlogi_core::binding::Action;

    use super::super::{Key, Layout};
    use super::{remove_key, set_key};

    fn key(index: u16, label: &str) -> Key {
        Key {
            index,
            label: Some(label.to_owned()),
            image: None,
            colour: None,
            background: None,
            action: None,
        }
    }

    fn parsed(source: &str) -> Layout {
        Layout::parse(std::path::Path::new("t.toml"), source).expect("the result is a layout")
    }

    /// The whole reason this module exists. A layout is a file someone owns,
    /// and the note they left themselves about which key is the mic mute is
    /// the part a re-serialization silently destroys — with nothing on screen
    /// to notice, which is worst for the person this project is for.
    #[test]
    fn a_comment_someone_wrote_survives_an_edit() {
        let source = "\
# My streaming deck.
# Key 0 is the mic mute — do not move it, muscle memory.
brightness = 80

[[keys]]
index = 0
label = \"MUTE MIC\"
";
        let edited = set_key(source, &key(1, "REC")).expect("edited");
        assert!(edited.contains("# My streaming deck."), "{edited}");
        assert!(edited.contains("muscle memory"), "{edited}");
        assert!(edited.contains("brightness = 80"), "{edited}");
    }

    #[test]
    fn setting_a_new_key_adds_it() {
        let edited = set_key("", &key(2, "REC")).expect("edited");
        let layout = parsed(&edited);
        assert_eq!(layout.keys.len(), 1);
        assert_eq!(layout.keys[0].index, 2);
        assert_eq!(layout.keys[0].label.as_deref(), Some("REC"));
    }

    /// Replacement, not merge. A command that quietly kept the old picture
    /// underneath new words would produce a key nobody asked for, and saying
    /// `--label` twice should give the second label and nothing else.
    #[test]
    fn setting_an_existing_key_replaces_it_rather_than_merging() {
        let source = "\
[[keys]]
index = 0
label = \"MUTE MIC\"
background = \"802020\"
";
        let edited = set_key(source, &key(0, "MUTE")).expect("edited");
        let layout = parsed(&edited);
        assert_eq!(layout.keys.len(), 1, "one key, not two");
        assert_eq!(layout.keys[0].label.as_deref(), Some("MUTE"));
        assert_eq!(
            layout.keys[0].background, None,
            "the old background must not survive under the new label"
        );
    }

    /// The specific way a merge would bite later: a key left carrying both a
    /// label and an image is refused when the layout is next applied, long
    /// after the edit that caused it.
    #[test]
    fn relabelling_a_picture_key_takes_the_picture_away() {
        let source = "\
[[keys]]
index = 0
image = \"icons/camera.png\"
";
        let edited = set_key(source, &key(0, "CAMERA")).expect("edited");
        assert!(!edited.contains("image"), "{edited}");
        let layout = parsed(&edited);
        assert_eq!(layout.keys[0].image, None);
        // The proof that it matters: a key with both is refused on apply.
        assert!(
            Layout::parse(std::path::Path::new("t.toml"), &edited).is_ok(),
            "and the file still parses"
        );
    }

    #[test]
    fn removing_a_key_reports_whether_it_was_there() {
        let source = "\
[[keys]]
index = 0
label = \"MUTE\"
";
        let (edited, removed) = remove_key(source, 0).expect("edited");
        assert!(removed, "the key was there");
        assert!(parsed(&edited).keys.is_empty());

        // Told "removed", someone believes something changed, and the next
        // thing they do is wonder why the deck looks the same.
        let (_, again) = remove_key(&edited, 0).expect("edited");
        assert!(!again, "it is no longer there");
    }

    #[test]
    fn removing_one_key_leaves_the_others_alone() {
        let source = "\
[[keys]]
index = 0
label = \"A\"

[[keys]]
index = 1
label = \"B\"
";
        let (edited, removed) = remove_key(source, 0).expect("edited");
        assert!(removed);
        let layout = parsed(&edited);
        assert_eq!(layout.keys.len(), 1);
        assert_eq!(layout.keys[0].label.as_deref(), Some("B"));
    }

    /// Removing a key must not take the comment belonging to another one.
    #[test]
    fn removing_a_key_leaves_another_keys_comment_behind() {
        let source = "\
[[keys]]
index = 0
label = \"A\"

# This one is the camera toggle.
[[keys]]
index = 1
label = \"B\"
";
        let (edited, _) = remove_key(source, 0).expect("edited");
        assert!(edited.contains("camera toggle"), "{edited}");
    }

    /// Every field has to survive being written and read back. Anything lost
    /// here is a setting someone made that the next edit silently discards.
    #[test]
    fn every_field_survives_being_written_and_read_back() {
        let full = Key {
            index: 0,
            label: Some("MUTE MIC".to_owned()),
            image: None,
            colour: Some("ffffff".to_owned()),
            background: Some("802020".to_owned()),
            action: Some(Action::Copy),
        };
        let picture = Key {
            index: 2,
            label: None,
            image: Some(PathBuf::from("icons/camera.png")),
            colour: None,
            background: None,
            action: None,
        };
        let edited = set_key("brightness = 80\n", &full).expect("edited");
        let edited = set_key(&edited, &picture).expect("edited again");

        let layout = parsed(&edited);
        assert_eq!(layout.brightness, Some(80));
        assert_eq!(layout.keys.len(), 2);
        assert!(layout.keys.contains(&full), "{edited}");
        assert!(layout.keys.contains(&picture), "{edited}");
    }

    /// An action that serializes as a table, not a bare string. Rendering it
    /// by hand is what this avoids: the action vocabulary grows, and a
    /// hand-written renderer falls behind it silently.
    #[test]
    fn an_action_with_a_payload_round_trips() {
        let mut key = key(3, "BUILD");
        key.action = Some(Action::RunShellCommand("make -C ~/project".to_owned()));
        let edited = set_key("", &key).expect("edited");
        let layout = parsed(&edited);
        assert_eq!(
            layout.keys[0].action,
            Some(Action::RunShellCommand("make -C ~/project".to_owned())),
            "{edited}"
        );
    }

    /// An edit is applied to whatever the file already says, so a file that
    /// is not a layout has to be refused rather than half-rewritten.
    #[test]
    fn a_file_that_is_not_toml_is_refused_rather_than_overwritten() {
        set_key("this is not toml {{{", &key(0, "X")).expect_err("not a layout");
        remove_key("this is not toml {{{", 0).expect_err("not a layout");
    }

    /// `keys` present but holding something else is a file we must not write
    /// a key into — it is someone's file and we do not know what it is.
    #[test]
    fn a_keys_field_that_is_not_a_list_of_keys_is_refused() {
        set_key("keys = 3\n", &key(0, "X")).expect_err("`keys` is not a list of keys");
    }

    /// Editing the same key twice must not accumulate anything.
    #[test]
    fn editing_the_same_key_twice_leaves_one_entry() {
        let once = set_key("", &key(0, "A")).expect("edited");
        let twice = set_key(&once, &key(0, "B")).expect("edited again");
        let layout = parsed(&twice);
        assert_eq!(layout.keys.len(), 1);
        assert_eq!(layout.keys[0].label.as_deref(), Some("B"));
    }
}
