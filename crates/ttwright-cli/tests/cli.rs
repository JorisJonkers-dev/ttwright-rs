// SPDX-License-Identifier: GPL-3.0-only
//! The `ttwright` binary as the Toolbox runs it: Contracts, schemas, exit codes and JSON output.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use ttwright_contract::{DocumentKind, ErrorCode, Status, check, schema_file_name, schema_json};
use ttwright_testkit::{Fixture, loc};

fn ttwright(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ttwright"))
        .args(args)
        .output()
        .expect("run ttwright")
}

fn json(out: &Output) -> Value {
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "stdout is one document: {text}");
    serde_json::from_str(lines[0]).expect("stdout is JSON")
}

fn install_args<'a>(f: &'a Fixture, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "install",
        "--json",
        "--mpi",
        f.mpi.to_str().unwrap(),
        "--fo3",
        f.fallout3.to_str().unwrap(),
        "--dest",
        f.dest.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    args
}

#[test]
fn contracts_json_is_the_handshake() {
    let out = ttwright(&["contracts", "--json"]);
    assert!(out.status.success());
    let doc = json(&out);
    check(DocumentKind::Contracts, &doc).unwrap();
    assert_eq!(doc["tool"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(doc["contracts"][0]["name"], "ttwright-result");
    assert_eq!(
        doc["contracts"][0]["supports"]["packages"][0]["versions"][1],
        "3.4"
    );
    assert_eq!(doc["contracts"][1]["name"], "ttwright-events");
    assert_eq!(doc["contracts"][2]["name"], "ttwright-api");
    assert_eq!(doc["contracts"][2]["versions"][0], 1);
    assert_eq!(ttwright(&["contracts"]).status.code(), Some(2));
}

#[test]
fn the_committed_schemas_are_what_the_binary_prints() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    for kind in DocumentKind::ALL {
        for version in kind.versions() {
            let file = root.join(schema_file_name(*kind, *version));
            let committed = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            assert_eq!(
                committed,
                schema_json(*kind, *version).unwrap(),
                "{}",
                file.display()
            );
            let printed = ttwright(&[
                "schema",
                kind.name(),
                "--schema-version",
                &version.to_string(),
            ]);
            assert_eq!(String::from_utf8_lossy(&printed.stdout), committed);
        }
    }
    assert_eq!(ttwright(&["schema", "plan"]).status.code(), Some(2));
    assert_eq!(
        ttwright(&["schema", "result", "--schema-version", "9"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn an_install_prints_a_result_and_events() {
    let f = Fixture::new("cli-install");
    let events = f.dir.join("events.jsonl");
    let out = ttwright(&install_args(&f, &["--events", events.to_str().unwrap()]));
    let doc = json(&out);
    check(DocumentKind::Result, &doc).unwrap();
    assert_eq!(out.status.code(), Some(0), "{doc}");
    assert_eq!(doc["status"], "success");
    assert_eq!(doc["package"]["version"], "3.4.0");
    assert_eq!(doc["assets"]["done"], 6);
    assert_eq!(doc["archives"]["done"], 1);
    let lines = std::fs::read_to_string(&events).unwrap();
    let parsed: Vec<Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for line in &parsed {
        check(DocumentKind::Events, line).unwrap();
    }
    assert_eq!(parsed.first().unwrap()["event"], "phase");
    assert_eq!(parsed.last().unwrap()["event"], "finished");
    assert_eq!(parsed.last().unwrap()["status"], "success");
}

#[test]
fn events_go_to_stderr_by_default() {
    let f = Fixture::new("cli-stderr");
    let out = ttwright(&install_args(&f, &["--dry-run"]));
    assert_eq!(out.status.code(), Some(0));
    assert!(json(&out)["dry_run"].as_bool().unwrap());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr
            .lines()
            .all(|l| serde_json::from_str::<Value>(l).is_ok()),
        "{stderr}"
    );
    assert!(stderr.contains(r#""event":"finished""#));
}

#[test]
fn unsupported_features_exit_3_with_the_feature() {
    let mut m = Fixture::manifest();
    m.asset(3, "", loc::GAME_DATA, loc::DEST, "a.xwm", Some("a.fuz"));
    let f = Fixture::with_manifest("cli-unsupported", &m);
    let out = ttwright(&install_args(&f, &[]));
    assert_eq!(out.status.code(), Some(3));
    let doc = json(&out);
    assert_eq!(doc["status"], "unsupported");
    assert_eq!(doc["error"]["code"], "unsupported_feature");
    assert_eq!(doc["error"]["feature"], "asset_op:3");
}

#[test]
fn json_mode_never_scans_for_game_copies() {
    let f = Fixture::new("cli-no-scan");
    let out = ttwright(&[
        "install",
        "--json",
        "--mpi",
        f.mpi.to_str().unwrap(),
        "--dest",
        f.dest.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let doc = json(&out);
    assert_eq!(doc["error"]["code"], "game_path_missing");
    assert_eq!(doc["error"]["feature"], "fallout3");
}

#[test]
fn a_missing_game_copy_in_json_mode_names_the_option_that_answers_it() {
    let f = Fixture::new("cli-hint");
    let out = ttwright(&[
        "install",
        "--json",
        "--mpi",
        f.mpi.to_str().unwrap(),
        "--dest",
        f.dest.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let doc = json(&out);
    check(DocumentKind::Result, &doc).unwrap();
    assert_eq!(doc["status"], "failed");
    assert_eq!(doc["error"]["code"], "game_path_missing");
    assert_eq!(doc["error"]["feature"], "fallout3");
    let message = doc["error"]["message"].as_str().unwrap();
    assert!(message.contains("--fo3"), "{message}");
}

#[test]
fn usage_errors_exit_2_and_in_json_mode_print_a_result() {
    let out = ttwright(&["install", "--json", "--mpi", "x"]);
    assert_eq!(out.status.code(), Some(2));
    let doc = json(&out);
    assert_eq!(doc["error"]["code"], "usage");
    let out = ttwright(&["install", "--mpi", "x"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(ttwright(&["--version"]).status.code(), Some(0));
    let f = Fixture::new("cli-events-dir");
    let bad = f.dir.join("no/such/dir/events.jsonl");
    let out = ttwright(&install_args(&f, &["--events", bad.to_str().unwrap()]));
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn every_error_code_maps_to_its_exit_code() {
    for code in ErrorCode::ALL {
        let expected = match code.status() {
            Status::Unsupported => 3,
            Status::Cancelled => 4,
            _ if *code == ErrorCode::Usage => 2,
            _ => 1,
        };
        assert_eq!(code.exit_code(), expected, "{code}");
    }
}

#[test]
fn people_get_a_summary_and_inspect_reports_support() {
    let f = Fixture::new("cli-human");
    let out = ttwright(&[
        "install",
        "--no-detect",
        "--mpi",
        f.mpi.to_str().unwrap(),
        "--fo3",
        f.fallout3.to_str().unwrap(),
        "--dest",
        f.dest.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("Installation Complete"));
    let out = ttwright(&["inspect", "--mpi", f.mpi.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Features: all supported"), "{stdout}");
    assert!(stdout.contains("Version: supported"), "{stdout}");

    let mut m = Fixture::manifest();
    m.post_command("cmd.exe /C copy a b");
    let g = Fixture::with_manifest("cli-human-bad", &m);
    let out = ttwright(&["inspect", "--mpi", g.mpi.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("post_command:copy"));
    let out = ttwright(&[
        "install",
        "--no-detect",
        "--mpi",
        g.mpi.to_str().unwrap(),
        "--dest",
        g.dest.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Missing: post_command:copy"));
}
