//! Where a deck's layouts live.
//!
//! Nothing a Stream Deck shows survives unplugging it, so a layout file is
//! where a deck's appearance actually lives. Until now that file lived
//! wherever the person happened to save it, which made two things awkward: you
//! had to remember the path every time, and nothing could gather your layouts
//! up to move them to another computer — the promise this whole project is
//! built around.
//!
//! So layouts get a home: `layouts/` inside the configuration directory, one
//! `.toml` per layout, named by whatever you call it. Icons sit beside the
//! layout that names them, because a relative image path is resolved against
//! its own file — so a layout and its pictures travel as one thing.
//!
//! An explicit path still works everywhere a name does. The library is a
//! convenience and a place for the profile bundle to look, never a cage: a
//! layout in a git repository next to the project it belongs to is a
//! perfectly good place for it to live.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The directory layouts live in.
///
/// # Errors
///
/// Fails when the configuration directory cannot be determined.
pub fn directory() -> Result<PathBuf> {
    Ok(openlogi_core::paths::config_dir()
        .context("failed to locate the configuration directory")?
        .join("layouts"))
}

/// Turn a layout argument into a path.
///
/// A bare name resolves inside the library; anything that looks like a path is
/// used as given. See [`looks_like_a_path`] for where the line is drawn.
///
/// # Errors
///
/// Fails when a bare name is given and the configuration directory cannot be
/// determined.
pub fn resolve(argument: &str) -> Result<PathBuf> {
    if looks_like_a_path(argument) {
        return Ok(PathBuf::from(argument));
    }
    Ok(directory()?.join(format!("{argument}.toml")))
}

/// Refuse anything that is not a bare library name.
///
/// The command line takes a name *or* a path, because a person who types a
/// path means that path. The MCP tools must not: their argument comes from a
/// language model, which can be steered by whatever it has been reading —
/// a web page, a document, a comment on a pull request. `library::resolve`
/// would happily treat `../../../../etc/thing` as a path and write there, and
/// "the model was asked nicely by a web page" is not a story anyone wants to
/// hear about why a file was overwritten.
///
/// So the model-driven surface is names only, and this is the boundary that
/// enforces it. A name is one path component: no separators, no `..`, no
/// leading dot, and not empty.
///
/// # Errors
///
/// A message naming what was wrong with it, phrased for the model to correct
/// itself rather than retry the same thing.
pub fn resolve_saved_name(name: &str) -> Result<PathBuf> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!(
            "a layout name is needed; list_layouts gives the ones saved here"
        ));
    }
    let is_a_bare_name = !trimmed.contains(['/', '\\'])
        && trimmed != ".."
        && trimmed != "."
        && !trimmed.starts_with('.')
        && !trimmed.contains('\0')
        && Path::new(trimmed).components().count() == 1;
    if !is_a_bare_name {
        return Err(anyhow::anyhow!(
            "\"{name}\" is not a layout name. These tools address layouts saved on \
             this machine by name — list_layouts gives them — and cannot reach a \
             path elsewhere on the disk."
        ));
    }
    Ok(directory()?.join(format!("{trimmed}.toml")))
}

/// Whether an argument names a file on disk rather than a layout in the
/// library.
///
/// Anything carrying a separator, a `.toml` extension, or a leading `.` or
/// `~` is a path. The rule is deliberately generous towards paths: someone who
/// typed a path and gets told "no layout called ./deck.toml" has been handed a
/// puzzle, whereas someone who typed a name that happens to contain a dot gets
/// a file-not-found naming the file they meant.
#[must_use]
pub fn looks_like_a_path(argument: &str) -> bool {
    argument.contains(std::path::MAIN_SEPARATOR)
        || argument.contains('/')
        || has_toml_extension(argument)
        || argument.starts_with('.')
        || argument.starts_with('~')
}

/// Whether an argument ends in a `.toml` extension, in any case.
///
/// Case-insensitive because a file named `Deck.TOML` is still a path, and
/// treating it as a library name would look for `Deck.TOML.toml` and report
/// that missing — a confusing answer to a file that is right there.
fn has_toml_extension(argument: &str) -> bool {
    Path::new(argument)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
}

/// Every layout in the library, by name, in a stable order.
///
/// An unreadable directory is not an error: a machine that has never saved a
/// layout has no such directory, and that is an empty library rather than a
/// fault.
///
/// # Errors
///
/// Fails only when the configuration directory cannot be determined.
pub fn list() -> Result<Vec<String>> {
    let Ok(entries) = std::fs::read_dir(directory()?) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| name_of(&entry.path()))
        .collect();
    names.sort();
    Ok(names)
}

/// The library name of a path, when it is a layout file.
fn name_of(path: &Path) -> Option<String> {
    if path.extension()? != "toml" {
        return None;
    }
    Some(path.file_stem()?.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{looks_like_a_path, name_of, resolve_saved_name};

    /// The boundary the MCP tools sit behind. Its argument comes from a
    /// language model, and a model can be steered by whatever it has been
    /// reading — so anything that could reach outside the library is refused
    /// here rather than resolved into a path and written to.
    #[test]
    fn a_saved_name_cannot_reach_outside_the_library() {
        for escape in [
            "../../../../etc/passwd",
            "..",
            ".",
            "../secrets",
            "sub/deck",
            "sub\\deck",
            ".hidden",
            "",
            "   ",
            "/etc/passwd",
        ] {
            resolve_saved_name(escape)
                .map(|path| path.display().to_string())
                .expect_err(&format!("{escape:?} must be refused"));
        }
    }

    #[test]
    fn an_ordinary_saved_name_resolves_inside_the_library() {
        let path = resolve_saved_name("streaming").expect("an ordinary name");
        assert!(
            path.ends_with("layouts/streaming.toml"),
            "{}",
            path.display()
        );
        // Surrounding space is a typo, not an escape; it is trimmed.
        let trimmed = resolve_saved_name("  streaming  ").expect("trimmed");
        assert_eq!(trimmed, path);
    }

    /// The refusal has to say what to do instead, or a model retries it.
    #[test]
    fn the_refusal_points_at_the_tool_that_lists_names() {
        let message = resolve_saved_name("../x")
            .map(|_| String::new())
            .expect_err("refused")
            .to_string();
        assert!(message.contains("list_layouts"), "{message}");
    }

    /// A bare name is the whole point of the library.
    #[test]
    fn a_bare_name_is_not_a_path() {
        for name in ["streaming", "my-deck", "deck2", "work_setup"] {
            assert!(!looks_like_a_path(name), "{name} should be a library name");
        }
    }

    /// Generous towards paths on purpose: someone who typed a path and is told
    /// "no layout called ./deck.toml" has been handed a puzzle.
    #[test]
    fn anything_that_looks_like_a_path_is_left_alone() {
        for path in [
            "deck.toml",
            "./deck.toml",
            "../decks/work.toml",
            "/home/me/deck.toml",
            "~/deck.toml",
            "decks/work",
            "Deck.TOML",
        ] {
            assert!(
                looks_like_a_path(path),
                "{path} should be treated as a path"
            );
        }
    }

    #[test]
    fn only_toml_files_are_layouts() {
        assert_eq!(
            name_of(&PathBuf::from("/x/streaming.toml")),
            Some("streaming".to_owned())
        );
        assert_eq!(name_of(Path::new("/x/icon.png")), None);
        assert_eq!(name_of(Path::new("/x/README")), None);
    }

    /// Icons sit beside the layout that names them, so a directory listing
    /// must not offer `camera` as a layout because `camera.png` is there.
    #[test]
    fn an_icon_beside_a_layout_is_not_itself_a_layout() {
        assert_eq!(name_of(Path::new("/x/icons/camera.png")), None);
    }
}
