// SPDX-License-Identifier: GPL-3.0-only
//! The documents `ttwright` prints for its callers (ADR-0009 of the Toolbox: the producer owns its Contracts).
//!
//! - [`ResultDoc`], Contract `ttwright-result` v1: one JSON document on stdout at the end of `install --json`,
//!   also when the run failed or was cancelled.
//! - [`EventLine`], Contract `ttwright-events` v1: newline-delimited JSON progress events on stderr, or in the
//!   file named by `--events`.
//! - [`Handshake`]: the answer of `ttwright contracts --json`, the Contracts and versions this build speaks and the
//!   TTW versions and manifest features it supports.
//!
//! Every document carries an integer `schema_version`. A reader knows a newest version N, accepts N and N-1 and
//! refuses N+1 ([`check`]). Adding an optional field is not a new version; removing, renaming or retyping one is.

mod error;
mod event;
mod result;
mod schema;
mod supports;

pub use error::ErrorCode;
pub use event::{Event, EventLine, Phase};
pub use result::{Counts, ErrorInfo, PackageInfo, ResultDoc, Status};
pub use schema::{DocumentKind, SchemaError, check, schema_file_name, schema_json};
pub use supports::{
    FEATURES, Feature, SupportedPackage, Supports, TTW_CURRENT_RELEASE, TTW_VERSIONS,
    VersionSupport, is_supported, supports, ttw_version_support,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The name of the result Contract.
pub const RESULT_CONTRACT: &str = "ttwright-result";
/// The result versions this build can print, oldest first.
pub const RESULT_VERSIONS: &[u32] = &[1];
/// The name of the event Contract.
pub const EVENTS_CONTRACT: &str = "ttwright-events";
/// The event versions this build can print, oldest first.
pub const EVENTS_VERSIONS: &[u32] = &[1];
/// The name of the handshake document (`ttwright contracts --json`).
pub const HANDSHAKE_CONTRACT: &str = "ttwright-contracts";
/// The handshake versions this build can print, oldest first.
pub const HANDSHAKE_VERSIONS: &[u32] = &[1];
/// The name of the in-process library API (`ttwright_core::api`), listed by `ttwright contracts --json`.
pub const API_CONTRACT: &str = "ttwright-api";
/// The version of the in-process library API. It changes when a call, a type or an error code of that API
/// changes in a way an existing caller would notice.
pub const API_VERSION: u32 = 1;

/// The tool that printed a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolInfo {
    /// Always `ttwright`.
    pub name: String,
    /// The release version, for example `0.3.0`.
    pub version: String,
}

impl ToolInfo {
    /// `ttwright` at `version`.
    #[must_use]
    pub fn new(version: &str) -> Self {
        Self {
            name: "ttwright".to_owned(),
            version: version.to_owned(),
        }
    }
}

/// One Contract this build speaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContractInfo {
    /// The Contract name, for example `ttwright-result`.
    pub name: String,
    /// The schema versions of that Contract this build can print, oldest first.
    pub versions: Vec<u32>,
    /// For the result Contract: the packages, TTW versions and manifest features an install supports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports: Option<Supports>,
}

/// The answer of `ttwright contracts --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Handshake {
    /// The handshake version; 1.
    #[schemars(extend("const" = 1))]
    pub schema_version: u32,
    /// Always `ttwright-contracts`.
    #[schemars(extend("const" = "ttwright-contracts"))]
    pub contract: String,
    /// The tool that answered.
    pub tool: ToolInfo,
    /// The Contracts, result first.
    pub contracts: Vec<ContractInfo>,
}

impl Handshake {
    /// What this build speaks, for a tool at `version`.
    #[must_use]
    pub fn current(version: &str) -> Self {
        Self {
            schema_version: 1,
            contract: HANDSHAKE_CONTRACT.to_owned(),
            tool: ToolInfo::new(version),
            contracts: vec![
                ContractInfo {
                    name: RESULT_CONTRACT.to_owned(),
                    versions: RESULT_VERSIONS.to_vec(),
                    supports: Some(supports()),
                },
                ContractInfo {
                    name: EVENTS_CONTRACT.to_owned(),
                    versions: EVENTS_VERSIONS.to_vec(),
                    supports: None,
                },
                ContractInfo {
                    name: API_CONTRACT.to_owned(),
                    versions: vec![API_VERSION],
                    supports: None,
                },
            ],
        }
    }
}

/// A document as one line of JSON (no newline).
///
/// # Panics
/// Never: the documents of this crate serialise to JSON.
#[must_use]
pub fn to_line<T: Serialize>(document: &T) -> String {
    serde_json::to_string(document).expect("a Contract document serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handshake_names_both_contracts_and_the_supported_ttw_versions() {
        let h = Handshake::current("1.2.3");
        assert_eq!(h.schema_version, 1);
        assert_eq!(h.tool, ToolInfo::new("1.2.3"));
        let names: Vec<_> = h.contracts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["ttwright-result", "ttwright-events", "ttwright-api"]
        );
        let supports = h.contracts[0].supports.as_ref().unwrap();
        assert!(
            supports
                .packages
                .iter()
                .any(|p| p.versions.contains(&"3.4".to_owned()))
        );
        assert!(h.contracts[1].supports.is_none());
        let line = to_line(&h);
        assert!(line.starts_with(r#"{"schema_version":1,"contract":"ttwright-contracts""#));
        assert!(!line.contains('\n'));
    }
}
