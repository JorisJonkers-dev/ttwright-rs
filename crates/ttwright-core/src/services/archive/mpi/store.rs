use anyhow::{Context, Result};
use ba2::tes4::Archive;
use ba2::{Borrowed, ByteSlice, Reader};
use indicatif::ProgressBar;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::warn;

use super::LZ4_FRAME_MAGIC;
use super::archive_progress_style;
use super::extractor::MpiExtractor;
use super::normalize_mpi_path;

/// In-memory MPI package store.
///
/// Holds all extracted files in a HashMap for instant lookups, avoiding a
/// 27k+ file extraction pass and case-insensitive filesystem crawling.
pub struct MpiStore {
    /// Files stored by lowercase normalized path -> data
    files: HashMap<String, Vec<u8>>,
    /// Total bytes stored
    total_bytes: usize,
}

impl MpiStore {
    /// Load an MPI file entirely into memory.
    pub fn load(mpi_path: &Path) -> Result<Self> {
        if !mpi_path.exists() {
            anyhow::bail!("MPI file not found: {}", mpi_path.display());
        }

        tracing::info!(
            "\nLoading MPI package into memory: {}",
            mpi_path.file_name().unwrap_or_default().to_string_lossy()
        );

        let (archive, options) = Archive::read(mpi_path).context("Failed to open MPI archive")?;
        let store = Self::from_archive(&archive, &options, true);

        tracing::info!(
            "  MPI loaded: {} files, {:.1} MB in memory",
            store.files.len(),
            store.total_bytes as f64 / 1024.0 / 1024.0
        );

        Ok(store)
    }

    /// Read an MPI package held in memory. Entries that do not decompress are left out, as [`Self::load`] does.
    ///
    /// # Errors
    /// When the bytes are not an MPI (TES4 archive) at all.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let (archive, options) =
            Archive::read(Borrowed(bytes)).context("Failed to open MPI archive")?;
        Ok(Self::from_archive(&archive, &options, false))
    }

    fn from_archive(
        archive: &Archive<'_>,
        options: &ba2::tes4::ArchiveOptions,
        show_progress: bool,
    ) -> Self {
        let compression_options: ba2::tes4::FileCompressionOptions = options.into();

        struct FileEntry<'a> {
            path: String,
            file: &'a ba2::tes4::File<'a>,
        }

        let mut entries: Vec<FileEntry> = Vec::new();
        for (dir_key, folder) in archive.iter() {
            let dir_name = String::from_utf8_lossy(dir_key.name().as_bytes()).replace('\\', "/");
            for (file_key, file) in folder.iter() {
                let file_name = String::from_utf8_lossy(file_key.name().as_bytes()).to_string();
                let path = if dir_name.is_empty() || dir_name == "." {
                    file_name
                } else {
                    format!("{}/{}", dir_name, file_name)
                };
                entries.push(FileEntry { path, file });
            }
        }

        let total_files = entries.len();
        let pb = if show_progress {
            tracing::info!("  {} files in archive, decompressing...", total_files);
            let pb = ProgressBar::new(total_files as u64);
            pb.set_style(archive_progress_style());
            pb
        } else {
            ProgressBar::hidden()
        };

        let files: Mutex<HashMap<String, Vec<u8>>> =
            Mutex::new(HashMap::with_capacity(total_files));
        let total_bytes = AtomicUsize::new(0);

        entries.par_iter().for_each(|entry| {
            let data = if entry.file.is_compressed() {
                let compressed = entry.file.as_bytes();
                if compressed.len() >= 4 && compressed[0..4] == LZ4_FRAME_MAGIC {
                    MpiExtractor::decompress_lz4_frame(compressed).ok()
                } else {
                    entry
                        .file
                        .decompress(&compression_options)
                        .map(|d| d.as_bytes().to_vec())
                        .ok()
                }
            } else {
                Some(entry.file.as_bytes().to_vec())
            };

            if let Some(data) = data {
                let key = normalize_mpi_path(&entry.path);
                match files.lock() {
                    Ok(mut files) => {
                        total_bytes.fetch_add(data.len(), Ordering::Relaxed);
                        files.insert(key, data);
                    }
                    Err(_) => warn!("MPI store lock poisoned while loading {}", entry.path),
                }
            }

            pb.inc(1);
        });

        pb.finish_with_message("Loaded into memory");
        Self {
            files: files
                .into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            total_bytes: total_bytes.load(Ordering::Relaxed),
        }
    }

    /// Get a file by path (case-insensitive, handles both / and \ separators).
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        let normalized = normalize_mpi_path(path);
        self.files.get(&normalized).map(|v| v.as_slice())
    }

    /// Get the manifest file (tries common locations).
    pub fn get_manifest(&self) -> Option<&[u8]> {
        for name in ["_package/index.json", "manifest.json", "index.json"] {
            if let Some(data) = self.get(name) {
                return Some(data);
            }
        }
        None
    }

    /// Total bytes stored in memory.
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Number of files stored.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::MpiStore;
    use crate::services::BsaBuilder;

    #[test]
    fn a_package_in_memory_reads_back_case_insensitively() {
        let mut builder = BsaBuilder::new();
        builder.add_file(
            "_package\\index.json",
            br#"{"Package":{"Title":"Synthetic"}}"#.to_vec(),
        );
        builder.add_file("data/Synthetic.esm.xd3", vec![1, 2, 3]);
        let store = MpiStore::from_bytes(&builder.build_to_bytes().unwrap()).unwrap();
        assert_eq!(store.file_count(), 2);
        assert_eq!(store.total_bytes(), 33 + 3);
        assert_eq!(store.get("DATA\\synthetic.ESM.xd3"), Some(&[1u8, 2, 3][..]));
        assert!(store.get_manifest().unwrap().starts_with(b"{\"Package\""));
        assert!(store.get("data/other").is_none());
    }

    #[test]
    fn garbage_is_not_a_package() {
        assert!(MpiStore::from_bytes(b"BSA\0garbage").is_err());
        assert!(MpiStore::from_bytes(&[]).is_err());
    }
}

#[cfg(test)]
mod disk_tests {
    use super::MpiStore;
    use crate::services::BsaBuilder;

    #[test]
    fn a_package_on_disk_loads_like_one_in_memory() {
        let dir = std::env::temp_dir().join(format!("ttwright_mpi_store_{}", std::process::id()));
        let path = dir.join("synthetic.mpi");
        let mut builder = BsaBuilder::new();
        builder.add_file("_package/index.json", b"{}".to_vec());
        builder.build(&path).unwrap();
        let store = MpiStore::load(&path).unwrap();
        assert_eq!(store.get_manifest(), Some(&b"{}"[..]));
        assert!(MpiStore::load(&dir.join("missing.mpi")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_empty_builder_builds_nothing() {
        assert!(BsaBuilder::new().build_to_bytes().is_err());
    }
}
