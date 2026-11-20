// SPDX-License-Identifier: GPL-3.0-only

//! One reading of the machine's RAM, in bytes.
//!
//! sysinfo 0.40 made `System::new` fallible. Before that, a platform sysinfo could not read reported
//! zero bytes, so a failed probe reads as zero here too and callers keep their behaviour.

use sysinfo::{Error, System};

/// Total and available system RAM, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SystemMemory {
    pub total: u64,
    pub available: u64,
}

impl SystemMemory {
    /// Read the current memory figures; zero for both when the platform cannot be probed.
    pub(crate) fn read() -> Self {
        Self::or_zero(Self::probe())
    }

    fn probe() -> Result<Self, Error> {
        let mut sys = System::new()?;
        sys.refresh_memory();
        Ok(Self {
            total: sys.total_memory(),
            available: sys.available_memory(),
        })
    }

    fn or_zero(probe: Result<Self, Error>) -> Self {
        probe.unwrap_or_default()
    }

    pub(crate) fn available_gb(self) -> f64 {
        self.available as f64 / 1024.0 / 1024.0 / 1024.0
    }

    pub(crate) fn total_gb(self) -> f64 {
        self.total as f64 / 1024.0 / 1024.0 / 1024.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_probe_keeps_its_figures() {
        let probed = SystemMemory {
            total: 8,
            available: 3,
        };
        assert_eq!(SystemMemory::or_zero(Ok(probed)), probed);
    }

    #[test]
    fn a_failed_probe_reads_as_zero_bytes() {
        let failed = SystemMemory::or_zero(Err(Error::Unsupported));
        assert_eq!(
            failed,
            SystemMemory {
                total: 0,
                available: 0
            }
        );
    }

    #[test]
    fn a_gigabyte_converts_to_one_gigabyte() {
        let one_gb = SystemMemory {
            total: 1 << 30,
            available: 1 << 30,
        };
        assert_eq!((one_gb.total_gb(), one_gb.available_gb()), (1.0, 1.0));
    }

    #[test]
    fn this_machine_reports_some_memory() {
        let memory = SystemMemory::read();
        assert!(memory.total > 0 && memory.available <= memory.total);
    }
}
