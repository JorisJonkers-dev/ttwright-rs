// SPDX-License-Identifier: GPL-3.0-only
//! `cargo xtask`: repository maintenance commands. Not shipped.
//!
//! `cargo xtask apply-ruleset [--allow-bypass-removal] [<owner/repo>]` applies the org-standard branch ruleset
//! (`.github/rulesets/main.json`) to a repository through the `gh` CLI. It creates the "Main" ruleset when the
//! repository has none and otherwise updates it in place, keeping the bypass actors (see `ruleset`). Without
//! `<owner/repo>` it uses the repository of the current directory.
//!
//! The template is located from `CARGO_MANIFEST_DIR` at compile time, so the command only works through
//! `cargo xtask` run from this checkout; a copied or installed binary cannot find it.

mod ruleset;

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::thread;

use serde_json::Value;

const USAGE: &str = "usage: cargo xtask apply-ruleset [--allow-bypass-removal] [<owner/repo>]";

/// Runs `gh`. `GhCli` calls the real CLI; tests inject a fake.
trait Gh {
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String>;
}

struct GhCli;

impl Gh for GhCli {
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        let mut child = Command::new("gh")
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run gh: {e}"))?;
        // Written on a thread, so a gh that exits early (and closes its stdin) surfaces as its own stderr below,
        // not as a broken-pipe error from this side.
        let writer = match (stdin, child.stdin.take()) {
            (Some(input), Some(mut pipe)) => {
                let input = input.to_owned();
                Some(thread::spawn(move || pipe.write_all(input.as_bytes())))
            }
            _ => None,
        };
        let output = child
            .wait_with_output()
            .map_err(|e| format!("gh did not finish: {e}"))?;
        let written = writer.map(|handle| {
            handle
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("the writer thread panicked")))
        });
        if !output.status.success() {
            return Err(format!(
                "gh {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        if let Some(Err(e)) = written {
            return Err(format!("cannot write to gh: {e}"));
        }
        String::from_utf8(output.stdout).map_err(|e| format!("gh printed non-UTF-8 output: {e}"))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Task {
    ApplyRuleset {
        allow_bypass_removal: bool,
        repo: Option<String>,
    },
}

fn parse(args: &[String]) -> Result<Task, String> {
    match args.split_first() {
        None => Err(USAGE.to_string()),
        Some((command, rest)) if command == "apply-ruleset" => parse_apply_ruleset(rest),
        Some((command, _)) => Err(format!("unknown command: {command}\n{USAGE}")),
    }
}

fn parse_apply_ruleset(args: &[String]) -> Result<Task, String> {
    let mut allow_bypass_removal = false;
    let mut positional = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--allow-bypass-removal" => allow_bypass_removal = true,
            flag if flag.starts_with('-') => {
                return Err(format!("unknown argument: {flag}\n{USAGE}"));
            }
            value => positional.push(value.to_string()),
        }
    }
    // As the shell script did: the first positional is the repository, later ones are ignored, and an empty one
    // means the current repository.
    let repo = positional.into_iter().next().filter(|r| !r.is_empty());
    if let Some(repo) = &repo {
        validate_repo(repo).map_err(|e| format!("{e}\n{USAGE}"))?;
    }
    Ok(Task::ApplyRuleset {
        allow_bypass_removal,
        repo,
    })
}

/// Accepts `<owner>/<repo>` where each part is letters, digits, `.`, `-` or `_`, and neither part is `.` or
/// contains `..`. The name goes into gh API paths, so nothing else gets through.
fn validate_repo(repo: &str) -> Result<(), String> {
    let valid_part = |part: &str| {
        !part.is_empty()
            && part != "."
            && !part.contains("..")
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    match repo.split('/').collect::<Vec<_>>().as_slice() {
        [owner, name] if valid_part(owner) && valid_part(name) => Ok(()),
        _ => Err(format!(
            "invalid repository {repo:?}: expected <owner>/<repo> with letters, digits, '.', '-' or '_'"
        )),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = match parse(&args) {
        Ok(task) => task,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let result = match task {
        Task::ApplyRuleset {
            allow_bypass_removal,
            repo,
        } => template_text()
            .and_then(|template| apply_ruleset(&GhCli, allow_bypass_removal, repo, &template)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn template_text() -> Result<String, String> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        ".github",
        "rulesets",
        "main.json",
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn apply_ruleset(
    gh: &dyn Gh,
    allow_bypass_removal: bool,
    repo: Option<String>,
    template_text: &str,
) -> Result<(), String> {
    let repo = match repo {
        Some(repo) => repo,
        None => gh
            .run(
                &[
                    "repo",
                    "view",
                    "--json",
                    "nameWithOwner",
                    "-q",
                    ".nameWithOwner",
                ],
                None,
            )?
            .trim()
            .to_string(),
    };
    validate_repo(&repo)?;
    let template: Value = serde_json::from_str(template_text)
        .map_err(|e| format!("the ruleset template is not JSON: {e}"))?;

    match find_main_ruleset_id(gh, &repo)? {
        Some(id) => {
            println!("Updating ruleset 'Main' ({id}) on {repo}");
            let current_text = gh.run(&["api", &format!("/repos/{repo}/rulesets/{id}")], None)?;
            let current: Value = serde_json::from_str(&current_text)
                .map_err(|e| format!("unexpected ruleset body: {e}"))?;
            let plan = ruleset::plan_update(&current, &template, allow_bypass_removal)?;
            for notice in &plan.notices {
                println!("{notice}");
            }
            for line in ruleset::describe_actors(&plan.body) {
                println!("{line}");
            }
            let body = plan.body.to_string();
            gh.run(
                &[
                    "api",
                    "-X",
                    "PUT",
                    &format!("/repos/{repo}/rulesets/{id}"),
                    "--input",
                    "-",
                ],
                Some(&body),
            )?;
        }
        None => {
            println!("Creating ruleset 'Main' on {repo}");
            gh.run(
                &[
                    "api",
                    "-X",
                    "POST",
                    &format!("/repos/{repo}/rulesets"),
                    "--input",
                    "-",
                ],
                Some(template_text),
            )?;
        }
    }
    println!(
        "Done. 'Pipeline Complete' is now the required check on the default branch of {repo}."
    );
    Ok(())
}

/// The id of the repository's own ruleset named "Main": `None` when there is none. A failed or unreadable listing
/// is an error, so a transient failure can never turn into a create. Rulesets inherited from an organisation
/// (`source_type` other than `Repository`) are not this repository's and are ignored.
fn find_main_ruleset_id(gh: &dyn Gh, repo: &str) -> Result<Option<u64>, String> {
    let listing = gh.run(&["api", &format!("/repos/{repo}/rulesets")], None)?;
    let rulesets: Vec<Value> = serde_json::from_str(&listing)
        .map_err(|e| format!("unexpected ruleset listing from gh: {e}"))?;
    let mains: Vec<&Value> = rulesets
        .iter()
        .filter(|r| r["name"] == "Main")
        .filter(|r| r.get("source_type").is_none_or(|s| s == "Repository"))
        .collect();
    match mains.as_slice() {
        [] => Ok(None),
        [only] => only["id"].as_u64().map(Some).ok_or_else(|| {
            format!(
                "the ruleset Main on {repo} has no numeric id: {}",
                only["id"]
            )
        }),
        many => {
            let ids: Vec<String> = many.iter().map(|r| r["id"].to_string()).collect();
            Err(format!(
                "{} rulesets named Main on {repo} (ids {}); delete all but one, then apply again",
                many.len(),
                ids.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::HashMap;

    const LISTING: &str = "api /repos/o/r/rulesets";
    const CURRENT: &str = "api /repos/o/r/rulesets/42";
    const PUT: &str = "api -X PUT /repos/o/r/rulesets/42 --input -";
    const POST: &str = "api -X POST /repos/o/r/rulesets --input -";
    const TEMPLATE: &str = include_str!("../../../.github/rulesets/main.json");

    /// Answers each gh call from a table keyed by its joined arguments, and records every call with its stdin.
    /// A call missing from the table fails the test by returning an error that names it.
    struct FakeGh {
        answers: HashMap<String, Result<String, String>>,
        calls: RefCell<Vec<(String, Option<String>)>>,
    }

    impl FakeGh {
        fn new(answers: &[(&str, Result<&str, &str>)]) -> Self {
            Self {
                answers: answers
                    .iter()
                    .map(|(key, answer)| {
                        (
                            key.to_string(),
                            answer.map(str::to_string).map_err(str::to_string),
                        )
                    })
                    .collect(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn keys(&self) -> Vec<String> {
            self.calls.borrow().iter().map(|(k, _)| k.clone()).collect()
        }

        fn stdin_of(&self, key: &str) -> Option<String> {
            self.calls
                .borrow()
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, stdin)| stdin.clone())
        }

        fn wrote(&self) -> bool {
            self.keys().iter().any(|k| k == PUT || k == POST)
        }
    }

    impl Gh for FakeGh {
        fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
            let key = args.join(" ");
            self.calls
                .borrow_mut()
                .push((key.clone(), stdin.map(str::to_string)));
            self.answers
                .get(&key)
                .cloned()
                .unwrap_or_else(|| Err(format!("unexpected gh call in test: {key}")))
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn apply(gh: &FakeGh, repo: Option<&str>) -> Result<(), String> {
        apply_ruleset(gh, false, repo.map(str::to_string), TEMPLATE)
    }

    fn actor(kind: &str, id: u64, mode: &str) -> Value {
        json!({ "actor_id": id, "actor_type": kind, "bypass_mode": mode })
    }

    #[test]
    fn no_command_is_a_usage_error() {
        assert_eq!(parse(&[]), Err(USAGE.to_string()));
    }

    #[test]
    fn unknown_command_is_refused() {
        assert!(
            parse(&args(&["frobnicate"]))
                .unwrap_err()
                .starts_with("unknown command: frobnicate")
        );
    }

    #[test]
    fn apply_ruleset_defaults_to_the_current_repository() {
        assert_eq!(
            parse(&args(&["apply-ruleset"])),
            Ok(Task::ApplyRuleset {
                allow_bypass_removal: false,
                repo: None
            })
        );
    }

    #[test]
    fn apply_ruleset_takes_the_flag_and_the_repository_in_either_order() {
        let expected = Ok(Task::ApplyRuleset {
            allow_bypass_removal: true,
            repo: Some("owner/repo".to_string()),
        });
        assert_eq!(
            parse(&args(&[
                "apply-ruleset",
                "--allow-bypass-removal",
                "owner/repo"
            ])),
            expected
        );
        assert_eq!(
            parse(&args(&[
                "apply-ruleset",
                "owner/repo",
                "--allow-bypass-removal"
            ])),
            expected
        );
    }

    #[test]
    fn an_empty_repository_argument_means_the_current_repository() {
        assert_eq!(
            parse(&args(&["apply-ruleset", ""])),
            Ok(Task::ApplyRuleset {
                allow_bypass_removal: false,
                repo: None
            })
        );
    }

    #[test]
    fn only_the_first_repository_argument_is_used() {
        assert_eq!(
            parse(&args(&["apply-ruleset", "owner/first", "owner/second"])),
            Ok(Task::ApplyRuleset {
                allow_bypass_removal: false,
                repo: Some("owner/first".to_string())
            })
        );
    }

    #[test]
    fn unknown_flags_are_refused_as_the_script_did() {
        assert_eq!(
            parse(&args(&["apply-ruleset", "--force"])),
            Err(format!("unknown argument: --force\n{USAGE}"))
        );
    }

    #[test]
    fn well_formed_repositories_are_accepted() {
        for repo in [
            "owner/repo",
            "Joris-Jonkers_dev/ttwright.rs",
            "a.b/c-d_e",
            "x/y.z",
        ] {
            assert_eq!(validate_repo(repo), Ok(()), "{repo}");
        }
    }

    #[test]
    fn malformed_repositories_are_rejected_before_any_gh_call() {
        for repo in [
            "owner",
            "owner/",
            "/repo",
            "a/b/c",
            "own er/repo",
            "owner/re$po",
            "owner/../repo",
            "owner/..",
            "../repo",
            "./repo",
            "owner/.",
            "owner/rep\u{e9}",
            "https://github.com/owner/repo",
        ] {
            assert!(validate_repo(repo).is_err(), "{repo} should be refused");
            let parsed = parse(&args(&["apply-ruleset", repo]));
            assert!(
                parsed
                    .as_ref()
                    .unwrap_err()
                    .starts_with("invalid repository"),
                "{repo}: {parsed:?}"
            );
        }
    }

    #[test]
    fn create_posts_the_template_when_no_main_ruleset_exists() {
        let gh = FakeGh::new(&[(LISTING, Ok("[]")), (POST, Ok(""))]);

        assert_eq!(apply(&gh, Some("o/r")), Ok(()));
        assert_eq!(gh.stdin_of(POST).as_deref(), Some(TEMPLATE));
        assert!(!gh.keys().iter().any(|k| k == PUT));
    }

    #[test]
    fn update_puts_the_body_and_keeps_the_existing_bypass_actors() {
        let current = json!({
            "name": "Main",
            "bypass_actors": [actor("Integration", 99, "always")],
        })
        .to_string();
        let gh = FakeGh::new(&[
            (
                LISTING,
                Ok(r#"[{"id":42,"name":"Main","source_type":"Repository"}]"#),
            ),
            (CURRENT, Ok(current.as_str())),
            (PUT, Ok("")),
        ]);

        assert_eq!(apply(&gh, Some("o/r")), Ok(()));
        assert!(!gh.keys().iter().any(|k| k == POST));
        let body: Value = serde_json::from_str(&gh.stdin_of(PUT).unwrap()).unwrap();
        assert_eq!(
            body["bypass_actors"],
            json!([actor("Integration", 99, "always")])
        );
        assert_eq!(body["name"], "Main");
        assert_eq!(body["rules"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn a_failed_listing_aborts_without_creating() {
        let gh = FakeGh::new(&[(LISTING, Err("gh: HTTP 500: internal error"))]);

        let err = apply(&gh, Some("o/r")).unwrap_err();

        assert!(err.contains("gh: HTTP 500: internal error"), "{err}");
        assert!(!gh.wrote());
    }

    #[test]
    fn an_unreadable_listing_aborts_without_creating() {
        let gh = FakeGh::new(&[(LISTING, Ok("<html>maintenance</html>"))]);

        assert!(apply(&gh, Some("o/r")).is_err());
        assert!(!gh.wrote());
    }

    #[test]
    fn several_repository_mains_abort_without_writing() {
        let gh = FakeGh::new(&[(
            LISTING,
            Ok(r#"[{"id":1,"name":"Main"},{"id":2,"name":"Main","source_type":"Repository"}]"#),
        )]);

        let err = apply(&gh, Some("o/r")).unwrap_err();

        assert!(err.contains("2 rulesets named Main"), "{err}");
        assert!(err.contains("1, 2"), "{err}");
        assert!(!gh.wrote());
    }

    #[test]
    fn an_organisation_main_is_not_this_repositorys_ruleset() {
        let gh = FakeGh::new(&[
            (
                LISTING,
                Ok(r#"[{"id":7,"name":"Main","source_type":"Organization"}]"#),
            ),
            (POST, Ok("")),
        ]);

        assert_eq!(apply(&gh, Some("o/r")), Ok(()));
        assert!(gh.keys().iter().any(|k| k == POST));
    }

    #[test]
    fn a_main_without_a_numeric_id_is_an_error_not_a_create() {
        let gh = FakeGh::new(&[(LISTING, Ok(r#"[{"id":"42","name":"Main"}]"#))]);

        let err = apply(&gh, Some("o/r")).unwrap_err();

        assert!(err.contains("no numeric id"), "{err}");
        assert!(!gh.wrote());
    }

    #[test]
    fn a_failed_write_surfaces_the_gh_error() {
        let gh = FakeGh::new(&[
            (LISTING, Ok("[]")),
            (
                POST,
                Err("gh api -X POST failed: Validation Failed: name already exists"),
            ),
        ]);

        let err = apply(&gh, Some("o/r")).unwrap_err();

        assert!(
            err.contains("Validation Failed: name already exists"),
            "{err}"
        );
    }

    #[test]
    fn an_update_refuses_a_current_body_without_bypass_actors() {
        let gh = FakeGh::new(&[
            (LISTING, Ok(r#"[{"id":42,"name":"Main"}]"#)),
            (CURRENT, Ok(r#"{"name":"Main"}"#)),
        ]);

        let err = apply(&gh, Some("o/r")).unwrap_err();

        assert!(err.contains("refusing to write"), "{err}");
        assert!(!gh.wrote());
    }

    #[test]
    fn the_repository_comes_from_gh_when_not_given() {
        let gh = FakeGh::new(&[
            (
                "repo view --json nameWithOwner -q .nameWithOwner",
                Ok("o/r\n"),
            ),
            (LISTING, Ok("[]")),
            (POST, Ok("")),
        ]);

        assert_eq!(apply(&gh, None), Ok(()));
        assert!(gh.keys().iter().any(|k| k == POST));
    }

    #[test]
    fn an_invalid_repository_is_refused_before_any_gh_call() {
        let gh = FakeGh::new(&[]);

        assert!(apply(&gh, Some("../x")).is_err());
        assert!(gh.keys().is_empty());
    }
}
