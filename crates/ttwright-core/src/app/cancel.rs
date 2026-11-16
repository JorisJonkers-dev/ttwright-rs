// SPDX-License-Identifier: GPL-3.0-only
//! Cancellation: a flag the caller sets (on Ctrl+C, or from another thread) and the installer checks between
//! assets. A cancelled install stops cleanly, saves its journal and can be resumed.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A shared cancel flag. Clones share it.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A flag that is not set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the install to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether the install was asked to stop.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::CancelToken;

    #[test]
    fn clones_share_the_flag() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
    }
}
