// SPDX-License-Identifier: GPL-3.0-only
//! A whole synthetic install: a Fallout 3 copy, an MPI package whose manifest uses every supported operation,
//! and a destination. The bytes are made up; only the shapes are those of a real package.

use std::path::PathBuf;

use serde_json::json;

use crate::{Manifest, TempDir, archive, game_copy, mpi_package, vcdiff, wav, write};

/// The bytes of the patch source in the game copy and of the file the patch makes.
pub const PATCH_SOURCE: &[u8] = b"synthetic source bytes, version one";
/// What the patch turns [`PATCH_SOURCE`] into.
pub const PATCH_TARGET: &[u8] = b"synthetic target bytes, version two";
/// The content of the loose file in the game copy.
pub const LOOSE: &[u8] = b"loose synthetic file";

/// Location indices of [`Fixture::manifest`].
pub mod loc {
    /// The game copy's `Data` folder.
    pub const GAME_DATA: usize = 0;
    /// An archive in the game copy.
    pub const GAME_ARCHIVE: usize = 1;
    /// The destination folder.
    pub const DEST: usize = 2;
    /// An archive written into the destination.
    pub const DEST_ARCHIVE: usize = 3;
}

/// A synthetic install laid out in a temporary folder.
#[derive(Debug)]
pub struct Fixture {
    /// The temporary folder; removed on drop.
    pub dir: TempDir,
    /// The `.mpi` file.
    pub mpi: PathBuf,
    /// The Fallout 3 copy.
    pub fallout3: PathBuf,
    /// Where to install.
    pub dest: PathBuf,
}

impl Fixture {
    /// The manifest: a registry variable for the game, four locations, one asset of each supported operation
    /// (copy from a folder, copy from an archive, new, patch, audio encode, copy into a written archive), three
    /// checks (CRC32 checksum, free space, path), a rename and a file time.
    #[must_use]
    pub fn manifest() -> Manifest {
        let mut m = Manifest::new("Tale of Two Wastelands", "3.4.0");
        m.variable(
            "FO3ROOT",
            4,
            Some("HKLM\\Software\\Synthetic\\Installed Path"),
        );
        m.location("FO3 Data", 0, "%FO3DATA%");
        m.location("Synthetic.bsa", 1, "%FO3DATA%\\Synthetic.bsa");
        m.location("Destination", 0, "%DESTINATION%");
        m.archive_target("Out.bsa", "%DESTINATION%\\Out.bsa");
        m.asset(0, "", loc::GAME_DATA, loc::DEST, "loose.txt", None)
            .asset(0, "", loc::GAME_ARCHIVE, loc::DEST, "meshes\\a.nif", None)
            .asset(1, "", loc::DEST, loc::DEST, "new.txt", None)
            .asset(2, "", loc::GAME_DATA, loc::DEST, "patchme.bin", Some("patched.bin"))
            .asset(
                5,
                "-f:22050 -q:5",
                loc::GAME_DATA,
                loc::DEST,
                "sound\\a.wav",
                Some("sound\\a.ogg"),
            )
            .asset(
                0,
                "",
                loc::GAME_DATA,
                loc::DEST_ARCHIVE,
                "loose.txt",
                Some("textures\\in-archive.txt"),
            )
            .check(json!({
                "Type": 0, "Inverted": false, "Loc": loc::GAME_DATA, "File": "loose.txt",
                "Checksums": crc32_hex(LOOSE), "CustomMessage": "the game copy is not the expected one",
            }))
            .check(json!({"Type": 1, "Inverted": false, "Loc": loc::DEST, "FreeSize": 1}))
            .check(json!({"Type": 2, "Inverted": false, "Loc": loc::DEST}))
            .post_command("cmd.exe /C ren \"%DESTINATION%\\new.txt\" \"renamed.txt\"")
            .file_attr("%DESTINATION%\\Out.bsa", "2008-10-01T12:00:00+00:00");
        m
    }

    /// The fixture with [`Self::manifest`].
    #[must_use]
    pub fn new(label: &str) -> Self {
        Self::with_manifest(label, &Self::manifest())
    }

    /// The fixture with another manifest.
    ///
    /// # Panics
    /// When the files cannot be written.
    #[must_use]
    pub fn with_manifest(label: &str, manifest: &Manifest) -> Self {
        let dir = TempDir::new(label);
        let fallout3 = game_copy(
            &dir.join("Fallout 3"),
            "Fallout3.exe",
            &[
                ("loose.txt", LOOSE),
                ("patchme.bin", PATCH_SOURCE),
                ("sound/a.wav", &wav(44_100, 4_410)),
                (
                    "Synthetic.bsa",
                    &archive(&[("meshes\\a.nif", b"synthetic mesh")]),
                ),
            ],
        );
        let package = mpi_package(
            &manifest.build(),
            &[
                ("new.txt", b"new synthetic file"),
                ("patched.bin.xd3", &vcdiff(PATCH_SOURCE, PATCH_TARGET)),
            ],
        );
        let mpi = dir.join("package/Synthetic.mpi");
        write(&mpi, &package);
        let dest = dir.join("out");
        Self {
            dir,
            mpi,
            fallout3,
            dest,
        }
    }
}

/// CRC-32 (IEEE) as 8 lower-case hex digits, as a manifest's checksums are written.
#[must_use]
pub fn crc32_hex(data: &[u8]) -> String {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    format!("{:08x}", !crc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_of_known_text() {
        // The check value of CRC-32/ISO-HDLC.
        assert_eq!(crc32_hex(b"123456789"), "cbf43926");
    }

    #[test]
    fn the_fixture_lays_out_its_files() {
        let f = Fixture::new("fixture");
        assert!(f.mpi.is_file());
        assert!(f.fallout3.join("Data/Synthetic.bsa").is_file());
        assert!(!f.dest.exists());
        assert_eq!(
            Fixture::manifest().build()["Assets"]
                .as_array()
                .unwrap()
                .len(),
            6
        );
    }
}
