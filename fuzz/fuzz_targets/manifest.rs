// SPDX-License-Identifier: GPL-3.0-only
//! An MPI manifest (`_package/index.json`) from arbitrary bytes must parse or fail cleanly.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ttwright_core::{ManifestLoader, TtwManifest};

fuzz_target!(|data: &[u8]| {
    if let Ok(manifest) = serde_json::from_slice::<TtwManifest>(data) {
        let _ = ManifestLoader::parse_assets(&manifest);
        let profile = ManifestLoader::select_best_profile(&manifest);
        let _ = ManifestLoader::get_locations(&manifest, profile);
        let _ = ManifestLoader::get_variables(&manifest, profile);
        let _ = ManifestLoader::get_bsa_target_locations(&manifest);
    }
});
