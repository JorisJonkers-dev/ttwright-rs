use crate::app::InstallError;
use crate::models::{Asset, Location, TtwManifest, Variable};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use ttwright_contract::ErrorCode;

/// Loads and parses TTW installation manifest
pub struct ManifestLoader;

impl ManifestLoader {
    /// Load manifest from JSON file
    pub fn load_from_file(manifest_path: &Path) -> Result<TtwManifest> {
        if !manifest_path.exists() {
            return Err(InstallError::new(
                ErrorCode::ManifestMissing,
                format!("Manifest not found: {}", manifest_path.display()),
            )
            .into());
        }

        tracing::info!("Loading manifest from: {}", manifest_path.display());

        let json = fs::read(manifest_path).context("Failed to read manifest file")?;
        Self::load_from_bytes(&json)
    }

    /// Parse a manifest held in memory.
    ///
    /// # Errors
    /// `manifest_invalid` when the bytes are not a manifest.
    pub fn load_from_bytes(json: &[u8]) -> Result<TtwManifest> {
        // Manifests written on Windows may start with a byte order mark.
        let json = json.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(json);
        let manifest: TtwManifest = serde_json::from_slice(json).map_err(|e| {
            InstallError::new(
                ErrorCode::ManifestInvalid,
                format!("Failed to parse manifest JSON: {e}"),
            )
        })?;

        // Print summary
        if let Some(pkg) = &manifest.package {
            tracing::info!(
                "  Package: {} v{}",
                pkg.title.as_deref().unwrap_or("Unknown"),
                pkg.version.as_deref().unwrap_or("?")
            );
        }

        let var_count = manifest
            .variables
            .as_ref()
            .and_then(|v| v.first())
            .map(|v| v.len())
            .unwrap_or(0);

        let loc_count = manifest
            .locations
            .as_ref()
            .and_then(|l| l.first())
            .map(|l| l.len())
            .unwrap_or(0);

        let asset_count = manifest.assets.as_ref().map(|a| a.len()).unwrap_or(0);

        tracing::info!("  Variables: {} defined", var_count);
        tracing::info!("  Locations: {} defined", loc_count);
        tracing::info!("  Assets: {} operations", asset_count);

        Ok(manifest)
    }

    /// Parse assets from manifest
    pub fn parse_assets(manifest: &TtwManifest) -> Result<Vec<Asset>> {
        let asset_arrays = match &manifest.assets {
            Some(assets) if !assets.is_empty() => assets,
            _ => return Ok(Vec::new()),
        };

        tracing::info!("\nParsing {} assets...", asset_arrays.len());

        // Every asset must parse: one that does not would be skipped, and the install would be incomplete.
        let mut assets = Vec::with_capacity(asset_arrays.len());
        for (index, asset_value) in asset_arrays.iter().enumerate() {
            let array = asset_value.as_array().ok_or_else(|| {
                InstallError::new(
                    ErrorCode::ManifestInvalid,
                    format!("asset {index} is not a JSON array"),
                )
            })?;
            let mut asset = Asset::from_json_array(array).map_err(|e| {
                InstallError::new(ErrorCode::ManifestInvalid, format!("asset {index}: {e}"))
            })?;
            asset.index = index;
            assets.push(asset);
        }

        tracing::info!("Successfully parsed {} assets", assets.len());

        // Print operation type summary
        Self::print_operation_summary(&assets);

        Ok(assets)
    }

    fn print_operation_summary(assets: &[Asset]) {
        use std::collections::HashMap;

        let mut counts: HashMap<i32, usize> = HashMap::new();
        for asset in assets {
            *counts.entry(asset.op_type).or_insert(0) += 1;
        }

        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by_key(|(op_type, _)| *op_type);

        tracing::info!("\nOperation summary:");
        for (op_type, count) in sorted {
            let name = match op_type {
                0 => "Copy",
                1 => "New",
                2 => "Patch",
                4 => "OggEnc2",
                5 => "AudioEnc",
                _ => "Unknown",
            };
            tracing::info!("  OpType {} ({}): {} operations", op_type, name, count);
        }
    }

    /// Get locations for a specific profile
    pub fn get_locations(manifest: &TtwManifest, profile_index: usize) -> Result<Vec<Location>> {
        let locations = match &manifest.locations {
            Some(locs) if !locs.is_empty() => locs,
            _ => return Ok(Vec::new()),
        };

        if profile_index >= locations.len() {
            anyhow::bail!("Invalid profile index: {}", profile_index);
        }

        Ok(locations[profile_index].clone())
    }

    /// Get variables for a specific profile
    pub fn get_variables(manifest: &TtwManifest, profile_index: usize) -> Result<Vec<Variable>> {
        let variables = match &manifest.variables {
            Some(vars) if !vars.is_empty() => vars,
            _ => return Ok(Vec::new()),
        };

        if profile_index >= variables.len() {
            anyhow::bail!("Invalid profile index: {}", profile_index);
        }

        Ok(variables[profile_index].clone())
    }

    /// Select the best profile index for Linux installation.
    /// Some MPI files have Profile 0 with hardcoded Windows paths (e.g., C:\Users\...)
    /// while Profile 1 uses proper %DESTINATION% variables. On Linux we prefer the
    /// profile that uses variables, since we substitute them with CLI-provided paths.
    pub fn select_best_profile(manifest: &TtwManifest) -> usize {
        let locations = match &manifest.locations {
            Some(locs) if !locs.is_empty() => locs,
            _ => return 0,
        };

        // Check if Profile 0 has hardcoded Windows paths in directory locations
        if let Some(profile0) = locations.first() {
            let has_hardcoded_windows_path = profile0.iter().any(|loc| {
                if loc.loc_type != 0 {
                    return false; // Only check directory locations
                }
                let value = loc.value.as_deref().unwrap_or("");
                // Detect Windows absolute paths: C:\..., D:\..., etc.
                Self::is_windows_absolute_path(value)
            });

            if has_hardcoded_windows_path && locations.len() > 1 {
                // Check if another profile uses %DESTINATION% or other variables instead
                for (idx, profile) in locations.iter().enumerate().skip(1) {
                    let uses_variables = profile.iter().all(|loc| {
                        let value = loc.value.as_deref().unwrap_or("");
                        // Either uses variables, or is a BSA source path (Type 1) which is fine
                        loc.loc_type != 0 || value.contains('%') || value.is_empty()
                    });

                    if uses_variables {
                        tracing::info!(
                            "  Profile 0 has hardcoded Windows paths, using Profile {} instead",
                            idx
                        );
                        return idx;
                    }
                }
            }
        }

        0
    }

    /// Check if a path looks like a Windows absolute path
    fn is_windows_absolute_path(path: &str) -> bool {
        let path = path.trim();
        // Match patterns like C:\, D:\, C:/, D:/, etc.
        if path.len() >= 3 {
            let bytes = path.as_bytes();
            bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && (bytes[2] == b'\\' || bytes[2] == b'/')
        } else {
            false
        }
    }

    /// Get BSA target locations from the best available profile
    /// Some MPI files have BSA targets in Profile 1 (Windows) but not Profile 0 (Linux)
    /// This method searches all profiles for Type 2 BSA targets with proper flags
    pub fn get_bsa_target_locations(manifest: &TtwManifest) -> Result<Vec<Location>> {
        let locations = match &manifest.locations {
            Some(locs) if !locs.is_empty() => locs,
            _ => return Ok(Vec::new()),
        };

        // Search all profiles for BSA targets (Type 2 with .bsa in Value and flags set)
        for (profile_idx, profile) in locations.iter().enumerate() {
            let bsa_targets: Vec<Location> = profile
                .iter()
                .filter(|loc| {
                    let value = loc.value.as_deref().unwrap_or("");
                    // Type 2 = BSA creation target with proper flags
                    loc.loc_type == 2
                        && value.to_lowercase().ends_with(".bsa")
                        && loc.archive_flags.is_some()
                })
                .cloned()
                .collect();

            if !bsa_targets.is_empty() {
                tracing::info!(
                    "  Found {} BSA targets in profile {}",
                    bsa_targets.len(),
                    profile_idx
                );
                return Ok(bsa_targets);
            }
        }

        // Fallback: look for Type 0 with .bsa in Name (TTW 3.4 style)
        for (profile_idx, profile) in locations.iter().enumerate() {
            let bsa_targets: Vec<Location> = profile
                .iter()
                .filter(|loc| {
                    let name = loc.name.as_deref().unwrap_or("");
                    loc.loc_type == 0 && name.to_lowercase().ends_with(".bsa")
                })
                .cloned()
                .collect();

            if !bsa_targets.is_empty() {
                tracing::info!(
                    "  Found {} BSA targets (Type 0) in profile {}",
                    bsa_targets.len(),
                    profile_idx
                );
                return Ok(bsa_targets);
            }
        }

        Ok(Vec::new())
    }

    /// Get pre-installation checks from manifest
    pub fn get_checks(manifest: &TtwManifest) -> Vec<crate::models::Check> {
        manifest.checks.clone().unwrap_or_default()
    }
}
