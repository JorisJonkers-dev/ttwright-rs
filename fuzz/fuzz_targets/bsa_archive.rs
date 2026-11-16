// SPDX-License-Identifier: GPL-3.0-only
//! A game archive (BSA) read from arbitrary bytes must fail cleanly, never panic or hang.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ttwright_core::BsaHandler;

fuzz_target!(|data: &[u8]| {
    // The first line names the file to look up, the rest is the archive.
    let split = data.iter().position(|&b| b == b'\n').unwrap_or(0);
    let name = String::from_utf8_lossy(&data[..split]);
    let _ = BsaHandler::extract_from_bytes(data.get(split + 1..).unwrap_or_default(), &name);
});
