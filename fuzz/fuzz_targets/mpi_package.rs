// SPDX-License-Identifier: GPL-3.0-only
//! An MPI package (a TES4 archive) read from arbitrary bytes must fail cleanly, never panic or hang.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ttwright_core::MpiStore;

fuzz_target!(|data: &[u8]| {
    if let Ok(store) = MpiStore::from_bytes(data) {
        let _ = store.get_manifest();
        let _ = store.file_count();
    }
});
