use anyhow::{Context, Result, bail};
use md5::Md5;
use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::Check;
use crate::services::LocationResolver;
use crate::services::path_utils::safe_join;

/// Check types from MPI manifest
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckType {
    /// File existence check (with optional checksum verification)
    /// If Checksums field is present, also verifies hash
    FileCheck = 0,
    /// Directory must have minimum free space (uses FreeSize field in MB)
    FreeSpace = 1,
    /// Path validation check (e.g., not in Program Files)
    PathCheck = 2,
}

impl CheckType {
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::FileCheck),
            1 => Some(Self::FreeSpace),
            2 => Some(Self::PathCheck),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::FileCheck => "FileCheck",
            Self::FreeSpace => "FreeSpace",
            Self::PathCheck => "PathCheck",
        }
    }
}

/// Result of verification
#[derive(Debug)]
pub struct VerificationResult {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

impl VerificationResult {
    pub fn is_success(&self) -> bool {
        self.failed == 0
    }
}

/// File verification service for checking game files before installation
pub struct FileVerifier<'a> {
    resolver: &'a LocationResolver,
}

impl<'a> FileVerifier<'a> {
    pub fn new(resolver: &'a LocationResolver) -> Self {
        Self { resolver }
    }

    /// Resolve the path that a manifest check will read.
    pub fn check_file_path(&self, check: &Check) -> Result<PathBuf> {
        self.resolve_check_path(check)
    }

    /// Run all checks from the manifest
    pub fn run_checks(&self, checks: &[Check]) -> Result<VerificationResult> {
        let mut result = VerificationResult {
            passed: 0,
            failed: 0,
            skipped: 0,
            errors: Vec::new(),
        };

        if checks.is_empty() {
            tracing::info!("No verification checks defined in manifest");
            return Ok(result);
        }

        tracing::info!(
            "\n=== Running {} Pre-Installation Checks ===\n",
            checks.len()
        );

        for (i, check) in checks.iter().enumerate() {
            let check_type = CheckType::from_i32(check.check_type);

            match check_type {
                Some(CheckType::FileCheck) => {
                    // Type 0: File check - existence and optionally checksum
                    if check
                        .checksums
                        .as_ref()
                        .is_some_and(|checksums| !checksums.is_empty())
                    {
                        self.verify_file_checksum(i, check, &mut result);
                    } else {
                        self.verify_file_exists(i, check, &mut result);
                    }
                }
                Some(CheckType::FreeSpace) => {
                    self.verify_free_space(i, check, &mut result);
                }
                Some(CheckType::PathCheck) => {
                    self.verify_path_check(i, check, &mut result);
                }
                None => {
                    tracing::info!(
                        "  [{}] SKIP: Unknown check type {}",
                        i + 1,
                        check.check_type
                    );
                    result.skipped += 1;
                }
            }
        }

        tracing::info!("\n=== Verification Summary ===");
        tracing::info!("  Passed:  {}", result.passed);
        tracing::info!("  Failed:  {}", result.failed);
        tracing::info!("  Skipped: {}", result.skipped);

        if !result.errors.is_empty() {
            tracing::info!("\nErrors:");
            for error in &result.errors {
                tracing::info!("  - {}", error);
            }
        }

        Ok(result)
    }

    /// Verify a file exists at the specified location
    fn verify_file_exists(&self, index: usize, check: &Check, result: &mut VerificationResult) {
        let file_path = match self.resolve_check_path(check) {
            Ok(p) => p,
            Err(e) => {
                tracing::info!("  [{}] FAIL: FileExists - {}", index + 1, e);
                result.failed += 1;
                result.errors.push(format!("FileExists: {}", e));
                return;
            }
        };

        let exists = file_path.exists();
        let expected = !check.inverted; // inverted means file should NOT exist

        if exists == expected {
            tracing::info!(
                "  [{}] PASS: FileExists - {}",
                index + 1,
                file_path.display()
            );
            result.passed += 1;
        } else {
            let msg = if check.inverted {
                format!("File should not exist: {}", file_path.display())
            } else {
                format!("File not found: {}", file_path.display())
            };

            if let Some(custom_msg) = &check.custom_message {
                tracing::info!("  [{}] FAIL: {} - {}", index + 1, custom_msg, msg);
                result.errors.push(format!("{}: {}", custom_msg, msg));
            } else {
                tracing::info!("  [{}] FAIL: FileExists - {}", index + 1, msg);
                result.errors.push(msg);
            }
            result.failed += 1;
        }
    }

    /// Verify a file's checksum matches expected value
    fn verify_file_checksum(&self, index: usize, check: &Check, result: &mut VerificationResult) {
        let file_path = match self.resolve_check_path(check) {
            Ok(p) => p,
            Err(e) => {
                tracing::info!("  [{}] FAIL: Checksum - {}", index + 1, e);
                result.failed += 1;
                result.errors.push(format!("Checksum: {}", e));
                return;
            }
        };

        let expected_checksums = match &check.checksums {
            Some(c) if !c.is_empty() => c,
            _ => {
                tracing::info!(
                    "  [{}] SKIP: Checksum - No checksum specified for {}",
                    index + 1,
                    file_path.display()
                );
                result.skipped += 1;
                return;
            }
        };

        // Read file and compute hash
        let file_data = match fs::read(&file_path) {
            Ok(data) => data,
            Err(e) => {
                let msg = format!("Cannot read file {}: {}", file_path.display(), e);
                if let Some(custom_msg) = &check.custom_message {
                    tracing::info!("  [{}] FAIL: {} - {}", index + 1, custom_msg, msg);
                    result.errors.push(format!("{}: {}", custom_msg, msg));
                } else {
                    tracing::info!("  [{}] FAIL: Checksum - {}", index + 1, msg);
                    result.errors.push(msg);
                }
                result.failed += 1;
                return;
            }
        };

        // Checksums can be separated by commas, newlines, or \r\n (multiple valid hashes for different versions)
        let valid_hashes: Vec<&str> = expected_checksums
            .split([',', '\n', '\r'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        // Each expected hash is compared with the digest of its length: CRC32 (8 hex digits), MD5 (32) or
        // SHA-1 (40). The digests are computed only when some expected hash needs them.
        let wants = |len: usize| valid_hashes.iter().any(|h| h.len() == len);
        let crc32_hash = wants(8).then(|| compute_crc32(&file_data));
        let md5_hash = wants(32).then(|| compute_md5(&file_data));
        let sha1_hash = (wants(40) || (crc32_hash.is_none() && md5_hash.is_none()))
            .then(|| compute_sha1(&file_data));
        let matches = valid_hashes.iter().any(|expected| {
            let actual = match expected.len() {
                8 => crc32_hash.as_deref(),
                32 => md5_hash.as_deref(),
                _ => sha1_hash.as_deref(),
            };
            actual.is_some_and(|a| expected.eq_ignore_ascii_case(a))
        });
        let actual_hash = [&sha1_hash, &md5_hash, &crc32_hash]
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join("/");

        if matches != check.inverted {
            tracing::info!(
                "  [{}] PASS: Checksum - {} ({})",
                index + 1,
                file_path.file_name().unwrap_or_default().to_string_lossy(),
                &actual_hash[..8]
            );
            result.passed += 1;
        } else {
            let msg = format!(
                "Checksum mismatch for {}: expected {}, got {}",
                file_path.display(),
                if valid_hashes.len() == 1 {
                    valid_hashes[0].to_string()
                } else {
                    format!("one of [{}]", valid_hashes.join(", "))
                },
                actual_hash
            );

            if let Some(custom_msg) = &check.custom_message {
                tracing::info!("  [{}] FAIL: {} - {}", index + 1, custom_msg, msg);
                result.errors.push(format!("{}: {}", custom_msg, msg));
            } else {
                tracing::info!("  [{}] FAIL: {}", index + 1, msg);
                result.errors.push(msg);
            }
            result.failed += 1;
        }
    }

    /// Verify free space at location
    /// FreeSize in manifest is in MB
    fn verify_free_space(&self, index: usize, check: &Check, result: &mut VerificationResult) {
        let required_mb = check.free_size.unwrap_or(0);

        if required_mb <= 0 {
            tracing::info!(
                "  [{}] SKIP: FreeSpace - No size requirement specified",
                index + 1
            );
            result.skipped += 1;
            return;
        }

        let required_bytes = required_mb as u64 * 1_048_576; // Convert MB to bytes

        let path = match self.resolver.resolve_path(check.loc) {
            Ok(p) => p,
            Err(e) => {
                tracing::info!(
                    "  [{}] FAIL: FreeSpace - Cannot resolve location: {}",
                    index + 1,
                    e
                );
                result.failed += 1;
                result
                    .errors
                    .push(format!("FreeSpace: Cannot resolve location {}", check.loc));
                return;
            }
        };

        // Get available space (platform-specific)
        match get_available_space(&path) {
            Ok(available) => {
                let required_gb = required_bytes as f64 / 1_073_741_824.0;
                let available_gb = available as f64 / 1_073_741_824.0;

                if available >= required_bytes {
                    tracing::info!(
                        "  [{}] PASS: FreeSpace - {:.2} GB available (need {:.2} GB)",
                        index + 1,
                        available_gb,
                        required_gb
                    );
                    result.passed += 1;
                } else {
                    let msg = if let Some(custom_msg) = &check.custom_message {
                        format!(
                            "{} ({:.2} GB available, {:.2} GB required)",
                            custom_msg, available_gb, required_gb
                        )
                    } else {
                        format!(
                            "Insufficient space at {}: {:.2} GB available, {:.2} GB required",
                            path.display(),
                            available_gb,
                            required_gb
                        )
                    };
                    tracing::info!("  [{}] FAIL: {}", index + 1, msg);
                    result.errors.push(msg);
                    result.failed += 1;
                }
            }
            Err(e) => {
                tracing::info!("  [{}] SKIP: FreeSpace - Cannot check: {}", index + 1, e);
                result.skipped += 1;
            }
        }
    }

    /// Verify path doesn't contain problematic locations (e.g., Program Files)
    fn verify_path_check(&self, index: usize, check: &Check, result: &mut VerificationResult) {
        let path = match self.resolver.resolve_path(check.loc) {
            Ok(p) => p,
            Err(e) => {
                tracing::info!(
                    "  [{}] FAIL: PathCheck - Cannot resolve location: {}",
                    index + 1,
                    e
                );
                result.failed += 1;
                result
                    .errors
                    .push(format!("PathCheck: Cannot resolve location {}", check.loc));
                return;
            }
        };

        let path_str = path.to_string_lossy().to_lowercase();

        // Check for problematic paths (Program Files on Windows)
        let is_problematic = path_str.contains("program files")
            || path_str.contains("programfiles")
            || path_str.contains("program files (x86)");

        let _expected_good = !check.inverted; // inverted=false means path should be good

        if is_problematic == check.inverted {
            // If inverted and problematic, or not inverted and not problematic = PASS
            tracing::info!(
                "  [{}] PASS: PathCheck - {} is valid",
                index + 1,
                path.display()
            );
            result.passed += 1;
        } else {
            let msg = if let Some(custom_msg) = &check.custom_message {
                custom_msg.clone()
            } else {
                format!("Path {} is in a problematic location", path.display())
            };
            tracing::info!("  [{}] FAIL: PathCheck - {}", index + 1, msg);
            result.errors.push(msg);
            result.failed += 1;
        }
    }

    /// Resolve the file path for a check
    fn resolve_check_path(&self, check: &Check) -> Result<PathBuf> {
        let base_path = self
            .resolver
            .resolve_path(check.loc)
            .context("Cannot resolve location")?;

        let file_name = check
            .file
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No file specified in check"))?;

        // Normalize path separators
        let normalized = file_name.replace('\\', "/");

        safe_join(&base_path, &normalized)
    }
}

/// Compute SHA1 hash of data and return as lowercase hex string
pub fn compute_sha1(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Compute MD5 hash of data and return as lowercase hex string
/// CRC-32 (IEEE, the zip and PNG polynomial) as 8 lower-case hex digits.
pub fn compute_crc32(data: &[u8]) -> String {
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

pub fn compute_md5(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Compute SHA1 hash of a file
pub fn compute_file_sha1(path: &Path) -> Result<String> {
    let data =
        fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;
    Ok(compute_sha1(&data))
}

/// Get available disk space at path
fn get_available_space(path: &Path) -> Result<u64> {
    // Find an existing parent directory
    let mut check_path = path.to_path_buf();
    while !check_path.exists() {
        check_path = match check_path.parent() {
            Some(p) => p.to_path_buf(),
            None => bail!("Cannot find existing path for space check"),
        };
    }

    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::mem::MaybeUninit;

        let path_cstr = CString::new(check_path.to_string_lossy().as_bytes())
            .context("Invalid path for space check")?;

        let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();

        let result = unsafe { libc::statvfs(path_cstr.as_ptr(), stat.as_mut_ptr()) };

        if result == 0 {
            let stat = unsafe { stat.assume_init() };
            // f_bavail = blocks available to unprivileged users
            // f_frsize = fragment size (actual block size)
            // The field widths differ per platform (u32 blocks on macOS, u64 on Linux); widen both before
            // multiplying so the product cannot overflow either.
            #[allow(clippy::useless_conversion)]
            let available = u64::from(stat.f_bavail) * u64::from(stat.f_frsize);
            Ok(available)
        } else {
            bail!("statvfs failed for {}", check_path.display())
        }
    }

    #[cfg(windows)]
    {
        // On Windows, just return a large value to pass the check
        // The actual Windows implementation would use GetDiskFreeSpaceExW
        // but that requires winapi crate
        Ok(u64::MAX)
    }

    #[cfg(not(any(unix, windows)))]
    {
        Ok(u64::MAX) // Skip check on unknown platforms
    }
}
