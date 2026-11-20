use anyhow::{Context, Result};
use ba2::tes4::Archive;
use ba2::{ByteSlice, Reader};
use indicatif::ProgressBar;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::LZ4_FRAME_MAGIC;
use super::archive_progress_style;
use crate::services::path_utils::safe_join;
/// Extracts .mpi files (BSA archives) to a temporary directory
pub struct MpiExtractor;

impl MpiExtractor {
    /// Check if the path is a package file that needs extraction: a `.mpi` file, or any file that starts with the
    /// package's `BSA\0` header, whatever its name.
    pub fn is_mpi_file(path: &Path) -> bool {
        path.is_file()
            && (path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("mpi"))
                .unwrap_or(false)
                || Self::header(path).is_some_and(|h| h.starts_with(b"BSA\0")))
    }

    fn header(path: &Path) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut buf = Vec::new();
        fs::File::open(path)
            .ok()?
            .take(8)
            .read_to_end(&mut buf)
            .ok()?;
        Some(buf)
    }

    /// What to tell the user about an existing file that is not a package, by its header: what it is and what to
    /// do. `None` when `path` is not an existing file.
    pub fn not_a_package_message(path: &Path) -> Option<String> {
        if !path.is_file() {
            return None;
        }
        let header = Self::header(path).unwrap_or_default();
        let kind = if header.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
            Some("a 7-Zip archive")
        } else if header.starts_with(b"PK\x03\x04") {
            Some("a ZIP archive")
        } else if header.starts_with(b"Rar!") {
            Some("a RAR archive")
        } else {
            None
        };
        let name = path.display();
        Some(match kind {
            Some(kind) => format!(
                "{name} is {kind}, not a TTW package. If it is the download, extract it and give the .mpi file inside it"
            ),
            None => format!(
                "{name} is not a TTW package: it is not a .mpi file and does not start with the package header (BSA\\0). \
                 If it is the download archive, extract it and give the .mpi file inside it"
            ),
        })
    }

    /// Extract .mpi file to a directory
    /// If output_dir is None, extracts to a subdirectory next to the MPI file
    /// Returns the path to the extracted directory
    pub fn extract_to_temp(mpi_path: &Path) -> Result<PathBuf> {
        // Extract next to the MPI file by default
        let default_output = mpi_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("ttw_mpi_extracted_{}", uuid_simple()));
        Self::extract_to(mpi_path, &default_output)
    }

    /// Extract .mpi file to a specific directory (parallel)
    ///
    /// Phase 1: Scan archive and collect all file entries (single-threaded, fast)
    /// Phase 2: Pre-create all directories (single-threaded, eliminates per-file create_dir_all)
    /// Phase 3: Decompress + write files in parallel across all CPU cores
    pub fn extract_to(mpi_path: &Path, output_dir: &Path) -> Result<PathBuf> {
        if !mpi_path.exists() {
            anyhow::bail!("MPI file not found: {}", mpi_path.display());
        }

        tracing::info!(
            "\nExtracting MPI package: {}",
            mpi_path.file_name().unwrap_or_default().to_string_lossy()
        );
        tracing::info!("Extracting to: {}", output_dir.display());
        tracing::info!("This may take a few minutes...\n");

        fs::create_dir_all(output_dir)?;
        let temp_dir = output_dir.to_path_buf();

        {
            let (archive, options) =
                Archive::read(mpi_path).context("Failed to open MPI archive")?;
            let compression_options: ba2::tes4::FileCompressionOptions = (&options).into();

            struct FileEntry<'a> {
                output_path: PathBuf,
                file: &'a ba2::tes4::File<'a>,
            }

            let mut entries: Vec<FileEntry> = Vec::new();
            for (dir_key, folder) in archive.iter() {
                let dir_name =
                    String::from_utf8_lossy(dir_key.name().as_bytes()).replace('\\', "/");

                for (file_key, file) in folder.iter() {
                    let file_name = String::from_utf8_lossy(file_key.name().as_bytes()).to_string();
                    let relative_path = if dir_name.is_empty() || dir_name == "." {
                        file_name
                    } else {
                        format!("{}/{}", dir_name, file_name)
                    };
                    let output_path = safe_join(&temp_dir, &relative_path)?;
                    entries.push(FileEntry { output_path, file });
                }
            }

            let mut dirs_needed: HashSet<PathBuf> = HashSet::new();
            for entry in &entries {
                if let Some(parent) = entry.output_path.parent() {
                    dirs_needed.insert(parent.to_path_buf());
                }
            }

            let total_files = entries.len();
            tracing::info!("Archive opened: {} files found", total_files);

            for dir in &dirs_needed {
                fs::create_dir_all(dir)?;
            }

            let pb = ProgressBar::new(total_files as u64);
            pb.set_style(archive_progress_style());

            let extracted = AtomicUsize::new(0);
            let failed = AtomicUsize::new(0);

            entries.par_iter().for_each(|entry| {
                let data = if entry.file.is_compressed() {
                    let compressed = entry.file.as_bytes();
                    if compressed.len() >= 4 && compressed[0..4] == LZ4_FRAME_MAGIC {
                        Self::decompress_lz4_frame(compressed)
                    } else {
                        entry
                            .file
                            .decompress(&compression_options)
                            .map(|d| d.as_bytes().to_vec())
                            .map_err(|e| anyhow::anyhow!("{}", e))
                    }
                } else {
                    Ok(entry.file.as_bytes().to_vec())
                };

                match data {
                    Ok(bytes) => {
                        if fs::write(&entry.output_path, &bytes).is_ok() {
                            extracted.fetch_add(1, Ordering::Relaxed);
                        } else {
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }

                pb.inc(1);
            });

            pb.finish_with_message("Extraction complete");

            let extracted = extracted.load(Ordering::Relaxed);
            let failed = failed.load(Ordering::Relaxed);

            tracing::info!("\nMPI extraction complete: {} files extracted", extracted);
            if failed > 0 {
                tracing::info!("{} files failed to extract", failed);
            }
        }

        Ok(temp_dir)
    }
    /// Decompress LZ4 frame format data
    pub(super) fn decompress_lz4_frame(compressed: &[u8]) -> Result<Vec<u8>> {
        use lz4_flex::frame::FrameDecoder;
        use std::io::Read;

        let mut decoder = FrameDecoder::new(compressed);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("LZ4 frame decompression failed")?;
        Ok(decompressed)
    }

    /// Clean up a temporary extraction directory
    pub fn cleanup_temp(temp_dir: &Path) -> Result<()> {
        // Safety check - only delete our temp directories
        let dir_name = temp_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Allow various temp directory patterns we use
        let is_safe = dir_name.starts_with("ttw_mpi_")
            || dir_name.starts_with(".ttw_mpi_")
            || dir_name == ".mpi_package"
            || dir_name.starts_with("mpi_extracted_");

        if !is_safe {
            anyhow::bail!(
                "Refusing to delete directory that doesn't match expected MPI temp pattern: {}",
                dir_name
            );
        }

        if temp_dir.exists() {
            tracing::info!("\nCleaning up temporary files...");
            fs::remove_dir_all(temp_dir)?;
        }

        Ok(())
    }
}

/// Generate a simple UUID-like string
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", duration.as_secs(), duration.subsec_nanos())
}
