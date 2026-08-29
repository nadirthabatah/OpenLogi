//! `openlogi profile` — export, inspect and import portable profiles.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::profile;

/// Exit status for "the profile was refused because it carries actions that
/// would run something" — distinct from a read or parse failure, so a script
/// can tell the two apart.
const UNTRUSTED: u8 = 3;

#[derive(Debug, Subcommand)]
pub enum ProfileCmd {
    /// Write the current configuration to a file you can carry to another
    /// machine.
    Export(PathArgs),
    /// Show what a profile contains, and any actions that would run something,
    /// without applying it.
    Inspect(PathArgs),
    /// Apply a profile, backing up the current configuration first.
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
            Self::Export(args) => {
                profile::export(&args.file)?;
                println!("profile written to {}", args.file.display());
                println!(
                    "Copy it to another machine and apply it with: openlogi profile import {}",
                    args.file.display()
                );
            }
            Self::Inspect(args) => {
                let (config, findings) = profile::inspect(&args.file)?;
                println!("profile: {}", args.file.display());
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
            Self::Import(args) => match profile::import(&args.file, args.accept_actions) {
                Ok(imported) => {
                    println!("profile imported from {}", args.file.display());
                    match &imported.backup {
                        Some(backup) => {
                            println!("  previous configuration saved to {}", backup.display());
                        }
                        None => println!(
                            "  this machine had no configuration yet, so there was nothing \
                             to back up"
                        ),
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
            },
        }
        Ok(ExitCode::SUCCESS)
    }
}
