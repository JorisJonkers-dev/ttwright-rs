use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use super::paths::find_file_case_insensitive;
use super::processor::AssetProcessor;
use super::stats::OpType;
use crate::models::Asset;
use crate::services::path_utils::safe_join;
use crate::services::{AudioFormat, AudioProcessor};
use crate::services::{Placed, redirected_file};
use ttwright_contract::{RedirectedFile, RedirectedStep};

impl AssetProcessor {
    pub(super) fn process_asset(&self, asset: &Asset) -> Result<()> {
        let op_type = OpType::from_i32(asset.op_type)
            .ok_or_else(|| anyhow::anyhow!("Unknown operation type: {}", asset.op_type))?;

        match op_type {
            OpType::Copy => self.process_copy(asset),
            OpType::New => self.process_new(asset),
            OpType::Patch => self.process_patch(asset),
            OpType::OggEnc2 => self.process_ogg_resample(asset),
            OpType::AudioEnc => self.process_audio_encode(asset),
        }
    }

    /// Copy operation: copy file from source to target
    fn process_copy(&self, asset: &Asset) -> Result<()> {
        let source_data = self.get_source_data(asset)?;

        if self.dry_run {
            return Ok(());
        }

        self.write_to_target(asset, &source_data)?;
        Ok(())
    }

    /// Read a file from the MPI package (in-memory store or disk fallback).
    fn read_mpi_file(&self, relative_path: &str) -> Result<Vec<u8>> {
        // Normalize: replace backslashes, strip leading ./ or /
        let normalized = relative_path.replace('\\', "/");
        let normalized = normalized.trim_start_matches("./").trim_start_matches('/');

        // Try in-memory store first (instant HashMap lookup)
        if let Some(ref store) = self.mpi_store
            && let Some(data) = store.get(normalized)
        {
            return Ok(data.to_vec());
        }

        // Disk-backed installs use a one-time lowercase index. MPI archive
        // names are lowercased during extraction, while the manifest retains
        // its original Windows casing.
        if let Some(ref index) = self.mpi_index
            && let Some(actual_path) = index.get(normalized)
        {
            return fs::read(actual_path)
                .with_context(|| format!("Failed to read: {}", actual_path.display()));
        }

        // Fallback for callers that do not provide an index.
        let source_path = safe_join(&self.mpi_dir, normalized)?;
        let actual_path = find_file_case_insensitive(&source_path).ok_or_else(|| {
            anyhow::anyhow!(
                "File not found in MPI: {} (normalized: {})",
                source_path.display(),
                normalized
            )
        })?;

        fs::read(&actual_path).with_context(|| format!("Failed to read: {}", actual_path.display()))
    }

    /// New operation: copy new file from MPI package
    fn process_new(&self, asset: &Asset) -> Result<()> {
        let source_data = self.read_mpi_file(&asset.source_path)?;

        if self.dry_run {
            return Ok(());
        }

        self.write_to_target(asset, &source_data)?;
        Ok(())
    }

    /// Patch operation: apply xdelta patch to source
    fn process_patch(&self, asset: &Asset) -> Result<()> {
        let source_data = self.get_source_data(asset)?;

        // Patch file is named based on TARGET path + ".xd3", not source path!
        let target_file = asset.target_path.as_deref().unwrap_or(&asset.source_path);
        let patch_file_name = format!("{}.xd3", target_file);

        let patch_data = self.read_mpi_file(&patch_file_name)?;

        if self.dry_run {
            return Ok(());
        }

        let patched = self
            .xdelta
            .apply_patch_from_bytes(&source_data, &patch_data)?;
        self.write_to_target(asset, &patched)?;
        Ok(())
    }

    /// OggEnc2 operation: decode OGG, resample, re-encode
    fn process_ogg_resample(&self, asset: &Asset) -> Result<()> {
        let source_data = self.get_source_data(asset)?;

        if self.dry_run {
            return Ok(());
        }

        // Create audio processor with manifest params (e.g. "-f:24000 -q:5")
        let audio_processor = AudioProcessor::new().with_params(&asset.params)?;
        let processed = audio_processor.process_ogg_resample(&source_data)?;
        self.write_to_target(asset, &processed)?;
        Ok(())
    }

    /// AudioEnc operation: convert audio format
    fn process_audio_encode(&self, asset: &Asset) -> Result<()> {
        let source_data = self.get_source_data(asset)?;

        // Parse output format from params or target path
        let output_format = self.get_audio_output_format(asset)?;

        // Determine input format from source path
        let input_format = Path::new(&asset.source_path)
            .extension()
            .and_then(|e| e.to_str());

        if self.dry_run {
            return Ok(());
        }

        // Create audio processor with manifest params (e.g. "-f:24000 -q:5")
        let audio_processor = AudioProcessor::new().with_params(&asset.params)?;
        let processed =
            audio_processor.process_audio_conversion(&source_data, input_format, output_format)?;
        self.write_to_target(asset, &processed)?;
        Ok(())
    }

    /// Get source data, either from SQLite cache, BSA, or directory
    fn get_source_data(&self, asset: &Asset) -> Result<Vec<u8>> {
        if self.resolver.is_bsa_location(asset.source_loc) {
            let bsa_path = self.resolver.get_bsa_path(asset.source_loc)?;

            // First, check the SQLite cache
            if let Some(data) = self.bsa_cache.get(&bsa_path, &asset.source_path)? {
                return Ok(data);
            }

            // Not in cache - extract directly (fallback, slower path)
            let mut handler = self
                .bsa_handler
                .lock()
                .map_err(|_| anyhow::anyhow!("BSA handler lock poisoned"))?;
            handler.extract_file(&bsa_path, &asset.source_path)
        } else {
            // Read from directory
            let source_dir = self.resolver.resolve_path(asset.source_loc)?;
            let normalized_path = asset.source_path.replace('\\', "/");
            let source_path = safe_join(&source_dir, &normalized_path)?;

            // Try case-insensitive lookup for Linux
            let actual_path = find_file_case_insensitive(&source_path).ok_or_else(|| {
                anyhow::anyhow!("Source file not found: {}", source_path.display())
            })?;

            fs::read(&actual_path)
                .with_context(|| format!("Failed to read: {}", actual_path.display()))
        }
    }

    /// Where an asset's loose file goes: the location's folder, or `_ttwright_game_root/` in the destination when
    /// the location is outside the destination.
    fn place_target(&self, asset: &Asset) -> Result<Placed> {
        let target_file = asset.target_path.as_deref().unwrap_or(&asset.source_path);
        // Normalize path separators for Linux
        let normalized = target_file.replace('\\', "/");
        self.resolver.place(asset.target_loc, &normalized)
    }

    /// The assets whose loose file is meant for a place outside the destination, with where each one goes
    /// instead. Nothing is written. Archive targets are not listed: archives are always written into the
    /// destination.
    pub fn redirected_assets(&self, assets: &[Asset]) -> Vec<RedirectedFile> {
        let archives = self.archive_locations();
        let mut out = Vec::new();
        for asset in assets {
            if archives.contains(&asset.target_loc)
                || !self
                    .resolver
                    .get_location(asset.target_loc)
                    .is_ok_and(|l| l.is_directory())
            {
                continue;
            }
            // An invalid path fails when the asset is processed; it is not a redirect.
            let Ok(placed) = self.place_target(asset) else {
                continue;
            };
            if let Some((location, meant)) = placed.redirected_from {
                out.push(redirected_file(
                    RedirectedStep::Asset,
                    location,
                    &meant,
                    &placed.path,
                    self.resolver.containment().root(),
                ));
            }
        }
        out
    }

    /// Write data to target location
    fn write_to_target(&self, asset: &Asset, data: &[u8]) -> Result<()> {
        let writer = self
            .bsa_writer
            .lock()
            .map_err(|_| anyhow::anyhow!("BSA writer lock poisoned"))?;
        if writer.is_bsa_location(asset.target_loc) {
            let target_file = asset.target_path.as_deref().unwrap_or(&asset.source_path);
            let normalized = target_file.replace('\\', "/");
            writer.add_file(asset.target_loc, &normalized, data.to_vec())?;
        } else {
            drop(writer);
            let target_path = self.place_target(asset)?.path;
            // The last line of defence: whatever the manifest said and however the path was built, nothing is
            // written to a place that resolves outside the destination.
            self.resolver.containment().check(&target_path)?;

            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            // The folders just made may have been links; check the final path again before the write.
            self.resolver.containment().check(&target_path)?;
            fs::write(&target_path, data)?;
        }

        Ok(())
    }

    /// Finalize installation by writing all BSA archives
    pub fn finalize_bsas(&self) -> Result<(usize, usize)> {
        if self.dry_run {
            tracing::info!(
                "\n[DRY RUN] Would write BSA archives to: {}",
                self.dest_dir.display()
            );
            return Ok((0, 0));
        }

        let mut writer = self
            .bsa_writer
            .lock()
            .map_err(|_| anyhow::anyhow!("BSA writer lock poisoned"))?;
        writer.write_all(&self.dest_dir)
    }

    /// Finalize BSAs with progress callback for GUI
    /// callback(current, total, bsa_name)
    pub fn finalize_bsas_with_callback<F>(&self, callback: F) -> Result<(usize, usize)>
    where
        F: Fn(usize, usize, &str) + Sync,
    {
        if self.dry_run {
            return Ok((0, 0));
        }

        let mut writer = self
            .bsa_writer
            .lock()
            .map_err(|_| anyhow::anyhow!("BSA writer lock poisoned"))?;
        writer.write_all_with_callback(&self.dest_dir, callback)
    }

    /// Get audio output format from asset params or target path
    fn get_audio_output_format(&self, asset: &Asset) -> Result<AudioFormat> {
        // Try to get from target path extension
        let target_file = asset.target_path.as_deref().unwrap_or(&asset.source_path);

        if let Some(ext) = Path::new(target_file).extension().and_then(|e| e.to_str())
            && let Some(format) = AudioFormat::from_extension(ext)
        {
            return Ok(format);
        }

        // Default to OGG
        Ok(AudioFormat::Ogg)
    }

    /// Clear BSA cache to free memory
    pub fn clear_cache(&self) {
        if let Ok(mut handler) = self.bsa_handler.lock() {
            handler.clear_cache();
        }
    }

    pub(super) fn process_asset_with_data(&self, asset: &Asset, source_data: &[u8]) -> Result<()> {
        let op_type = OpType::from_i32(asset.op_type)
            .ok_or_else(|| anyhow::anyhow!("Unknown operation type: {}", asset.op_type))?;

        match op_type {
            OpType::Copy => {
                if !self.dry_run {
                    self.write_to_target(asset, source_data)?;
                }
                Ok(())
            }
            OpType::New => self.process_new(asset),
            OpType::Patch => {
                let target_file = asset.target_path.as_deref().unwrap_or(&asset.source_path);
                let patch_file_name = format!("{}.xd3", target_file);
                let patch_data = self.read_mpi_file(&patch_file_name)?;

                if !self.dry_run {
                    let patched = self
                        .xdelta
                        .apply_patch_from_bytes(source_data, &patch_data)?;
                    self.write_to_target(asset, &patched)?;
                }
                Ok(())
            }
            OpType::OggEnc2 => {
                if !self.dry_run {
                    let audio_processor = AudioProcessor::new().with_params(&asset.params)?;
                    let processed = audio_processor.process_ogg_resample(source_data)?;
                    self.write_to_target(asset, &processed)?;
                }
                Ok(())
            }
            OpType::AudioEnc => {
                let output_format = self.get_audio_output_format(asset)?;
                let input_format = Path::new(&asset.source_path)
                    .extension()
                    .and_then(|e| e.to_str());

                if !self.dry_run {
                    let audio_processor = AudioProcessor::new().with_params(&asset.params)?;
                    let processed = audio_processor.process_audio_conversion(
                        source_data,
                        input_format,
                        output_format,
                    )?;
                    self.write_to_target(asset, &processed)?;
                }
                Ok(())
            }
        }
    }
}
