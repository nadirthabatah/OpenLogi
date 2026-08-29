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

use openlogi_streamdeck::model::Model;
use serde::Deserialize;
use thiserror::Error;

/// A deck layout as written in a file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    /// An entry that says nothing about what to draw.
    #[error("key {index} has neither a label nor an image, so there is nothing to draw")]
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
            match (&key.label, &key.image) {
                (None, None) => return Err(LayoutError::NothingToDraw { index: key.index }),
                (Some(_), Some(_)) => return Err(LayoutError::Ambiguous { index: key.index }),
                _ => {}
            }
        }
        Ok(())
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
    use std::path::Path;

    use openlogi_streamdeck::model::{ELGATO_VENDOR_ID, Model, identify};

    use super::{Layout, LayoutError};

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

    /// A misspelled field silently doing nothing is worse than a refusal:
    /// the key just stays blank and nothing says why.
    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let error = parse(
            r#"
            [[keys]]
            index = 0
            lable = "TYPO"
            "#,
        )
        .expect_err("misspelled field");
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
    fn a_key_with_nothing_to_draw_is_refused() {
        let layout = parse(
            r#"
            [[keys]]
            index = 0
            colour = "ffffff"
            "#,
        )
        .expect("parses");
        assert_eq!(
            layout.validate(mk2()).expect_err("nothing to draw"),
            LayoutError::NothingToDraw { index: 0 }
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
}
