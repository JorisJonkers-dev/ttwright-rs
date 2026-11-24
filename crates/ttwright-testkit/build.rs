// SPDX-License-Identifier: GPL-3.0-only
//! Writes the catalogue's ids module (catalogue/, read by tally) with tally's generator: a copy that `tally check`
//! keeps equal to the pinned tally's (`tally ids --generator catalogue/tally_ids_gen.rs`).

include!("../../catalogue/tally_ids_gen.rs");

fn main() {
    tally_ids_gen::build("../../catalogue");
}
