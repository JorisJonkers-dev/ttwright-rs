// SPDX-License-Identifier: GPL-3.0-only
//! Synthetic fixtures for the tests of ttwright: archives, MPI packages, manifests and game copies.
//!
//! Nothing here is game data. Every file is made up in the test that uses it, from a few bytes of text, so the
//! fixtures can be public and the tests never need a real game or package.

mod fixture;

// The feature catalogue's ids (catalogue/, docs/USAGE.md "Feature catalogue"): a test says which manifest features it
// proves with `covers(&[ids::install::features::ASSET_OP_2])`; an id the catalogue does not have does not compile.
include!(concat!(env!("OUT_DIR"), "/tally_ids.rs"));

pub use fixture::{Fixture, LOOSE, PATCH_SOURCE, PATCH_TARGET, crc32_hex, loc};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ba2::CompressableFrom;
use ba2::tes4::{
    Archive, ArchiveFlags, ArchiveKey, ArchiveOptions, Directory, DirectoryKey, File, Version,
};
use serde_json::{Value, json};

/// A folder under the system temporary directory, removed when dropped.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// A new, empty folder whose name starts with `label`.
    ///
    /// # Panics
    /// When the folder cannot be created.
    #[must_use]
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "ttwright-test-{label}-{}-{n}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create a temporary folder");
        Self(path)
    }

    /// The folder.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside the folder.
    #[must_use]
    pub fn join(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Write `bytes` to `path`, creating its parent folders.
///
/// # Panics
/// When the file cannot be written.
pub fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent folders");
    }
    std::fs::write(path, bytes).expect("write a fixture file");
}

/// An uncompressed TES4 archive (BSA, version 104) holding `files` as `(path, bytes)`. Paths take either
/// separator; the folder is everything before the last one.
///
/// # Panics
/// When the archive cannot be written (never for valid input).
#[must_use]
pub fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = Archive::new();
    for (path, bytes) in files {
        let path = path.replace('/', "\\");
        let (folder, name) = path.rsplit_once('\\').unwrap_or(("", path.as_str()));
        let file = File::from_decompressed(bytes.to_vec().into_boxed_slice());
        let key = ArchiveKey::from(folder.as_bytes());
        let name = DirectoryKey::from(name.as_bytes());
        if let Some(directory) = archive.get_mut(&key) {
            directory.insert(name, file);
        } else {
            let mut directory = Directory::default();
            directory.insert(name, file);
            archive.insert(key, directory);
        }
    }
    let options = ArchiveOptions::builder()
        .version(Version::v104)
        .flags(
            ArchiveFlags::DIRECTORY_STRINGS
                | ArchiveFlags::FILE_STRINGS
                | ArchiveFlags::RETAIN_DIRECTORY_NAMES
                | ArchiveFlags::RETAIN_FILE_NAMES,
        )
        .build();
    let mut out = Vec::new();
    archive
        .write(&mut out, &options)
        .expect("write a synthetic archive");
    out
}

/// An MPI package: an archive with `manifest` as `_package/index.json` and the package's own `files`.
#[must_use]
pub fn mpi_package(manifest: &Value, files: &[(&str, &[u8])]) -> Vec<u8> {
    let manifest = serde_json::to_vec(manifest).expect("a manifest serialises");
    let mut all: Vec<(&str, &[u8])> = vec![("_package/index.json", &manifest)];
    all.extend_from_slice(files);
    archive(&all)
}

/// A VCDIFF patch that turns `source` into `target`, as the package's `.xd3` files are.
///
/// # Panics
/// Never for in-memory input.
#[must_use]
pub fn vcdiff(source: &[u8], target: &[u8]) -> Vec<u8> {
    oxidelta::compress::encoder::encode_all(
        Vec::new(),
        source,
        target,
        oxidelta::compress::encoder::CompressOptions::default(),
    )
    .expect("encode a synthetic patch")
}

/// A mono 16-bit PCM WAV file of `samples` at `rate` Hz (a sawtooth, not a recording).
///
/// # Panics
/// For clips longer than 2 GiB.
#[must_use]
pub fn wav(rate: u32, samples: usize) -> Vec<u8> {
    let data_len = u32::try_from(samples * 2).expect("a short clip");
    let mut out = Vec::with_capacity(44 + samples * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let v = i16::try_from((i % 200) * 100).unwrap_or(0) - 10_000;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// A folder that passes for a game copy: the executable name the installer looks for, a `Data` folder and
/// whatever `data_files` (paths relative to `Data`) the test needs.
#[must_use]
pub fn game_copy(root: &Path, executable: &str, data_files: &[(&str, &[u8])]) -> PathBuf {
    write(&root.join(executable), b"synthetic executable");
    std::fs::create_dir_all(root.join("Data")).expect("create Data");
    for (path, bytes) in data_files {
        write(&root.join("Data").join(path), bytes);
    }
    root.to_path_buf()
}

/// A manifest in the MPI shape, built field by field. Lists are per profile, as in real manifests: one profile
/// here unless a test sets more.
#[derive(Debug, Clone)]
pub struct Manifest {
    value: Value,
}

impl Manifest {
    /// A manifest with a package title and version and empty lists.
    #[must_use]
    pub fn new(title: &str, version: &str) -> Self {
        Self {
            value: json!({
                "Package": {"Title": title, "Version": version, "Author": "synthetic"},
                "Variables": [[]],
                "Locations": [[]],
                "Tags": [],
                "Assets": [],
                "Checks": [],
                "FileAttrs": [],
                "PostCommands": [],
            }),
        }
    }

    fn push(&mut self, key: &str, item: Value) -> &mut Self {
        self.value[key]
            .as_array_mut()
            .expect("an array field")
            .push(item);
        self
    }

    fn push_location(&mut self, item: Value) -> usize {
        let list = self.value["Locations"][0]
            .as_array_mut()
            .expect("profile 0");
        list.push(item);
        list.len() - 1
    }

    /// Add a variable to profile 0.
    pub fn variable(&mut self, name: &str, kind: i32, value: Option<&str>) -> &mut Self {
        let item = json!({"Name": name, "Type": kind, "Value": value});
        self.value["Variables"][0]
            .as_array_mut()
            .expect("profile 0")
            .push(item);
        self
    }

    /// Add a location to profile 0 and return its index.
    pub fn location(&mut self, name: &str, kind: i32, value: &str) -> usize {
        self.push_location(
            json!({"Name": name, "Type": kind, "Value": value, "CreateFolder": kind == 0}),
        )
    }

    /// Add an archive written to (location type 2) and return its index.
    pub fn archive_target(&mut self, name: &str, value: &str) -> usize {
        self.push_location(json!({
            "Name": name, "Type": 2, "Value": value,
            "ArchiveType": 104, "ArchiveFlags": 1795, "FilesFlags": 0, "ArchiveCompressed": false,
        }))
    }

    /// Add an asset: `[tags, op, params, status, source location, target location, source path, target path?]`.
    pub fn asset(
        &mut self,
        op: i32,
        params: &str,
        source: usize,
        target: usize,
        source_path: &str,
        target_path: Option<&str>,
    ) -> &mut Self {
        let mut item = json!([1, op, params, 0, source, target, source_path]);
        if let Some(path) = target_path {
            item.as_array_mut().expect("an array").push(json!(path));
        }
        self.push("Assets", item)
    }

    /// Add a check.
    pub fn check(&mut self, check: Value) -> &mut Self {
        self.push("Checks", check)
    }

    /// Add a post-installation command.
    pub fn post_command(&mut self, command: &str) -> &mut Self {
        self.push(
            "PostCommands",
            json!({"Value": command, "Wait": true, "Hidden": true}),
        )
    }

    /// Add a file time.
    pub fn file_attr(&mut self, path: &str, last_modified: &str) -> &mut Self {
        self.push(
            "FileAttrs",
            json!({"Value": path, "LastModified": last_modified}),
        )
    }

    /// Set any top-level field (for tests of unknown fields).
    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        self.value[key] = value;
        self
    }

    /// The manifest as JSON.
    #[must_use]
    pub fn build(&self) -> Value {
        self.value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ba2::{ByteSlice, Reader};

    #[test]
    fn an_archive_reads_back() {
        let bytes = archive(&[
            ("meshes\\a.nif", b"a"),
            ("textures/b.dds", b"bb"),
            ("top.txt", b"t"),
        ]);
        let (archive, _) = Archive::read(ba2::Borrowed(&bytes)).unwrap();
        let mut names = Vec::new();
        for (dir, folder) in &archive {
            for (file, data) in folder {
                names.push((
                    format!(
                        "{}\\{}",
                        String::from_utf8_lossy(dir.name().as_bytes()),
                        String::from_utf8_lossy(file.name().as_bytes())
                    ),
                    data.as_bytes().len(),
                ));
            }
        }
        assert_eq!(names.len(), 3);
        assert!(names.contains(&("meshes\\a.nif".into(), 1)));
    }

    #[test]
    fn a_patch_is_vcdiff() {
        let patch = vcdiff(b"source bytes", b"target bytes");
        assert_eq!(&patch[..3], &[0xD6, 0xC3, 0xC4]);
        let back = oxidelta::compress::decoder::decode_all(b"source bytes", &patch).unwrap();
        assert_eq!(back, b"target bytes");
    }

    #[test]
    fn the_manifest_builder_fills_profile_zero() {
        let mut m = Manifest::new("Synthetic", "1.0");
        m.variable("FO3ROOT", 4, Some("HKLM\\x"));
        let data = m.location("Data", 0, "%DESTINATION%");
        let bsa = m.archive_target("A.bsa", "%DESTINATION%\\A.bsa");
        m.asset(1, "", data, bsa, "x", Some("y"))
            .post_command("cmd.exe /C del \"%DESTINATION%\\x\"")
            .file_attr("%DESTINATION%\\y", "2010-01-01T00:00:00+00:00")
            .check(json!({"Type": 1, "Loc": 0, "FreeSize": 1}))
            .set("Extra", json!(1));
        let v = m.build();
        assert_eq!((data, bsa), (0, 1));
        assert_eq!(v["Variables"][0][0]["Name"], "FO3ROOT");
        assert_eq!(v["Assets"][0].as_array().unwrap().len(), 8);
        assert_eq!(v["Extra"], 1);
        let package = mpi_package(&v, &[("data/x", b"x")]);
        assert!(package.starts_with(b"BSA\0"));
    }

    #[test]
    fn a_game_copy_and_a_wav_and_a_temp_dir() {
        let dir = TempDir::new("kit");
        let root = game_copy(&dir.join("game"), "Game.exe", &[("a.esm", b"esm")]);
        assert!(root.join("Game.exe").is_file());
        assert_eq!(std::fs::read(root.join("Data/a.esm")).unwrap(), b"esm");
        let wav = wav(22_050, 100);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 200);
        let path = dir.path().to_path_buf();
        drop(dir);
        assert!(!path.exists());
    }
}
