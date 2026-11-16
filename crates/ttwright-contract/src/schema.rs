// SPDX-License-Identifier: GPL-3.0-only
//! JSON Schemas of the documents, and the check that refuses a document of a version this build does not speak.

use std::fmt;

use schemars::schema_for;
use serde_json::Value;

use crate::{
    EVENTS_CONTRACT, EVENTS_VERSIONS, EventLine, HANDSHAKE_CONTRACT, HANDSHAKE_VERSIONS, Handshake,
    RESULT_CONTRACT, RESULT_VERSIONS, ResultDoc,
};

/// Which document a schema or check is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// The result document, `ttwright-result`.
    Result,
    /// One event line, `ttwright-events`.
    Events,
    /// The handshake, `ttwright-contracts`.
    Contracts,
}

impl DocumentKind {
    /// Every kind, in the order the release publishes them.
    pub const ALL: &'static [Self] = &[Self::Result, Self::Events, Self::Contracts];

    /// The Contract name, for example `ttwright-result`.
    #[must_use]
    pub fn contract(self) -> &'static str {
        match self {
            Self::Result => RESULT_CONTRACT,
            Self::Events => EVENTS_CONTRACT,
            Self::Contracts => HANDSHAKE_CONTRACT,
        }
    }

    /// `result`, `events` or `contracts`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Result => "result",
            Self::Events => "events",
            Self::Contracts => "contracts",
        }
    }

    /// Parse `result`, `events` or `contracts`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == text)
    }

    /// The versions of this document this build speaks, oldest first.
    #[must_use]
    pub fn versions(self) -> &'static [u32] {
        match self {
            Self::Result => RESULT_VERSIONS,
            Self::Events => EVENTS_VERSIONS,
            Self::Contracts => HANDSHAKE_VERSIONS,
        }
    }
}

/// Why a document or a requested version was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// The version is not one this build speaks.
    UnsupportedVersion {
        /// The document.
        kind: DocumentKind,
        /// What was asked for or found.
        found: String,
    },
    /// The document does not match the schema of its version.
    Invalid(String),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { kind, found } => write!(
                f,
                "{} schema_version {found} is not supported; this build speaks {:?}",
                kind.contract(),
                kind.versions()
            ),
            Self::Invalid(why) => write!(f, "the document does not match its schema: {why}"),
        }
    }
}

impl std::error::Error for SchemaError {}

/// The file name a release publishes the schema under, for example `ttwright-result.v1.json`.
#[must_use]
pub fn schema_file_name(kind: DocumentKind, version: u32) -> String {
    format!("{}.v{version}.json", kind.contract())
}

/// The JSON Schema of `kind` at `version`, pretty-printed with a final newline.
///
/// # Errors
/// [`SchemaError::UnsupportedVersion`] when this build does not speak `version`.
///
/// # Panics
/// Never: a schema is JSON.
pub fn schema_json(kind: DocumentKind, version: u32) -> Result<String, SchemaError> {
    if !kind.versions().contains(&version) {
        return Err(SchemaError::UnsupportedVersion {
            kind,
            found: version.to_string(),
        });
    }
    let schema = match kind {
        DocumentKind::Result => schema_for!(ResultDoc),
        DocumentKind::Events => schema_for!(EventLine),
        DocumentKind::Contracts => schema_for!(Handshake),
    };
    let mut text = serde_json::to_string_pretty(&schema).expect("a schema serialises");
    text.push('\n');
    Ok(text)
}

/// Check that `value` is a document of `kind` in a version this build speaks.
///
/// # Errors
/// [`SchemaError::UnsupportedVersion`] for a missing or unknown `schema_version`, [`SchemaError::Invalid`] when
/// the document names another Contract or does not have the shape of its version.
pub fn check(kind: DocumentKind, value: &Value) -> Result<(), SchemaError> {
    let known = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .is_some_and(|n| kind.versions().contains(&n));
    if !known {
        return Err(SchemaError::UnsupportedVersion {
            kind,
            found: value
                .get("schema_version")
                .map_or_else(|| "missing".to_owned(), Value::to_string),
        });
    }
    if kind != DocumentKind::Events {
        match value.get("contract").and_then(Value::as_str) {
            Some(name) if name == kind.contract() => {}
            Some(other) => {
                return Err(SchemaError::Invalid(format!(
                    "the document names the Contract {other:?}, not {:?}",
                    kind.contract()
                )));
            }
            None => return Err(SchemaError::Invalid("contract is missing".into())),
        }
    }
    let outcome = match kind {
        DocumentKind::Result => serde_json::from_value::<ResultDoc>(value.clone()).map(drop),
        DocumentKind::Events => serde_json::from_value::<EventLine>(value.clone()).map(drop),
        DocumentKind::Contracts => serde_json::from_value::<Handshake>(value.clone()).map(drop),
    };
    outcome.map_err(|e| SchemaError::Invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, Phase};
    use serde_json::json;

    #[test]
    fn names_and_file_names() {
        for kind in DocumentKind::ALL {
            assert_eq!(DocumentKind::parse(kind.name()), Some(*kind));
        }
        assert_eq!(DocumentKind::parse("plan"), None);
        assert_eq!(
            schema_file_name(DocumentKind::Result, 1),
            "ttwright-result.v1.json"
        );
    }

    #[test]
    fn schemas_exist_only_for_spoken_versions() {
        for kind in DocumentKind::ALL {
            let text = schema_json(*kind, 1).unwrap();
            assert!(text.ends_with("}\n"));
            let value: Value = serde_json::from_str(&text).unwrap();
            assert!(
                value["properties"]["schema_version"].is_object(),
                "{kind:?}"
            );
        }
        let err = schema_json(DocumentKind::Result, 2).unwrap_err();
        assert!(
            err.to_string()
                .contains("schema_version 2 is not supported")
        );
    }

    #[test]
    fn documents_of_this_build_pass_the_check() {
        let result = serde_json::to_value(ResultDoc::new("0.3.0")).unwrap();
        check(DocumentKind::Result, &result).unwrap();
        let handshake = serde_json::to_value(Handshake::current("0.3.0")).unwrap();
        check(DocumentKind::Contracts, &handshake).unwrap();
        let event = serde_json::to_value(EventLine::new(Event::Phase {
            phase: Phase::Checks,
        }))
        .unwrap();
        check(DocumentKind::Events, &event).unwrap();
    }

    #[test]
    fn the_check_refuses_other_versions_contracts_and_shapes() {
        let mut result = serde_json::to_value(ResultDoc::new("0.3.0")).unwrap();
        result["schema_version"] = json!(2);
        assert!(matches!(
            check(DocumentKind::Result, &result),
            Err(SchemaError::UnsupportedVersion { .. })
        ));
        result["schema_version"] = json!(1);
        result["contract"] = json!("lodwright-result");
        assert!(
            check(DocumentKind::Result, &result)
                .unwrap_err()
                .to_string()
                .contains("names the Contract")
        );
        result.as_object_mut().unwrap().remove("contract");
        assert!(check(DocumentKind::Result, &result).is_err());
        let missing = check(DocumentKind::Events, &json!({"event": "phase"})).unwrap_err();
        assert!(missing.to_string().contains("missing"));
        let bad = check(
            DocumentKind::Events,
            &json!({"schema_version": 1, "event": "nope"}),
        );
        assert!(matches!(bad, Err(SchemaError::Invalid(_))));
    }
}
