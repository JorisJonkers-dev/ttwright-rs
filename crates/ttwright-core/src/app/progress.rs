// SPDX-License-Identifier: GPL-3.0-only
use ttwright_contract::{Event, Phase};

/// What the installer reports while it runs: log lines for people, and the Contract events for callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallEvent {
    /// A line for the log.
    Log(String),
    /// A `ttwright-events` event.
    Event(Event),
}

impl InstallEvent {
    /// A log line.
    pub fn log(message: impl Into<String>) -> Self {
        Self::Log(message.into())
    }

    /// Progress within `phase`.
    pub fn progress(phase: Phase, done: u64, of: u64, message: impl Into<String>) -> Self {
        Self::Event(Event::Progress {
            phase,
            done,
            of,
            message: message.into(),
        })
    }

    /// The start of `phase`.
    #[must_use]
    pub fn phase(phase: Phase) -> Self {
        Self::Event(Event::Phase { phase })
    }

    /// A warning: the run goes on.
    pub fn warning(message: impl Into<String>) -> Self {
        Self::Event(Event::Warning {
            message: message.into(),
        })
    }
}
