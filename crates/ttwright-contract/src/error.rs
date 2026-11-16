// SPDX-License-Identifier: GPL-3.0-only
//! Stable error codes. Callers match on the code, never on the message. A code is never renamed or reused; a new
//! failure gets a new code.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Status;

/// Why a run did not succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The arguments are wrong (exit code 2).
    Usage,
    /// The MPI package path does not exist.
    MpiNotFound,
    /// The MPI package cannot be read: not an archive, or truncated.
    MpiInvalid,
    /// The package has no manifest (`_package/index.json`).
    ManifestMissing,
    /// The manifest is not valid JSON or does not have the manifest's shape.
    ManifestInvalid,
    /// The manifest uses a feature this build does not implement; `feature` names it. Nothing was written.
    UnsupportedFeature,
    /// The package is a TTW version this build does not support; `feature` names the version.
    UnsupportedVersion,
    /// The manifest needs a game copy that was not given (`--fo3`, `--fnv`, `--oblivion`); `feature` names it.
    GamePathMissing,
    /// A game copy that was given does not exist or is not that game.
    GameNotFound,
    /// A pre-installation check of the manifest failed (missing or modified game file, too little space).
    CheckFailed,
    /// One or more asset operations failed.
    AssetFailed,
    /// One or more archives (BSA) could not be written.
    ArchiveFailed,
    /// A post-installation command failed.
    PostCommandFailed,
    /// `--resume` was asked, but the journal in the destination belongs to another package or another set of
    /// game copies.
    ResumeMismatch,
    /// The run was cancelled; `--resume` continues it.
    Cancelled,
    /// A file could not be read or written.
    Io,
    /// A fault in ttwright itself.
    Internal,
}

impl ErrorCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::Usage,
        Self::MpiNotFound,
        Self::MpiInvalid,
        Self::ManifestMissing,
        Self::ManifestInvalid,
        Self::UnsupportedFeature,
        Self::UnsupportedVersion,
        Self::GamePathMissing,
        Self::GameNotFound,
        Self::CheckFailed,
        Self::AssetFailed,
        Self::ArchiveFailed,
        Self::PostCommandFailed,
        Self::ResumeMismatch,
        Self::Cancelled,
        Self::Io,
        Self::Internal,
    ];

    /// The code as it appears in documents, for example `unsupported_feature`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::MpiNotFound => "mpi_not_found",
            Self::MpiInvalid => "mpi_invalid",
            Self::ManifestMissing => "manifest_missing",
            Self::ManifestInvalid => "manifest_invalid",
            Self::UnsupportedFeature => "unsupported_feature",
            Self::UnsupportedVersion => "unsupported_version",
            Self::GamePathMissing => "game_path_missing",
            Self::GameNotFound => "game_not_found",
            Self::CheckFailed => "check_failed",
            Self::AssetFailed => "asset_failed",
            Self::ArchiveFailed => "archive_failed",
            Self::PostCommandFailed => "post_command_failed",
            Self::ResumeMismatch => "resume_mismatch",
            Self::Cancelled => "cancelled",
            Self::Io => "io",
            Self::Internal => "internal",
        }
    }

    /// How a run that stopped with this code ended.
    #[must_use]
    pub fn status(self) -> Status {
        match self {
            Self::UnsupportedFeature | Self::UnsupportedVersion => Status::Unsupported,
            Self::Cancelled => Status::Cancelled,
            _ => Status::Failed,
        }
    }

    /// The process exit code: 2 for usage, otherwise the status's ([`Status::exit_code`]).
    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Usage => 2,
            other => other.status().exit_code(),
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorCode;
    use crate::Status;

    #[test]
    fn the_wire_name_is_the_serde_name_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for code in ErrorCode::ALL {
            let json = serde_json::to_string(code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
            assert_eq!(code.to_string(), code.as_str());
            assert!(seen.insert(code.as_str()));
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *code);
        }
        assert_eq!(seen.len(), 17);
    }

    #[test]
    fn statuses_and_exit_codes() {
        assert_eq!(ErrorCode::Usage.exit_code(), 2);
        assert_eq!(ErrorCode::UnsupportedFeature.status(), Status::Unsupported);
        assert_eq!(ErrorCode::UnsupportedVersion.exit_code(), 3);
        assert_eq!(ErrorCode::Cancelled.exit_code(), 4);
        assert_eq!(ErrorCode::CheckFailed.exit_code(), 1);
        assert_eq!(ErrorCode::Internal.status(), Status::Failed);
    }
}
