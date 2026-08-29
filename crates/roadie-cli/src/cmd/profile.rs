//! `roadie profile` — export, inspect and import portable profiles.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::bundle;
use crate::profile;
use crate::spoken::counted;

/// Exit status for "the profile was refused because it carries actions that
/// would run something" — distinct from a read or parse failure, so a script
/// can tell the two apart.
const UNTRUSTED: u8 = 3;

/// Exit status for "the configuration landed but the layouts did not".
///
/// Its own status because it is neither success nor a clean failure: half the
/// setup is in place, and a script that treated it as either would be wrong.
const PARTIAL: u8 = 4;

#[derive(Debug, Subcommand)]
pub enum ProfileCmd {
    /// Save this machine's setup somewhere you can carry it.
    ///
    /// A path ending in `.toml` writes the configuration alone, as one file.
    /// Any other path writes a *bundle* directory carrying the configuration
    /// and every saved Stream Deck layout, icons included — the whole setup.
    /// Either way the command says which it wrote.
    Export(PathArgs),
    /// Show what a profile contains, and any actions that would run something,
    /// without applying it.
    Inspect(PathArgs),
    /// Apply a saved setup, backing up the current configuration first.
    ///
    /// Takes either form `export` writes.
    Import(ImportArgs),
}

#[derive(Debug, Args)]
pub struct PathArgs {
    /// The profile file.
    pub file: PathBuf,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// The profile file.
    pub file: PathBuf,
    /// Accept actions that run a program or type text.
    ///
    /// Without this, a profile carrying any such action is refused and
    /// nothing is written. Inspect it first.
    #[arg(long)]
    pub accept_actions: bool,
}

impl ProfileCmd {
    /// Run the chosen profile operation.
    ///
    /// # Errors
    ///
    /// Propagates read, write and parse failures. A profile refused for
    /// carrying untrusted actions is not an error: it exits [`UNTRUSTED`]
    /// after printing what it found.
    pub fn run(self) -> Result<ExitCode> {
        match self {
            Self::Export(args) => return export(&args.file),
            Self::Inspect(args) => {
                let source = profile_file(&args.file);
                let (config, findings) = profile::inspect(&source)?;
                println!("profile: {}", source.display());
                println!("  schema version: {}", config.schema_version);
                println!("  configured devices: {}", config.devices.len());
                if findings.is_empty() {
                    println!("  no actions that would run a program or type text");
                } else {
                    println!(
                        "  {} that would run a program or type text:",
                        counted(findings.len(), "action", "actions")
                    );
                    for finding in &findings {
                        println!("    {finding}");
                    }
                    println!(
                        "  Importing this profile needs --accept-actions, which is your \
                         decision to trust its source."
                    );
                }
            }
            Self::Import(args) => {
                match profile::import(&profile_file(&args.file), args.accept_actions) {
                    Ok(imported) => {
                        println!("setup imported from {}", args.file.display());
                        match &imported.backup {
                            Some(backup) => {
                                println!("  previous configuration saved to {}", backup.display());
                            }
                            None => println!(
                                "  this machine had no configuration yet, so there was nothing \
                             to back up"
                            ),
                        }
                        match restore_layouts(&args.file) {
                            Ok(names) if !names.is_empty() => {
                                println!(
                                    "  {} restored: {}",
                                    counted(names.len(), "layout", "layouts"),
                                    names.join(", ")
                                );
                            }
                            Ok(_) => {}
                            // A failure here is worth saying out loud and is not
                            // worth undoing the configuration import over: the
                            // configuration landed, and the person needs to know
                            // exactly which half did not.
                            Err(error) => {
                                eprintln!(
                                    "  the configuration was imported, but the layouts were not: {error}"
                                );
                                return Ok(ExitCode::from(PARTIAL));
                            }
                        }
                        if !imported.accepted.is_empty() {
                            // The verb leads, so nothing after the count has to
                            // agree with it: "1 action ... were accepted" is what
                            // a count wedged into the middle of a sentence gives.
                            println!(
                                "  accepted {} that would run a program or type text:",
                                counted(imported.accepted.len(), "action", "actions")
                            );
                            for finding in &imported.accepted {
                                println!("    {finding}");
                            }
                        }
                        println!(
                            "  The running agent still holds the old configuration; restart it, \
                         or ask it to reload, for this to take effect."
                        );
                    }
                    Err(error @ profile::ProfileError::UntrustedActions { .. }) => {
                        eprintln!("{error}");
                        return Ok(ExitCode::from(UNTRUSTED));
                    }
                    Err(other) => return Err(other.into()),
                }
            }
        }
        Ok(ExitCode::SUCCESS)
    }
}

/// Where the configuration actually is, given what the person named.
///
/// A bundle keeps it in `config.toml` inside the directory; a profile file is
/// itself the configuration.
fn profile_file(named: &Path) -> PathBuf {
    if bundle::is_bundle(named) {
        bundle::config_in(named)
    } else {
        named.to_path_buf()
    }
}

/// `roadie profile export`.
///
/// Says which of the two things it wrote, and how to get the other. The rule
/// is easy to state and easy to forget, and someone who wanted their layouts
/// and got a file without them should find that out now rather than on the
/// machine they moved to.
fn export(named: &Path) -> Result<ExitCode> {
    if !bundle::is_bundle(named) {
        profile::export(named)?;
        println!("configuration written to {}", named.display());
        println!(
            "That is the configuration alone. To carry your Stream Deck layouts too, \
             export to a folder instead: roadie profile export my-setup"
        );
        println!(
            "Apply it on another machine with: roadie profile import {}",
            crate::spoken::shell_argument(&named.to_string_lossy())
        );
        return Ok(ExitCode::SUCCESS);
    }

    std::fs::create_dir_all(named)
        .with_context(|| format!("failed to create {}", named.display()))?;
    profile::export(&bundle::config_in(named))?;
    let library = crate::cmd::streamdeck::layout_library()?;
    let gathered = bundle::gather_layouts(&library, named)?;

    println!("setup written to {}", named.display());
    println!("  configuration: config.toml");
    if gathered.carried.is_empty() {
        println!("  no saved layouts to carry (roadie streamdeck layouts lists them)");
    } else {
        println!(
            "  {}: {}",
            counted(gathered.carried.len(), "layout", "layouts"),
            gathered.carried.join(", ")
        );
    }
    // A linked folder is not followed, so its contents did not travel. Said,
    // because the alternative is discovering it as blank keys on the machine
    // the bundle was carried to — with the bundle itself looking complete.
    if !gathered.skipped_links.is_empty() {
        println!(
            "  {} not followed:",
            counted(
                gathered.skipped_links.len(),
                "linked folder inside your layouts folder was",
                "linked folders inside your layouts folder were"
            )
        );
        for link in &gathered.skipped_links {
            println!("    {}", link.display());
        }
        println!(
            "  Each points somewhere else on this machine, so nothing inside them \
             travelled. Copy what should into the layouts folder itself."
        );
    }
    // Exporting again over an earlier bundle copies in without deleting, so a
    // layout removed since is still in that folder. Said out loud rather than
    // removed: this is a path the person named, and quietly deleting inside it
    // is not a risk worth taking for tidiness.
    if !gathered.left_over.is_empty() {
        println!(
            "  also still in that folder from an earlier export, and no longer in your \
             library: {}",
            gathered.left_over.join(", ")
        );
        println!("  delete those files yourself if you do not want them carried.");
    }
    println!(
        "Copy the whole folder to another machine and apply it with: \
         roadie profile import {}",
        crate::spoken::shell_argument(&named.to_string_lossy())
    );
    Ok(ExitCode::SUCCESS)
}

/// Put a bundle's layouts back into this machine's library.
fn restore_layouts(named: &Path) -> Result<Vec<String>> {
    if !bundle::is_bundle(named) {
        return Ok(Vec::new());
    }
    let library = crate::cmd::streamdeck::layout_library()?;
    bundle::restore_layouts(named, &library)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::profile_file;

    /// Where the configuration lives decides what `import` reads and what
    /// `inspect` reports. Getting it wrong reads the wrong file, or reports a
    /// path that is not the one that was applied — and both look like the
    /// command worked.
    #[test]
    fn a_toml_path_is_itself_the_configuration() {
        assert_eq!(
            profile_file(Path::new("setup.toml")),
            PathBuf::from("setup.toml")
        );
        assert_eq!(
            profile_file(Path::new("/backups/mine.TOML")),
            PathBuf::from("/backups/mine.TOML"),
            "the extension is matched whatever its case, as everywhere else"
        );
    }

    #[test]
    fn any_other_path_keeps_its_configuration_inside_the_bundle() {
        assert_eq!(
            profile_file(Path::new("my-setup")),
            PathBuf::from("my-setup/config.toml")
        );
        assert_eq!(
            profile_file(Path::new("/backups/desk")),
            PathBuf::from("/backups/desk/config.toml")
        );
    }

    /// A trailing separator is how a shell completes a directory name, so it
    /// arrives this way often. It must not change what the path means.
    #[test]
    fn a_trailing_separator_still_names_a_bundle() {
        assert_eq!(
            profile_file(Path::new("my-setup/")),
            PathBuf::from("my-setup/config.toml")
        );
    }
}
