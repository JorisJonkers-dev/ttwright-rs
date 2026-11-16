// SPDX-License-Identifier: GPL-3.0-only
//! Events v1: one JSON object per line, on stderr or in the file named by `--events`. Each line carries
//! `schema_version` and an `event` tag. A reader ignores fields and events it does not know.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ErrorCode, Status};

/// The phases of an install, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Reading the MPI package and its manifest.
    Package,
    /// Checking the manifest's features, version and game copies.
    Preflight,
    /// The manifest's pre-installation checks.
    Checks,
    /// Asset operations and archive writing.
    Assets,
    /// Post-installation commands and file times.
    PostInstall,
}

/// One event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The manifest was read.
    Started {
        /// The manifest's title.
        package: String,
        /// The manifest's version.
        version: String,
        /// Asset operations in the package.
        assets_total: u64,
        /// This run continues an interrupted one.
        resumed: bool,
    },
    /// A phase began.
    Phase {
        /// The phase.
        phase: Phase,
    },
    /// Progress within the current phase.
    Progress {
        /// The phase.
        phase: Phase,
        /// Items done so far.
        done: u64,
        /// Items in total.
        of: u64,
        /// What is being worked on, for people.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        message: String,
    },
    /// Something did not go as planned, but the run goes on.
    Warning {
        /// For people.
        message: String,
    },
    /// The run ended; the result document follows on stdout.
    Finished {
        /// How it ended.
        status: Status,
        /// Set unless the status is `success`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<ErrorCode>,
    },
}

/// One line of the event stream: the event and its schema version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventLine {
    /// The event version; 1.
    #[schemars(extend("const" = 1))]
    pub schema_version: u32,
    /// The event.
    #[serde(flatten)]
    pub event: Event,
}

impl EventLine {
    /// The current version of `event`.
    #[must_use]
    pub fn new(event: Event) -> Self {
        Self {
            schema_version: 1,
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_has_the_version_and_the_tag() {
        let line = crate::to_line(&EventLine::new(Event::Progress {
            phase: Phase::Assets,
            done: 3,
            of: 9,
            message: String::new(),
        }));
        assert_eq!(
            line,
            r#"{"schema_version":1,"event":"progress","phase":"assets","done":3,"of":9}"#
        );
    }

    #[test]
    fn a_line_reads_back_and_ignores_unknown_fields() {
        let line: EventLine = serde_json::from_str(
            r#"{"schema_version":1,"event":"finished","status":"cancelled","code":"cancelled","eta":3}"#,
        )
        .unwrap();
        assert_eq!(
            line.event,
            Event::Finished {
                status: Status::Cancelled,
                code: Some(ErrorCode::Cancelled)
            }
        );
    }
}
