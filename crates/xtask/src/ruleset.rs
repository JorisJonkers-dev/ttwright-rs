// SPDX-License-Identifier: GPL-3.0-only
//! Builds the ruleset body that `cargo xtask apply-ruleset` writes, without touching the network.
//!
//! The template (`.github/rulesets/main.json`) has an empty `bypass_actors`. A PUT replaces the whole ruleset, so
//! a naive update would wipe the GitHub Apps that deploy repositories keep as bypass actors. An update therefore
//! keeps the existing actors and adds the template's missing ones; removing any requires `--allow-bypass-removal`.
//! An actor's identity is `actor_type` plus `actor_id`; the existing `bypass_mode` wins.

use serde_json::Value;

/// The body to PUT, plus the lines to print before the write (removals and mode changes are shown, not silent).
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub body: Value,
    pub notices: Vec<String>,
}

/// Plans an update of the existing ruleset `current` from the desired `template`.
pub fn plan_update(current: &Value, template: &Value, allow_removal: bool) -> Result<Plan, String> {
    let have = current
        .get("bypass_actors")
        .and_then(Value::as_array)
        .ok_or("unexpected ruleset body; refusing to write")?;
    let template_obj = template
        .as_object()
        .ok_or("the ruleset template is not a JSON object")?;
    let wanted: Vec<Value> = template_obj
        .get("bypass_actors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let merged: Vec<Value> = if allow_removal {
        wanted
    } else {
        let mut merged = have.clone();
        merged.extend(
            wanted
                .iter()
                .filter(|t| !have.iter().any(|h| same_actor(h, t)))
                .cloned(),
        );
        merged
    };

    let mut notices = Vec::new();
    for old in have {
        if !merged.iter().any(|m| same_actor(m, old)) {
            notices.push(format!(
                "BYPASS REMOVE {}:{}",
                show(&old["actor_type"]),
                show(&old["actor_id"])
            ));
        }
    }
    for old in have {
        for new in merged.iter().filter(|m| same_actor(m, old)) {
            if new["bypass_mode"] != old["bypass_mode"] {
                notices.push(format!(
                    "BYPASS MODE {}:{} {} -> {}",
                    show(&new["actor_type"]),
                    show(&new["actor_id"]),
                    show(&old["bypass_mode"]),
                    show(&new["bypass_mode"])
                ));
            }
        }
    }

    let mut body = template.clone();
    body["bypass_actors"] = Value::Array(merged);
    Ok(Plan { body, notices })
}

/// One "bypass actor" line per actor of a planned body, as printed before the write.
pub fn describe_actors(body: &Value) -> Vec<String> {
    body["bypass_actors"]
        .as_array()
        .map(|actors| {
            actors
                .iter()
                .map(|a| {
                    format!(
                        "bypass actor {}:{} ({})",
                        show(&a["actor_type"]),
                        show(&a["actor_id"]),
                        show(&a["bypass_mode"])
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn same_actor(a: &Value, b: &Value) -> bool {
    a["actor_type"] == b["actor_type"] && a["actor_id"] == b["actor_id"]
}

/// Text of a JSON scalar as jq's `\(...)` interpolation prints it: strings without quotes.
fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn template() -> Value {
        json!({
            "name": "Main",
            "target": "branch",
            "bypass_actors": [],
            "rules": [{ "type": "deletion" }]
        })
    }

    fn actor(kind: &str, id: u64, mode: &str) -> Value {
        json!({ "actor_id": id, "actor_type": kind, "bypass_mode": mode })
    }

    #[test]
    fn default_update_keeps_existing_actors_and_adds_missing_template_actors() {
        let mut tpl = template();
        tpl["bypass_actors"] = json!([actor("Integration", 2, "always")]);
        let current = json!({ "bypass_actors": [actor("Integration", 1, "always")] });

        let plan = plan_update(&current, &tpl, false).unwrap();

        assert!(plan.notices.is_empty());
        assert_eq!(
            plan.body["bypass_actors"],
            json!([
                actor("Integration", 1, "always"),
                actor("Integration", 2, "always")
            ])
        );
        assert_eq!(plan.body["name"], "Main");
        assert_eq!(plan.body["rules"], json!([{ "type": "deletion" }]));
    }

    #[test]
    fn existing_bypass_mode_wins_for_an_actor_the_template_also_lists() {
        let mut tpl = template();
        tpl["bypass_actors"] = json!([actor("Team", 7, "always")]);
        let current = json!({ "bypass_actors": [actor("Team", 7, "pull_request")] });

        let plan = plan_update(&current, &tpl, false).unwrap();

        assert_eq!(
            plan.body["bypass_actors"],
            json!([actor("Team", 7, "pull_request")])
        );
        assert!(plan.notices.is_empty());
    }

    #[test]
    fn default_update_never_reports_a_removal() {
        let current = json!({ "bypass_actors": [actor("Integration", 1, "always")] });

        let plan = plan_update(&current, &template(), false).unwrap();

        assert_eq!(
            plan.body["bypass_actors"],
            json!([actor("Integration", 1, "always")])
        );
        assert!(plan.notices.is_empty());
    }

    #[test]
    fn allow_removal_uses_the_template_list_and_reports_removals_and_mode_changes() {
        let mut tpl = template();
        tpl["bypass_actors"] = json!([
            actor("Integration", 2, "always"),
            actor("Team", 7, "always")
        ]);
        let current = json!({
            "bypass_actors": [actor("Integration", 1, "always"), actor("Team", 7, "pull_request")]
        });

        let plan = plan_update(&current, &tpl, true).unwrap();

        assert_eq!(
            plan.body["bypass_actors"],
            json!([
                actor("Integration", 2, "always"),
                actor("Team", 7, "always")
            ])
        );
        assert_eq!(
            plan.notices,
            vec![
                "BYPASS REMOVE Integration:1".to_string(),
                "BYPASS MODE Team:7 pull_request -> always".to_string(),
            ]
        );
    }

    #[test]
    fn refuses_a_current_ruleset_without_a_bypass_array() {
        let current = json!({ "name": "Main" });

        let err = plan_update(&current, &template(), false).unwrap_err();

        assert_eq!(err, "unexpected ruleset body; refusing to write");
    }

    #[test]
    fn describe_actors_lists_every_actor_of_the_body() {
        let body = json!({ "bypass_actors": [actor("Integration", 1, "always")] });

        assert_eq!(
            describe_actors(&body),
            vec!["bypass actor Integration:1 (always)".to_string()]
        );
    }

    #[test]
    fn committed_template_is_the_main_ruleset_with_the_pipeline_check() {
        let template: Value =
            serde_json::from_str(include_str!("../../../.github/rulesets/main.json")).unwrap();

        assert_eq!(template["name"], "Main");
        assert_eq!(template["bypass_actors"], json!([]));
        let checks = template["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == "required_status_checks")
            .unwrap();
        assert!(
            checks["parameters"]["required_status_checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["context"] == "Pipeline Complete")
        );
    }
}
