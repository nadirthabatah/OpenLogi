//! "Why isn't my mouse working", over MCP.
//!
//! The most useful thing an assistant can do when a device is missing is not
//! to guess. `roadie doctor` already works out whether the cause is
//! permissions, an empty desk, or something else, and produces steps a person
//! can follow; this hands the model the same findings as structured data so it
//! can read them out, in order, and check afterwards.
//!
//! The steps go out as text meant for a person, not as commands for the model
//! to run. Installing udev rules and granting Input Monitoring are things only
//! the human at the machine can do — this tool's job is to make sure they are
//! told the right ones.

use serde_json::{Value, json};

use super::{no_arguments_schema, rendered};
use crate::cmd::doctor::{Check, Verdict};

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![json!({
        "name": "diagnose",
        "description": "Work out why peripherals are not being found, and get the steps \
            that fix it. Use this whenever a device the person says is plugged in does \
            not appear in list_peripherals — the usual cause is a permission this \
            program does not have, not a missing device. The steps are for the person \
            to carry out, not for you: only they can install system rules or grant \
            access. Read them out in the order given, then run this again to check.",
        "inputSchema": no_arguments_schema(),
    })]
}

/// Run `diagnose`.
pub async fn diagnose() -> Result<String, String> {
    let checks = crate::cmd::doctor::examine().await;
    let problems: Vec<&Check> = checks
        .iter()
        .filter(|check| matches!(check.verdict, Verdict::Problem { .. }))
        .collect();

    // The same rendering `roadie doctor --json` prints, so a script and an
    // assistant diagnosing the same machine cannot be told different things.
    let findings: Vec<Value> = checks.iter().map(crate::cmd::doctor::check_json).collect();
    let steps: Vec<Value> = problems
        .iter()
        .filter_map(|check| match &check.verdict {
            Verdict::Problem { fix, .. } => Some(json!({
                "about": check.name,
                "steps": fix,
            })),
            _ => None,
        })
        .collect();

    rendered(&json!({
        "checks": findings,
        "problems": problems.len(),
        "what_the_person_should_do": steps,
        "note": if problems.is_empty() {
            "Nothing is wrong with device access. If a device is still missing, it is \
             more likely unplugged, or not yet paired with the operating system."
        } else {
            "These steps are for the person at the machine to carry out; you cannot do \
             them. Read them out in order — the first problem often causes the rest — \
             then call diagnose again to check."
        },
    }))
}

#[cfg(test)]
mod tests {
    use crate::cmd::doctor::{Check, Verdict};

    use crate::cmd::doctor::check_json as describe;

    use super::tools;

    #[test]
    fn each_state_is_named_in_a_word_a_model_can_branch_on() {
        let cases = [
            (Verdict::Fine("all good".to_owned()), "ok"),
            (Verdict::Undetermined("cannot say".to_owned()), "note"),
            (
                Verdict::Problem {
                    detail: "broken".to_owned(),
                    fix: vec!["do a thing".to_owned()],
                },
                "problem",
            ),
        ];
        for (verdict, expected) in cases {
            let entry = describe(&Check {
                name: "Something",
                verdict,
            });
            assert_eq!(entry["state"], expected);
            assert_eq!(entry["check"], "Something");
            assert!(entry["detail"].as_str().is_some_and(|d| !d.is_empty()));
        }
    }

    /// The steps are things only the person at the machine can do. A model
    /// that reads "install the udev rules" as an instruction to itself will
    /// either fail or, worse, try.
    #[test]
    fn the_description_tells_the_model_the_steps_are_not_for_it() {
        let catalog = tools();
        let description = catalog[0]["description"].as_str().expect("a description");
        assert!(description.contains("not for you"), "{description}");
        assert!(description.contains("list_peripherals"), "{description}");
    }
}
