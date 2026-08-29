//! Carrying a whole configuration to another machine.
//!
//! Importing is deliberately asymmetric with the CLI: `openlogi profile
//! import --accept-actions` lets a person accept a profile that runs
//! programs, and this tool has no equivalent. Deciding that a profile someone
//! sent is trustworthy is a judgement about provenance the model cannot make
//! and should not be asked to — so a profile carrying such actions is
//! reported here and applied, if at all, by a person at a terminal.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};
use crate::profile::{self, ProfileError};

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    let file_argument = json!({
        "type": "string",
        "description": "Absolute path to the profile file.",
    });
    vec![
        json!({
            "name": "export_profile",
            "description": "Write the whole current configuration to a file that can be \
                carried to another computer and imported there.",
            "inputSchema": json!({
                "type": "object",
                "properties": { "file": file_argument },
                "required": ["file"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "inspect_profile",
            "description": "Show what a profile file contains without applying it, \
                including any actions that would run a program or type text.",
            "inputSchema": json!({
                "type": "object",
                "properties": { "file": file_argument },
                "required": ["file"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "import_profile",
            "description": "Apply a profile file, backing up the current configuration \
                first. A profile carrying actions that run a program or type text is \
                refused here and must be applied by a person with `openlogi profile \
                import --accept-actions`, because trusting its source is their call.",
            "inputSchema": json!({
                "type": "object",
                "properties": { "file": file_argument },
                "required": ["file"],
                "additionalProperties": false,
            }),
        }),
        json!({
            "name": "config_location",
            "description": "Where this machine keeps its configuration file. Useful \
                when a person wants to back it up or edit it directly.",
            "inputSchema": no_arguments_schema(),
        }),
    ]
}

/// Read the `file` argument.
fn file_argument(arguments: &Value) -> Result<PathBuf, String> {
    arguments
        .get("file")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "the `file` argument must be a path string".to_string())
}

/// Write the live configuration out as a profile.
pub fn export_profile(arguments: &Value) -> Result<String, String> {
    let path = file_argument(arguments)?;
    profile::export(&path).map_err(|error| error.to_string())?;
    Ok(format!(
        "the current configuration was written to {}. Copy it to the other machine and \
         import it there.",
        path.display()
    ))
}

/// Describe a profile without applying it.
pub fn inspect_profile(arguments: &Value) -> Result<String, String> {
    let path = file_argument(arguments)?;
    let (config, findings) = profile::inspect(&path).map_err(|error| error.to_string())?;
    let listed: Vec<Value> = findings
        .iter()
        .map(|finding| {
            json!({
                "location": finding.location,
                "action": finding.action,
                "detail": finding.detail,
            })
        })
        .collect();
    rendered(&json!({
        "file": path.display().to_string(),
        "schema_version": config.schema_version,
        "configured_devices": config.devices.len(),
        "actions_that_run_something": listed,
        "importable_here": findings.is_empty(),
    }))
}

/// Apply a profile, refusing one that would run something.
pub fn import_profile(arguments: &Value) -> Result<String, String> {
    let path = file_argument(arguments)?;
    match profile::import(&path, false) {
        Ok(imported) => Ok(format!(
            "profile applied from {}. {} Ask the agent to reload its configuration for it \
             to take effect.",
            path.display(),
            match &imported.backup {
                Some(backup) => format!(
                    "The previous configuration was saved to {}, so this is reversible.",
                    backup.display()
                ),
                None => "This machine had no configuration yet, so there was nothing to back up."
                    .to_string(),
            }
        )),
        Err(error @ ProfileError::UntrustedActions { .. }) => Err(format!(
            "{error}\n\nNothing was changed. Whether this profile's source is trustworthy \
             is a decision for the person at the keyboard, not for me — they can apply it \
             with: openlogi profile import {} --accept-actions",
            crate::spoken::shell_argument(&path.to_string_lossy())
        )),
        Err(other) => Err(other.to_string()),
    }
}

/// Where the configuration lives on this machine.
pub fn config_location() -> Result<String, String> {
    let path = openlogi_core::paths::config_path()
        .map_err(|error| format!("could not resolve the configuration path: {error}"))?;
    Ok(format!(
        "the configuration file is {} (this layout is used on every platform, including \
         Windows and macOS)",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{file_argument, import_profile, tools};

    #[test]
    fn a_missing_file_argument_is_refused() {
        file_argument(&json!({})).expect_err("the path is required");
        file_argument(&json!({ "file": 7 })).expect_err("the path is a string");
    }

    #[test]
    fn import_is_not_offered_a_way_to_accept_risky_actions() {
        // The absence of this argument is the safety property, so it is
        // asserted rather than left to the reader of the schema.
        let tools = tools();
        let import = tools
            .iter()
            .find(|tool| tool["name"] == "import_profile")
            .expect("import_profile is advertised");
        let properties = import["inputSchema"]["properties"]
            .as_object()
            .expect("an object schema");
        assert_eq!(
            properties.keys().collect::<Vec<_>>(),
            vec!["file"],
            "import_profile must expose no way for a model to accept risky actions"
        );
    }

    #[test]
    fn importing_a_profile_that_runs_something_is_refused_with_the_human_path_named() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("hostile.toml");
        let mut config = openlogi_core::config::Config::default();
        config.set_keyboard_binding(
            "F13".parse().expect("valid"),
            Some(openlogi_core::binding::Action::RunShellCommand(
                "wipe".into(),
            )),
        );
        std::fs::write(&path, toml::to_string_pretty(&config).expect("serializes"))
            .expect("writes");

        let message = import_profile(&json!({ "file": path.to_str().expect("utf-8") }))
            .expect_err("must refuse");
        assert!(message.contains("RunShellCommand"));
        assert!(message.contains("Nothing was changed"));
        assert!(message.contains("--accept-actions"));
    }
}
