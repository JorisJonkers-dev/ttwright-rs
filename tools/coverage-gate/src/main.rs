// SPDX-License-Identifier: GPL-3.0-only
//! The changed-line coverage gate.
//!
//! ```text
//! coverage-gate --lcov lcov.info --base <commit> [--min 70] [--summary <file>]
//! ```
//!
//! It takes the lines a change adds or edits (`git diff -U0 -M <base>...HEAD`), keeps those that the coverage
//! report counts as executable, and fails when fewer than `--min` percent of them ran in a test. A change with no
//! executable changed line passes. Moved files count only their edited lines, because renames are detected.
//! The report (per file, with the uncovered line numbers) goes to stdout and, with `--summary`, is appended to that
//! file as Markdown (the job summary on CI).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, ExitCode};

/// Changed lines per repository-relative path.
type Lines = BTreeMap<String, BTreeSet<u32>>;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(why) => {
            eprintln!("coverage-gate: {why}");
            ExitCode::from(2)
        }
    }
}

struct Options {
    lcov: String,
    base: String,
    min: f64,
    summary: Option<String>,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut lcov = None;
    let mut base = None;
    let mut min = 70.0;
    let mut summary = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--lcov" => lcov = Some(value()?),
            "--base" => base = Some(value()?),
            "--min" => {
                min = value()?
                    .parse()
                    .map_err(|_| "--min takes a number".to_owned())?;
            }
            "--summary" => summary = Some(value()?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Options {
        lcov: lcov.ok_or("--lcov is required")?,
        base: base.ok_or("--base is required")?,
        min,
        summary,
    })
}

fn run(args: Vec<String>) -> Result<bool, String> {
    let options = parse_options(&args)?;
    let root = git(&["rev-parse", "--show-toplevel"])?.trim().to_owned();
    let diff = git(&[
        "diff",
        "-U0",
        "-M",
        "--no-color",
        "--no-ext-diff",
        &format!("{}...HEAD", options.base),
        "--",
        "*.rs",
    ])?;
    let changed = changed_lines(&diff);
    let lcov = std::fs::read_to_string(&options.lcov)
        .map_err(|e| format!("cannot read {}: {e}", options.lcov))?;
    let report = gate(&changed, &parse_lcov(&lcov, &root), options.min);
    print!("{}", report.text);
    if let Some(path) = options.summary {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("cannot open {path}: {e}"))?;
        file.write_all(report.text.as_bytes())
            .map_err(|e| format!("cannot write {path}: {e}"))?;
    }
    Ok(report.passed)
}

fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "git printed something that is not UTF-8".to_owned())
}

/// The new-side line numbers each file of a `git diff -U0` adds or edits.
fn changed_lines(diff: &str) -> Lines {
    let mut lines = Lines::new();
    let mut file: Option<String> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            file = path.strip_prefix("b/").map(str::to_owned);
        } else if let (Some(hunk), Some(path)) = (line.strip_prefix("@@ "), &file) {
            // @@ -a,b +c,d @@  : d lines from c, d defaults to 1 and 0 means none.
            let Some(new) = hunk.split(' ').find_map(|part| part.strip_prefix('+')) else {
                continue;
            };
            let (start, count) = match new.split_once(',') {
                Some((s, c)) => (s.parse::<u32>().ok(), c.parse::<u32>().ok()),
                None => (new.parse::<u32>().ok(), Some(1)),
            };
            if let (Some(start), Some(count)) = (start, count) {
                lines
                    .entry(path.clone())
                    .or_default()
                    .extend(start..start + count);
            }
        }
    }
    lines.retain(|_, set| !set.is_empty());
    lines
}

/// Executable lines and whether each ran, per repository-relative path, from an lcov report.
fn parse_lcov(text: &str, root: &str) -> BTreeMap<String, BTreeMap<u32, bool>> {
    let mut files: BTreeMap<String, BTreeMap<u32, bool>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            current = Some(relative(path, root));
        } else if let (Some(da), Some(path)) = (line.strip_prefix("DA:"), &current) {
            let mut parts = da.split(',');
            let number = parts.next().and_then(|n| n.parse::<u32>().ok());
            let count = parts.next().and_then(|n| n.parse::<u64>().ok());
            if let (Some(number), Some(count)) = (number, count) {
                let ran = files
                    .entry(path.clone())
                    .or_default()
                    .entry(number)
                    .or_insert(false);
                *ran |= count > 0;
            }
        } else if line == "end_of_record" {
            current = None;
        }
    }
    files
}

fn relative(path: &str, root: &str) -> String {
    let path = path.replace('\\', "/");
    let root = root.replace('\\', "/");
    Path::new(&path)
        .strip_prefix(&root)
        .map_or(path.clone(), |p| p.to_string_lossy().into_owned())
}

struct Report {
    passed: bool,
    text: String,
}

fn gate(changed: &Lines, coverage: &BTreeMap<String, BTreeMap<u32, bool>>, min: f64) -> Report {
    let mut total = 0usize;
    let mut covered = 0usize;
    let mut rows = String::new();
    for (path, lines) in changed {
        let Some(executable) = coverage.get(path) else {
            continue;
        };
        let mut file_total = 0usize;
        let mut missed = Vec::new();
        for line in lines {
            if let Some(&ran) = executable.get(line) {
                file_total += 1;
                if !ran {
                    missed.push(*line);
                }
            }
        }
        if file_total == 0 {
            continue;
        }
        total += file_total;
        covered += file_total - missed.len();
        let _ = writeln!(
            rows,
            "| `{path}` | {} / {file_total} | {} |",
            file_total - missed.len(),
            ranges(&missed)
        );
    }
    let mut text = String::from("## Changed-line coverage\n\n");
    if total == 0 {
        text.push_str("No executable line changed: nothing to gate.\n");
        return Report { passed: true, text };
    }
    #[allow(clippy::cast_precision_loss)]
    let share = covered as f64 * 100.0 / total as f64;
    let passed = share + 1e-9 >= min;
    let _ = writeln!(
        text,
        "{covered} of {total} executable changed lines ran in a test: **{share:.1}%** (minimum {min}%). {}\n",
        if passed { "Passed." } else { "Failed." }
    );
    text.push_str("| File | Covered | Lines no test ran |\n| --- | --- | --- |\n");
    text.push_str(&rows);
    text.push('\n');
    Report { passed, text }
}

/// `3-5, 9` from `[3, 4, 5, 9]`.
fn ranges(lines: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start = lines[i];
        let mut end = start;
        while i + 1 < lines.len() && lines[i + 1] == end + 1 {
            i += 1;
            end = lines[i];
        }
        out.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    out.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,0 +2,3 @@ fn x() {
+one
+two
+three
@@ -10 +12 @@
-old
+new
diff --git a/src/gone.rs b/src/gone.rs
--- a/src/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-x
-y
";

    #[test]
    fn changed_lines_reads_the_new_side_of_each_hunk() {
        let lines = changed_lines(DIFF);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines["src/a.rs"].iter().copied().collect::<Vec<_>>(),
            vec![2, 3, 4, 12]
        );
    }

    #[test]
    fn lcov_paths_become_relative_and_counts_merge() {
        let lcov = "SF:/repo/src/a.rs\nDA:2,0\nDA:3,4\nDA:2,1\nend_of_record\nSF:/elsewhere/b.rs\nDA:1,0\nend_of_record\n";
        let files = parse_lcov(lcov, "/repo");
        assert!(files["src/a.rs"][&2]);
        assert!(files["src/a.rs"][&3]);
        assert!(files.contains_key("/elsewhere/b.rs"));
    }

    fn coverage(entries: &[(u32, bool)]) -> BTreeMap<String, BTreeMap<u32, bool>> {
        BTreeMap::from([("src/a.rs".to_owned(), entries.iter().copied().collect())])
    }

    #[test]
    fn the_gate_counts_only_executable_changed_lines() {
        let changed = changed_lines(DIFF);
        // Line 3 is not executable; 2 and 12 ran, 4 did not: 2 of 3.
        let report = gate(
            &changed,
            &coverage(&[(2, true), (4, false), (12, true)]),
            66.0,
        );
        assert!(report.passed, "{}", report.text);
        assert!(report.text.contains("2 of 3"));
        assert!(report.text.contains("| `src/a.rs` | 2 / 3 | 4 |"));
        let report = gate(
            &changed,
            &coverage(&[(2, true), (4, false), (12, true)]),
            70.0,
        );
        assert!(!report.passed);
    }

    #[test]
    fn no_executable_changed_line_passes() {
        let changed = changed_lines(DIFF);
        let report = gate(&changed, &coverage(&[(100, false)]), 70.0);
        assert!(report.passed);
        assert!(report.text.contains("nothing to gate"));
    }

    #[test]
    fn ranges_join_runs() {
        assert_eq!(ranges(&[3, 4, 5, 9, 11, 12]), "3-5, 9, 11-12");
        assert_eq!(ranges(&[]), "");
    }

    #[test]
    fn options_need_lcov_and_base() {
        let args = |s: &str| s.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert!(parse_options(&args("--lcov a")).is_err());
        let o = parse_options(&args("--lcov a --base b --min 80 --summary s")).unwrap();
        assert_eq!((o.lcov.as_str(), o.base.as_str(), o.min), ("a", "b", 80.0));
        assert_eq!(o.summary.as_deref(), Some("s"));
        assert!(parse_options(&args("--what")).is_err());
    }
}
