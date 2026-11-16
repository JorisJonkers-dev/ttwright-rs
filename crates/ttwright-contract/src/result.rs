// SPDX-License-Identifier: GPL-3.0-only
//! Result v1: what `ttwright install --json` prints on stdout at the end, also when it failed or was cancelled.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ErrorCode, RESULT_CONTRACT, ToolInfo};

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Installed; exit code 0.
    Success,
    /// Failed; exit code 1 (2 for a usage error).
    Failed,
    /// The package needs a feature or is a version this build does not support; exit code 3. Nothing was written.
    Unsupported,
    /// Cancelled; exit code 4. `--resume` continues from the journal.
    Cancelled,
}

impl Status {
    /// The process exit code that goes with the status.
    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Failed => 1,
            Self::Unsupported => 3,
            Self::Cancelled => 4,
        }
    }
}

/// The package that was installed, from its manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PackageInfo {
    /// The manifest's title.
    pub title: String,
    /// The manifest's version, as written there.
    pub version: String,
    /// Whether this build lists the package and version as supported (`ttwright contracts --json`).
    pub supported: bool,
}

/// Done and failed counts of one kind of work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Counts {
    /// Items in the package.
    pub total: u64,
    /// Items done in this run.
    pub done: u64,
    /// Items that failed.
    pub failed: u64,
    /// Items done by an earlier, interrupted run and kept (`--resume`).
    #[serde(default)]
    pub resumed: u64,
}

/// Why the run did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorInfo {
    /// The stable code callers match on.
    pub code: ErrorCode,
    /// Prose for people; never matched on.
    pub message: String,
    /// For `unsupported_feature`, `unsupported_version` and `game_path_missing`: what is missing, for example
    /// `asset_op:3` or `variable:%XYZ%`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
}

/// Result v1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResultDoc {
    /// The document version; 1.
    #[schemars(extend("const" = 1))]
    pub schema_version: u32,
    /// Always `ttwright-result`: a reader refuses a document that names another Contract.
    #[schemars(extend("const" = "ttwright-result"))]
    pub contract: String,
    /// The tool that printed this document.
    pub tool: ToolInfo,
    /// How the run ended.
    pub status: Status,
    /// The process exit code.
    pub exit_code: u8,
    /// The package, once its manifest was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageInfo>,
    /// The destination folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    /// Whether every pre-installation check of the manifest passed (the game copies are the expected ones).
    pub checks_passed: bool,
    /// Asset operations (copy, new, patch, audio).
    pub assets: Counts,
    /// Archives (BSA) written.
    pub archives: Counts,
    /// Post-installation commands (rename, delete) and file times.
    pub post_install: Counts,
    /// Whether this run continued an interrupted one.
    pub resumed: bool,
    /// A dry run reads everything and writes nothing.
    pub dry_run: bool,
    /// Wall time in milliseconds.
    pub elapsed_ms: u64,
    /// The log file of the run, when one was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<String>,
    /// Set unless the status is `success`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    /// Up to twenty lines on what went wrong in detail (a failed asset, a failed check).
    #[serde(default)]
    pub messages: Vec<String>,
}

impl ResultDoc {
    /// A successful, empty result for a tool at `version`.
    #[must_use]
    pub fn new(version: &str) -> Self {
        Self {
            schema_version: 1,
            contract: RESULT_CONTRACT.to_owned(),
            tool: ToolInfo::new(version),
            status: Status::Success,
            exit_code: 0,
            package: None,
            output_dir: None,
            checks_passed: false,
            assets: Counts::default(),
            archives: Counts::default(),
            post_install: Counts::default(),
            resumed: false,
            dry_run: false,
            elapsed_ms: 0,
            log_file: None,
            error: None,
            messages: Vec::new(),
        }
    }

    /// Mark the run as stopped by `code`, which sets the status and the exit code.
    pub fn fail(&mut self, code: ErrorCode, message: impl Into<String>, feature: Option<String>) {
        self.status = code.status();
        self.exit_code = code.exit_code();
        self.error = Some(ErrorInfo {
            code,
            message: message.into(),
            feature,
        });
    }

    /// Add a detail line, keeping at most twenty.
    pub fn note(&mut self, message: impl Into<String>) {
        if self.messages.len() < 20 {
            self.messages.push(message.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_result_is_a_success_and_fail_sets_status_and_exit_code() {
        let mut r = ResultDoc::new("0.3.0");
        assert_eq!((r.status, r.exit_code), (Status::Success, 0));
        r.fail(
            ErrorCode::UnsupportedFeature,
            "asset operation 3 is not implemented",
            Some("asset_op:3".into()),
        );
        assert_eq!((r.status, r.exit_code), (Status::Unsupported, 3));
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["error"]["code"], "unsupported_feature");
        assert_eq!(json["error"]["feature"], "asset_op:3");
        assert_eq!(json["contract"], "ttwright-result");
    }

    #[test]
    fn notes_are_capped_at_twenty() {
        let mut r = ResultDoc::new("0");
        for i in 0..30 {
            r.note(format!("line {i}"));
        }
        assert_eq!(r.messages.len(), 20);
        assert_eq!(r.messages[19], "line 19");
    }

    #[test]
    fn status_exit_codes() {
        let codes: Vec<u8> = [
            Status::Success,
            Status::Failed,
            Status::Unsupported,
            Status::Cancelled,
        ]
        .iter()
        .map(|s| s.exit_code())
        .collect();
        assert_eq!(codes, [0, 1, 3, 4]);
    }
}
