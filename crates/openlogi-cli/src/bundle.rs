//! A profile bundle: everything about a setup, in one folder you can copy.
//!
//! A profile file carries the configuration — bindings, per-app overlays,
//! camera settings. It does not carry a Stream Deck layout, because a layout
//! is its own file and may name icon files beside it. Someone moving to a new
//! computer wants *their setup*, not the half of it that fits in one file.
//!
//! A bundle is a directory, not an archive. That is deliberate: the promise
//! this project makes is that your settings are plain text you can read and
//! edit, and a zip would take that back for the sake of one fewer file to
//! copy. A folder can be inspected, diffed, and kept in git, and every tool a
//! person already has can move one.
//!
//! ```text
//! my-setup/
//!   config.toml        the configuration a profile file would hold
//!   layouts/
//!     streaming.toml   one per Stream Deck layout
//!     streaming/       any icons that layout names, if it has them
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// What a bundle held, so the command can say what it actually did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contents {
    /// Where it was written or read.
    pub path: PathBuf,
    /// Names of the layouts carried.
    pub layouts: Vec<String>,
}

/// The configuration file inside a bundle.
#[must_use]
pub fn config_in(bundle: &Path) -> PathBuf {
    bundle.join("config.toml")
}

/// The layouts directory inside a bundle.
#[must_use]
pub fn layouts_in(bundle: &Path) -> PathBuf {
    bundle.join("layouts")
}

/// Whether a path names a bundle rather than a single profile file.
///
/// A `.toml` path is a profile file; anything else is a bundle directory. The
/// rule is the same one the layout library uses for names versus paths, so
/// there is one thing to learn rather than two — and both commands say which
/// reading they took, so a wrong guess is corrected immediately rather than
/// discovered later.
#[must_use]
pub fn is_bundle(path: &Path) -> bool {
    !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
}

/// Copy every file in `from` into `to`, recursing into directories.
///
/// Used for a layout's icons, which sit in a directory beside it. Recursive
/// because a person may organise icons into folders, and a copy that silently
/// flattened or skipped them would produce a bundle that looks complete and
/// applies to a deck full of blank keys.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("failed to create {}", to.display()))?;
    let entries =
        std::fs::read_dir(from).with_context(|| format!("failed to read {}", from.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read {}", from.display()))?;
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &destination)?;
        } else {
            std::fs::copy(&source, &destination)
                .with_context(|| format!("failed to copy {}", source.display()))?;
        }
    }
    Ok(())
}

/// Copy the layout library into a bundle.
///
/// Returns the layout names carried. A missing library is not a failure: a
/// machine with no saved layouts has a setup that is entirely configuration,
/// and that is a complete bundle rather than a broken one.
pub fn gather_layouts(library: &Path, bundle: &Path) -> Result<Vec<String>> {
    if !library.is_dir() {
        return Ok(Vec::new());
    }
    let destination = layouts_in(bundle);
    copy_tree(library, &destination)?;
    Ok(names_in(&destination))
}

/// Copy a bundle's layouts back into the library.
///
/// Existing layouts of the same name are overwritten, which is what importing
/// a setup means. The configuration is backed up before an import for the same
/// reason it always was; layouts are not, because a layout is a file the
/// person chose to keep and can see, not hidden state.
pub fn restore_layouts(bundle: &Path, library: &Path) -> Result<Vec<String>> {
    let source = layouts_in(bundle);
    if !source.is_dir() {
        return Ok(Vec::new());
    }
    copy_tree(&source, library)?;
    Ok(names_in(&source))
}

/// The layout names in a directory, sorted.
fn names_in(directory: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()? != "toml" {
                return None;
            }
            Some(path.file_stem()?.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{config_in, gather_layouts, is_bundle, layouts_in, restore_layouts};

    /// A scratch directory that cleans itself up.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "openlogi-bundle-{tag}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_nanos())
            ));
            std::fs::create_dir_all(&path).expect("a scratch directory");
            Self(path)
        }

        fn join(&self, tail: &str) -> PathBuf {
            self.0.join(tail)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(path, body).expect("a file");
    }

    #[test]
    fn a_toml_path_is_a_profile_file_and_anything_else_is_a_bundle() {
        assert!(!is_bundle(Path::new("setup.toml")));
        assert!(!is_bundle(Path::new("Setup.TOML")));
        assert!(is_bundle(Path::new("my-setup")));
        assert!(is_bundle(Path::new("/backups/desk")));
    }

    #[test]
    fn a_bundle_names_its_parts_predictably() {
        let bundle = Path::new("/x/setup");
        assert_eq!(config_in(bundle), PathBuf::from("/x/setup/config.toml"));
        assert_eq!(layouts_in(bundle), PathBuf::from("/x/setup/layouts"));
    }

    /// A machine with no saved layouts has a setup that is entirely
    /// configuration. That is a complete bundle, not a broken one.
    #[test]
    fn a_library_that_does_not_exist_yet_makes_an_empty_layout_set() {
        let scratch = Scratch::new("empty");
        let carried = gather_layouts(&scratch.join("nothing-here"), &scratch.join("bundle"))
            .expect("an absent library is not a failure");
        assert!(carried.is_empty());
    }

    #[test]
    fn gathering_carries_every_layout_by_name() {
        let scratch = Scratch::new("gather");
        let library = scratch.join("library");
        write(&library.join("streaming.toml"), "brightness = 80\n");
        write(&library.join("work.toml"), "brightness = 50\n");
        let bundle = scratch.join("bundle");

        let carried = gather_layouts(&library, &bundle).expect("gathered");
        assert_eq!(carried, vec!["streaming".to_owned(), "work".to_owned()]);
        assert!(layouts_in(&bundle).join("streaming.toml").is_file());
    }

    /// The failure this exists to prevent: a bundle that looks complete and
    /// applies to a deck full of blank keys, because the icons a layout names
    /// were left behind.
    #[test]
    fn a_layouts_icons_travel_with_it() {
        let scratch = Scratch::new("icons");
        let library = scratch.join("library");
        write(&library.join("streaming.toml"), "brightness = 80\n");
        write(&library.join("streaming/camera.png"), "not really a png");
        write(&library.join("streaming/deep/mic.png"), "nor this");
        let bundle = scratch.join("bundle");

        gather_layouts(&library, &bundle).expect("gathered");
        let carried = layouts_in(&bundle);
        assert!(carried.join("streaming/camera.png").is_file());
        assert!(
            carried.join("streaming/deep/mic.png").is_file(),
            "a nested icon directory must not be flattened or skipped"
        );
    }

    /// An icon file must not be offered as a layout you could apply.
    #[test]
    fn only_layout_files_are_counted_as_layouts() {
        let scratch = Scratch::new("counting");
        let library = scratch.join("library");
        write(&library.join("streaming.toml"), "brightness = 80\n");
        write(&library.join("streaming/camera.png"), "an icon");
        let bundle = scratch.join("bundle");

        let carried = gather_layouts(&library, &bundle).expect("gathered");
        assert_eq!(carried, vec!["streaming".to_owned()]);
    }

    /// The whole point: a bundle made on one machine puts the layouts back on
    /// another, icons included.
    #[test]
    fn a_bundle_restores_onto_a_machine_with_nothing_on_it() {
        let scratch = Scratch::new("restore");
        let library = scratch.join("library");
        write(&library.join("streaming.toml"), "brightness = 80\n");
        write(&library.join("streaming/camera.png"), "an icon");
        let bundle = scratch.join("bundle");
        gather_layouts(&library, &bundle).expect("gathered");

        let fresh = scratch.join("fresh-machine");
        let restored = restore_layouts(&bundle, &fresh).expect("restored");
        assert_eq!(restored, vec!["streaming".to_owned()]);
        assert!(fresh.join("streaming.toml").is_file());
        assert!(fresh.join("streaming/camera.png").is_file());
    }

    /// A profile file exported the old way carries no layouts. Restoring from
    /// it must be an ordinary empty result, not a failure — that is still a
    /// valid thing to hand this function.
    #[test]
    fn a_bundle_with_no_layouts_restores_nothing_without_complaining() {
        let scratch = Scratch::new("layoutless");
        let bundle = scratch.join("bundle");
        write(&config_in(&bundle), "schema_version = 1\n");
        let restored = restore_layouts(&bundle, &scratch.join("library")).expect("not a failure");
        assert!(restored.is_empty());
    }
}
