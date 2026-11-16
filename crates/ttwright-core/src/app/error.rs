// SPDX-License-Identifier: GPL-3.0-only
//! An install failure with a stable code ([`ErrorCode`]). It travels inside `anyhow::Error`; [`classify`] finds it
//! again, so callers never match on message text.

use std::fmt;

use ttwright_contract::ErrorCode;

/// Why an install stopped: a code callers match on, prose for people, and what was missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallError {
    /// The stable code.
    pub code: ErrorCode,
    /// For people.
    pub message: String,
    /// The feature, version or game copy that is missing, for the codes that name one.
    pub feature: Option<String>,
}

impl InstallError {
    /// A failure with `code` and `message`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            feature: None,
        }
    }

    /// An `unsupported_feature` failure naming `feature`, for example `asset_op:3`.
    pub fn unsupported(feature: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UnsupportedFeature,
            message: message.into(),
            feature: Some(feature.into()),
        }
    }

    /// The same failure naming `feature`.
    #[must_use]
    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.feature = Some(feature.into());
        self
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InstallError {}

/// The code of any error: an [`InstallError`] anywhere in the chain keeps its own; a file system error is `io`;
/// anything else is `internal`.
#[must_use]
pub fn classify(error: &anyhow::Error) -> InstallError {
    for cause in error.chain() {
        if let Some(install) = cause.downcast_ref::<InstallError>() {
            let mut found = install.clone();
            // Keep the outer context: it says where it happened.
            found.message = format!("{error:#}");
            return found;
        }
    }
    let code = if error.chain().any(|c| c.is::<std::io::Error>()) {
        ErrorCode::Io
    } else {
        ErrorCode::Internal
    };
    InstallError::new(code, format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn a_coded_error_is_found_through_context() {
        let inner: anyhow::Result<()> = Err(InstallError::unsupported("asset_op:3", "op 3").into());
        let err = inner.context("while reading the manifest").unwrap_err();
        let found = classify(&err);
        assert_eq!(found.code, ErrorCode::UnsupportedFeature);
        assert_eq!(found.feature.as_deref(), Some("asset_op:3"));
        assert_eq!(found.message, "while reading the manifest: op 3");
    }

    #[test]
    fn io_and_other_errors() {
        let io = anyhow::Error::from(std::io::Error::other("disk"));
        assert_eq!(classify(&io).code, ErrorCode::Io);
        assert_eq!(classify(&anyhow::anyhow!("odd")).code, ErrorCode::Internal);
        let e = InstallError::new(ErrorCode::GamePathMissing, "no copy").with_feature("fo3");
        assert_eq!(e.to_string(), "no copy");
        assert_eq!(e.feature.as_deref(), Some("fo3"));
    }
}
