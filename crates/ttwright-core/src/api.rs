// SPDX-License-Identifier: GPL-3.0-only
//! The library API of ttwright: install a package in process, with progress, cancel, resume and copyable errors.
//!
//! A caller builds an [`InstallJob`], passes a [`Progress`] receiver and a [`CancelToken`], and gets an
//! [`InstallOutput`] or an [`Error`]:
//!
//! ```no_run
//! use ttwright_core::api::{CancelToken, InstallJob, InstallOutput, install};
//! use ttwright_contract::Event;
//!
//! let job = InstallJob {
//!     mpi_path: "/downloads/package.mpi".into(),
//!     falloutnv_path: Some("/games/newvegas".into()),
//!     destination_path: "/mods/ttw".into(),
//!     ..InstallJob::default()
//! };
//! let cancel = CancelToken::new(); // a clone can cancel from another thread
//! let mut progress = |event: &Event| eprintln!("{event:?}");
//! match install(&job, &mut progress, &cancel) {
//!     Ok(InstallOutput::Done { result }) => println!("installed {} assets", result.assets.done),
//!     Ok(InstallOutput::Cancelled { .. }) => println!("stopped; resume continues it"),
//!     Err(e) => println!("{}", e.copy_text()),
//! }
//! ```
//!
//! The library prints nothing and never ends the process; the `ttwright` command line is a wrapper over it.
//! [`API_VERSION`] is the version of this interface; `ttwright contracts --json` lists it as the `ttwright-api`
//! contract.

use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ttwright_contract::{ErrorCode, Event, PackageInfo, Phase, ResultDoc, Status};

pub use crate::app::CancelToken;
use crate::app::{InstallEvent, InstallOutcome, InstallRequest, JOURNAL_FILE, run_install};
pub use ttwright_contract::{API_CONTRACT, API_VERSION};

/// What to install, from where, into where.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallJob {
    /// The `.mpi` file, or a folder it was extracted to.
    pub mpi_path: PathBuf,
    /// The Fallout 3 copy, when the package needs it.
    pub fallout3_path: Option<PathBuf>,
    /// The Fallout: New Vegas copy, when the package needs it.
    pub falloutnv_path: Option<PathBuf>,
    /// The Oblivion copy, when the package needs it.
    pub oblivion_path: Option<PathBuf>,
    /// The destination folder.
    pub destination_path: PathBuf,
    /// Read and check everything, write nothing.
    pub dry_run: bool,
    /// Continue an interrupted install from the journal in the destination.
    pub resume: bool,
    /// Install a TTW package of a version this build does not list as supported.
    pub allow_unsupported_version: bool,
    /// Look on this machine for game copies that were not given. Off by default: a caller that already knows the
    /// copies passes them.
    pub detect_missing: bool,
}

/// How an [`install`] call ended when it did not fail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum InstallOutput {
    /// Installed (or, for a dry run, everything was read and checked).
    Done {
        /// The `ttwright-result` document, status `success`.
        result: Box<ResultDoc>,
    },
    /// The cancel token stopped the install between assets. The journal in the destination is kept, and a call
    /// with [`InstallJob::resume`] set continues from it.
    Cancelled {
        /// The `ttwright-result` document, status `cancelled`: what was done before the stop.
        result: Box<ResultDoc>,
    },
}

/// Why an install failed: a stable code, a summary for people, what is missing for the codes that name it, and
/// the result document with what was done before the failure. Never contains a command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Error {
    /// The stable code callers match on.
    pub code: ErrorCode,
    /// For people: what went wrong, with its causes.
    pub summary: String,
    /// For `unsupported_feature`, `unsupported_version` and `game_path_missing`: what is missing, for example
    /// `asset_op:3` or `falloutnv`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    /// The `ttwright-result` document of the failed run (status `failed` or `unsupported`).
    pub result: Box<ResultDoc>,
}

impl Error {
    /// The error as one block of text to paste into a bug report: the library and its version, the code, the
    /// summary and what is missing. The home folder is shown as `~`.
    #[must_use]
    pub fn copy_text(&self) -> String {
        let mut text = format!(
            "ttwright {} (api {API_VERSION})\nerror: {}\n{}",
            env!("CARGO_PKG_VERSION"),
            self.code,
            self.summary
        );
        if let Some(feature) = &self.feature {
            text.push_str("\nmissing: ");
            text.push_str(feature);
        }
        for message in &self.result.messages {
            text.push_str("\n  ");
            text.push_str(message);
        }
        match home() {
            Some(home) => replace_home(&text, &home),
            None => text,
        }
    }
}

/// `text` with every occurrence of the folder `home` that is a whole path replaced by `~`: one that ends the text or
/// is followed by a separator. `/home/al` is not replaced inside `/home/alice`.
fn replace_home(text: &str, home: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(home) {
        let after = &rest[at + home.len()..];
        out.push_str(&rest[..at]);
        if after.is_empty() || after.starts_with(['/', '\\']) {
            out.push('~');
        } else {
            out.push_str(home);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The home folder, when it is long enough that replacing it cannot hit unrelated text.
fn home() -> Option<String> {
    ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|h| h.trim_end_matches(['/', '\\']).to_owned())
        .find(|h| h.len() > 1)
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.summary)
    }
}

impl std::error::Error for Error {}

/// Receives the `ttwright-events` events of a call: `started`, `phase`, `progress`, `warning` and, last,
/// `finished`. Assets are worked on several threads, so a receiver is `Send`; the call hands it one event at a
/// time. Log lines for people go to `tracing` instead.
pub trait Progress: Send {
    /// One event.
    fn report(&mut self, event: &Event);
}

impl<F: FnMut(&Event) + Send> Progress for F {
    fn report(&mut self, event: &Event) {
        self(event);
    }
}

/// A receiver that drops every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn report(&mut self, _: &Event) {}
}

/// A stable key for `phase` that a caller words in its own voice, for example `ttw.assets`.
#[must_use]
pub fn label_key(phase: Phase) -> &'static str {
    match phase {
        Phase::Package => "ttw.package",
        Phase::Preflight => "ttw.preflight",
        Phase::Checks => "ttw.checks",
        Phase::Assets => "ttw.assets",
        Phase::PostInstall => "ttw.post_install",
    }
}

fn result_document(job: &InstallJob, outcome: &InstallOutcome) -> ResultDoc {
    let report = &outcome.report;
    let mut doc = ResultDoc::new(env!("CARGO_PKG_VERSION"));
    doc.package = report.package_name.as_ref().map(|title| PackageInfo {
        title: title.clone(),
        version: report.package_version.clone().unwrap_or_default(),
        supported: report.package_supported,
    });
    doc.output_dir = Some(job.destination_path.display().to_string());
    doc.checks_passed = report.checks_passed;
    doc.assets = report.assets;
    doc.archives = report.archives;
    doc.post_install = report.post_install;
    doc.resumed = report.resumed;
    doc.dry_run = job.dry_run;
    doc.elapsed_ms = u64::try_from(report.elapsed.as_millis()).unwrap_or(u64::MAX);
    for message in &report.messages {
        doc.note(message.clone());
    }
    if let Some(error) = &outcome.error {
        doc.fail(error.code, error.message.clone(), error.feature.clone());
    }
    doc
}

/// Install the package `job` names into its destination.
///
/// `progress` receives the `ttwright-events` events; `cancel` is checked between assets, and a token that is already
/// set when the call starts ends it as cancelled. The token only changes the outcome when the install stopped because
/// of it: an install that finished is a success, and a failure for another reason keeps its own code. A cancelled call
/// returns [`InstallOutput::Cancelled`]; when it left a journal in the destination, a later call with
/// [`InstallJob::resume`] continues where it stopped. The last event is always [`Event::Finished`], with the status of
/// the returned document. Nothing is printed and the process is never ended.
///
/// # Errors
/// An [`Error`] whose `code` says why: for example `mpi_not_found`, `manifest_invalid`, `unsupported_feature`,
/// `unsupported_version`, `game_path_missing`, `check_failed`, `asset_failed`, `resume_mismatch` or `io`.
pub fn install(
    job: &InstallJob,
    progress: &mut dyn Progress,
    cancel: &CancelToken,
) -> Result<InstallOutput, Error> {
    let request = InstallRequest {
        mpi_path: job.mpi_path.clone(),
        fallout3_path: job.fallout3_path.clone(),
        falloutnv_path: job.falloutnv_path.clone(),
        oblivion_path: job.oblivion_path.clone(),
        destination_path: job.destination_path.clone(),
        dry_run: job.dry_run,
        detect_missing: job.detect_missing,
        resume: job.resume,
        allow_unsupported_version: job.allow_unsupported_version,
        cancel: cancel.clone(),
    };
    // The receiver is used from the install's worker threads, so it lives behind a lock. A panic in it poisons the
    // lock; the events after that are dropped rather than panicking again, and the receiver is still recovered for
    // the `Finished` event below.
    let shared = Mutex::new(progress);
    let outcome = run_install(request, |event| match event {
        InstallEvent::Log(message) => tracing::info!("{message}"),
        InstallEvent::Event(event) => {
            if let Ok(mut p) = shared.lock() {
                p.report(&event);
            }
        }
    });
    let mut doc = result_document(job, &outcome);
    // A cancel only changes the outcome when the install stopped because of it: the token was set before the work
    // finished. A cancel that arrives before the asset phase (for example during the checks) is not seen by the
    // asset processor, so such a run would otherwise report the cancel as a failure; it is turned into a cancelled
    // result here. A run that finished, or that failed for another reason, keeps its own outcome, and a token set
    // after it returned changes nothing.
    let stopped_by_cancel = cancel.is_cancelled()
        && match &outcome.error {
            None => false,
            Some(e) => e.code == ErrorCode::Cancelled,
        };
    if stopped_by_cancel && doc.status != Status::Cancelled {
        doc.fail(
            ErrorCode::Cancelled,
            "The install was cancelled; resuming it continues from where it stopped",
            None,
        );
    }
    let cancelled = stopped_by_cancel || doc.status == Status::Cancelled;
    // Only a journal on disk is something a resumed call can continue from.
    let resumable = cancelled && job.destination_path.join(JOURNAL_FILE).is_file();
    if cancelled && !resumable {
        // Nothing was written that a resume could continue: offer none.
        if let Some(error) = &mut doc.error
            && error.code == ErrorCode::Cancelled
        {
            "The install was cancelled before it started; run it again to install"
                .clone_into(&mut error.message);
        }
    }
    let progress = shared
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    progress.report(&Event::Finished {
        status: doc.status,
        code: doc.error.as_ref().map(|e| e.code),
    });
    if cancelled {
        return Ok(InstallOutput::Cancelled {
            result: Box::new(doc),
        });
    }
    let Some(e) = outcome.error else {
        return Ok(InstallOutput::Done {
            result: Box::new(doc),
        });
    };
    Err(Error {
        code: e.code,
        summary: e.message,
        feature: e.feature,
        result: Box::new(doc),
    })
}

#[cfg(test)]
mod tests {
    use super::{Error, NoProgress, Progress, label_key};
    use ttwright_contract::{ErrorCode, Event, Phase, ResultDoc};

    fn error(feature: Option<&str>) -> Error {
        Error {
            code: ErrorCode::GamePathMissing,
            summary: "The package needs the falloutnv copy".into(),
            feature: feature.map(str::to_owned),
            result: Box::new(ResultDoc::new("0.0.0")),
        }
    }

    #[test]
    fn the_copy_text_names_the_version_the_code_the_summary_and_what_is_missing() {
        let text = error(Some("falloutnv")).copy_text();
        assert_eq!(
            text,
            format!(
                "ttwright {} (api {})\nerror: game_path_missing\nThe package needs the falloutnv copy\nmissing: falloutnv",
                env!("CARGO_PKG_VERSION"),
                super::API_VERSION
            )
        );
    }

    #[test]
    fn the_copy_text_shows_the_home_folder_as_a_tilde() {
        // A known home, whatever this machine's is: the summary names a file in it.
        let home = "/home/synthetic-user";
        let mut e = error(None);
        e.summary = format!("cannot read {home}/x.mpi");
        // The machine's home is replaced by `copy_text` itself; any other known home is replaced here, so the test
        // says the same thing on every machine.
        let text = super::replace_home(&e.copy_text(), home);
        assert!(text.contains("cannot read ~/x.mpi"), "{text}");
        assert!(!text.contains(home), "{text}");
        if let Some(real) = super::home() {
            let mut e = error(None);
            e.summary = format!("cannot read {real}/x.mpi");
            let text = e.copy_text();
            assert!(text.contains("cannot read ~/x.mpi"), "{text}");
            assert!(
                !text.contains(&real),
                "the home folder is shown as ~: {text}"
            );
        }
    }

    #[test]
    fn the_home_folder_is_only_replaced_at_a_path_boundary() {
        let home = "/home/al";
        assert_eq!(
            super::replace_home("in /home/al/x and /home/alice/y and /home/al", home),
            "in ~/x and /home/alice/y and ~"
        );
    }

    #[test]
    fn every_phase_has_its_own_label_key() {
        let keys: Vec<_> = [
            Phase::Package,
            Phase::Preflight,
            Phase::Checks,
            Phase::Assets,
            Phase::PostInstall,
        ]
        .into_iter()
        .map(label_key)
        .collect();
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "the keys are all unique");
        assert!(keys.iter().all(|k| k.starts_with("ttw.")));
    }

    #[test]
    fn no_progress_takes_any_event() {
        NoProgress.report(&Event::Warning {
            message: "x".into(),
        });
    }
}
