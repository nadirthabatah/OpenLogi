//! `openlogi profile` — export, inspect and import portable profiles.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::bundle;
use crate::profile;

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
                        "  {} action(s) that would run a program or type text:",
                        findings.len()
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
                                    "  {} layout(s) restored: {}",
                                    names.len(),
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
                            println!(
                                "  {} action(s) that run a program or type text were accepted:",
                                imported.accepted.len()
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

/// `openlogi profile export`.
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
             export to a folder instead: openlogi profile export my-setup"
        );
        println!(
            "Apply it on another machine with: openlogi profile import {}",
            named.display()
        );
        return Ok(ExitCode::SUCCESS);
    }

    std::fs::create_dir_all(named)
        .with_context(|| format!("failed to create {}", named.display()))?;
    profile::export(&bundle::config_in(named))?;
    let library = crate::cmd::streamdeck::layout_library()?;
    let layouts = bundle::gather_layouts(&library, named)?;

    println!("setup written to {}", named.display());
    println!("  configuration: config.toml");
    if layouts.is_empty() {
        println!("  no saved layouts to carry (openlogi streamdeck layouts lists them)");
    } else {
        println!("  {} layout(s): {}", layouts.len(), layouts.join(", "));
    }
    println!(
        "Copy the whole folder to another machine and apply it with: \
         openlogi profile import {}",
        named.display()
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
