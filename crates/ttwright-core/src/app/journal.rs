// SPDX-License-Identifier: GPL-3.0-only
//! The resume journal: which asset operations, archives and post-installation steps an install has finished, kept
//! in the destination as `.ttwright-journal.json`. A cancelled or interrupted install leaves it; `--resume` reads
//! it and skips what is done; a finished install removes it.
//!
//! What counts as done: a loose file once it is written; the files of an archive (BSA) only once the archive is
//! written, since an archive is built in memory and written whole. The journal is ttwright's own state, not a
//! Contract: only ttwright reads it, and a journal of another version is ignored, not migrated.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use ttwright_contract::ErrorCode;

use crate::app::InstallError;

/// The journal's file name in the destination.
pub const JOURNAL_FILE: &str = ".ttwright-journal.json";
const JOURNAL_VERSION: u32 = 1;
const FLUSH_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    schema_version: u32,
    /// SHA-1 of the manifest: the same package.
    package: String,
    /// SHA-1 of the game copies and options: the same inputs.
    inputs: String,
    assets_done: BTreeSet<usize>,
    archives_done: BTreeSet<i32>,
    post_install_done: bool,
}

#[derive(Debug)]
struct Inner {
    state: State,
    last_flush: Instant,
    dirty: bool,
}

/// The journal of one install.
#[derive(Debug)]
pub struct Journal {
    path: Option<PathBuf>,
    resumed: bool,
    inner: Mutex<Inner>,
}

impl Journal {
    /// Open the journal in `destination` for the package and inputs given by their digests.
    ///
    /// With `resume`, a journal of the same package and inputs is continued; one of another package or other
    /// inputs is refused with `resume_mismatch`; none starts afresh. Without `resume`, any earlier journal is
    /// replaced.
    ///
    /// # Errors
    /// `resume_mismatch`, or an I/O error reading the journal.
    pub fn open(destination: &Path, package: &str, inputs: &str, resume: bool) -> Result<Self> {
        let path = destination.join(JOURNAL_FILE);
        let fresh = State {
            schema_version: JOURNAL_VERSION,
            package: package.to_owned(),
            inputs: inputs.to_owned(),
            ..State::default()
        };
        let (state, resumed) = if resume && path.is_file() {
            let text = std::fs::read(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let found: State = serde_json::from_slice(&text).map_err(|e| {
                InstallError::new(
                    ErrorCode::ResumeMismatch,
                    format!("The journal {} cannot be read: {e}", path.display()),
                )
            })?;
            if found.schema_version != JOURNAL_VERSION {
                return Err(InstallError::new(
                    ErrorCode::ResumeMismatch,
                    format!(
                        "The journal was written by another ttwright version (journal version {})",
                        found.schema_version
                    ),
                )
                .into());
            }
            if found.package != package || found.inputs != inputs {
                return Err(InstallError::new(
                    ErrorCode::ResumeMismatch,
                    "The journal in the destination belongs to another package or other game copies; \
                     install into an empty folder, or run without --resume to start again",
                )
                .into());
            }
            (found, true)
        } else {
            (fresh, false)
        };
        Ok(Self {
            path: Some(path),
            resumed,
            inner: Mutex::new(Inner {
                state,
                last_flush: Instant::now(),
                dirty: true,
            }),
        })
    }

    /// A journal that keeps nothing on disk (a dry run).
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            path: None,
            resumed: false,
            inner: Mutex::new(Inner {
                state: State::default(),
                last_flush: Instant::now(),
                dirty: false,
            }),
        }
    }

    /// Whether an earlier run's journal was continued.
    #[must_use]
    pub fn resumed(&self) -> bool {
        self.resumed
    }

    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut inner)
    }

    /// Whether the asset at `index` was finished by an earlier run.
    #[must_use]
    pub fn asset_done(&self, index: usize) -> bool {
        self.with(|i| i.state.assets_done.contains(&index))
    }

    /// Whether the archive written to location `location` was finished by an earlier run.
    #[must_use]
    pub fn archive_done(&self, location: i32) -> bool {
        self.with(|i| i.state.archives_done.contains(&location))
    }

    /// Whether the post-installation steps were finished by an earlier run.
    #[must_use]
    pub fn post_install_done(&self) -> bool {
        self.with(|i| i.state.post_install_done)
    }

    /// Record a finished loose-file asset. Written to disk at most every few seconds.
    pub fn record_asset(&self, index: usize) {
        self.record(|s| {
            s.assets_done.insert(index);
        });
    }

    /// Record a written archive.
    pub fn record_archive(&self, location: i32) {
        self.record(|s| {
            s.archives_done.insert(location);
        });
    }

    /// Record that the post-installation steps ran.
    pub fn record_post_install(&self) {
        self.record(|s| s.post_install_done = true);
    }

    fn record(&self, change: impl FnOnce(&mut State)) {
        let due = self.with(|i| {
            change(&mut i.state);
            i.dirty = true;
            i.last_flush.elapsed() >= FLUSH_EVERY
        });
        if due {
            let _ = self.flush();
        }
    }

    /// Write the journal now (atomically: a temporary file, then a rename).
    ///
    /// # Errors
    /// When the file cannot be written.
    pub fn flush(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = self.with(|i| {
            if !i.dirty {
                return None;
            }
            i.dirty = false;
            i.last_flush = Instant::now();
            Some(serde_json::to_vec(&i.state).expect("the journal serialises"))
        });
        let Some(text) = text else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("Failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Remove the journal: the install is finished.
    ///
    /// # Errors
    /// When the file exists and cannot be removed.
    pub fn finish(&self) -> Result<()> {
        if let Some(path) = &self.path
            && path.exists()
        {
            std::fs::remove_file(path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(label: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("ttwright_journal_{label}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_journal_survives_a_restart_and_is_removed_at_the_end() {
        let d = dir("restart");
        let j = Journal::open(&d, "pkg", "in", true).unwrap();
        assert!(!j.resumed());
        j.record_asset(3);
        j.record_archive(7);
        j.record_post_install();
        j.flush().unwrap();
        drop(j);
        let j = Journal::open(&d, "pkg", "in", true).unwrap();
        assert!(j.resumed());
        assert!(j.asset_done(3) && !j.asset_done(4));
        assert!(j.archive_done(7) && !j.archive_done(1));
        assert!(j.post_install_done());
        j.finish().unwrap();
        assert!(!d.join(JOURNAL_FILE).exists());
        j.finish().unwrap();
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn another_package_or_inputs_is_refused_and_no_resume_starts_afresh() {
        let d = dir("mismatch");
        let j = Journal::open(&d, "pkg", "in", false).unwrap();
        j.record_asset(1);
        j.flush().unwrap();
        for (p, i) in [("other", "in"), ("pkg", "other")] {
            let err = Journal::open(&d, p, i, true).unwrap_err();
            assert_eq!(crate::app::classify(&err).code, ErrorCode::ResumeMismatch);
        }
        let j = Journal::open(&d, "other", "in", false).unwrap();
        assert!(!j.asset_done(1) && !j.resumed());
        std::fs::write(d.join(JOURNAL_FILE), b"not json").unwrap();
        let err = Journal::open(&d, "pkg", "in", true).unwrap_err();
        assert_eq!(crate::app::classify(&err).code, ErrorCode::ResumeMismatch);
        std::fs::write(
            d.join(JOURNAL_FILE),
            br#"{"schema_version":9,"package":"pkg","inputs":"in","assets_done":[],"archives_done":[],"post_install_done":false}"#,
        )
        .unwrap();
        let err = Journal::open(&d, "pkg", "in", true).unwrap_err();
        assert!(err.to_string().contains("another ttwright version"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_in_memory_journal_writes_nothing() {
        let j = Journal::in_memory();
        j.record_asset(1);
        assert!(j.asset_done(1));
        j.flush().unwrap();
        j.finish().unwrap();
    }
}
