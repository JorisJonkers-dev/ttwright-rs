// SPDX-License-Identifier: GPL-3.0-only
//! The commands for people: extract, inspect, verify, detect and logs (upstream's, kept as they were).

use anyhow::{Result, bail};
use std::path::Path;
use ttwright_contract::{VersionSupport, ttw_version_support};
use ttwright_core::{
    app::{check_manifest, find_manifest},
    services::{DetectedGame, GameDetection, Logger, ManifestLoader, MpiExtractor},
};

pub(crate) fn run_extract(mpi_path: &Path, output: &Path) -> Result<()> {
    println!("=== TTW MPI Extractor ===\n");

    if !MpiExtractor::is_mpi_file(mpi_path) {
        bail!("Not an MPI file: {}", mpi_path.display());
    }

    // Extract to output directory
    let extracted = MpiExtractor::extract_to_temp(mpi_path)?;

    // Move to desired location
    if output.exists() {
        bail!("Output directory already exists: {}", output.display());
    }

    if std::fs::rename(&extracted, output).is_err() {
        // Cross-device move, need to copy
        copy_dir_recursive(&extracted, output)?;
        std::fs::remove_dir_all(&extracted)?;
    }

    println!("\nExtracted to: {}", output.display());
    Ok(())
}

pub(crate) fn run_inspect(mpi_path: &Path, verbose: bool) -> Result<()> {
    println!("=== TTW MPI Inspector ===\n");

    // Handle MPI extraction if needed
    let (mpi_dir, cleanup_needed) = if MpiExtractor::is_mpi_file(mpi_path) {
        let extracted = MpiExtractor::extract_to_temp(mpi_path)?;
        (extracted, true)
    } else if mpi_path.is_dir() {
        (mpi_path.to_path_buf(), false)
    } else {
        bail!("Invalid MPI path: {}", mpi_path.display());
    };

    // Find and load manifest
    let manifest_path = find_manifest(&mpi_dir)?;
    let manifest = ManifestLoader::load_from_file(&manifest_path)?;

    // Print package info
    if let Some(pkg) = &manifest.package {
        println!("\nPackage Information:");
        println!("  Title: {}", pkg.title.as_deref().unwrap_or("Unknown"));
        println!("  Version: {}", pkg.version.as_deref().unwrap_or("?"));
        println!("  Author: {}", pkg.author.as_deref().unwrap_or("Unknown"));
    }

    // Parse and summarize assets
    let assets = ManifestLoader::parse_assets(&manifest)?;
    println!("  Assets: {}", assets.len());
    let profile = ManifestLoader::select_best_profile(&manifest);
    match check_manifest(&manifest, &assets, profile) {
        Ok(needs) => println!(
            "  Features: all supported (needs Fallout 3: {}, Fallout New Vegas: {}, Oblivion: {})",
            needs.fallout3, needs.falloutnv, needs.oblivion
        ),
        Err(e) => println!(
            "  Features: NOT supported, {} ({})",
            e.feature.as_deref().unwrap_or("?"),
            e.message
        ),
    }
    if let Some(pkg) = &manifest.package {
        let title = pkg.title.as_deref().unwrap_or_default();
        let version = pkg.version.as_deref().unwrap_or_default();
        match ttw_version_support(title, version) {
            VersionSupport::Supported => println!("  Version: supported"),
            VersionSupport::NotTtw => {}
            VersionSupport::Unsupported(v) => {
                println!("  Version: TTW {v} is NOT a supported version")
            }
        }
    }

    if verbose {
        println!("\nAssets (first 20):");
        for (i, asset) in assets.iter().take(20).enumerate() {
            println!(
                "  [{}] OpType:{} {} -> loc:{}",
                i, asset.op_type, asset.source_path, asset.target_loc
            );
        }
        if assets.len() > 20 {
            println!("  ... and {} more assets", assets.len() - 20);
        }
    }

    // Cleanup
    if cleanup_needed {
        MpiExtractor::cleanup_temp(&mpi_dir)?;
    }

    Ok(())
}

pub(crate) fn run_verify(
    fo3: Option<&Path>,
    fnv: Option<&Path>,
    oblivion: Option<&Path>,
) -> Result<()> {
    println!("=== Game Installation Verifier ===\n");

    let mut all_valid = true;
    let detection = if fo3.is_none() || fnv.is_none() || oblivion.is_none() {
        Some(GameDetection::detect())
    } else {
        None
    };

    if let Some(fo3_path) = fo3.or_else(|| {
        detection
            .as_ref()
            .and_then(|detected| detected_path(detected.fallout3.as_ref()))
    }) {
        println!("Checking Fallout 3: {}", fo3_path.display());
        if verify_fo3_install(fo3_path) {
            println!("  [OK] Valid Fallout 3 installation");
        } else {
            println!("  [FAIL] Invalid or incomplete Fallout 3 installation");
            all_valid = false;
        }
    }

    if let Some(fnv_path) = fnv.or_else(|| {
        detection
            .as_ref()
            .and_then(|detected| detected_path(detected.falloutnv.as_ref()))
    }) {
        println!("Checking Fallout New Vegas: {}", fnv_path.display());
        if verify_fnv_install(fnv_path) {
            println!("  [OK] Valid Fallout New Vegas installation");
        } else {
            println!("  [FAIL] Invalid or incomplete Fallout New Vegas installation");
            all_valid = false;
        }
    }

    if let Some(oblivion_path) = oblivion.or_else(|| {
        detection
            .as_ref()
            .and_then(|detected| detected_path(detected.oblivion.as_ref()))
    }) {
        println!("Checking Oblivion: {}", oblivion_path.display());
        if verify_oblivion_install(oblivion_path) {
            println!("  [OK] Valid Oblivion installation");
        } else {
            println!("  [FAIL] Invalid or incomplete Oblivion installation");
            all_valid = false;
        }
    }

    if all_valid {
        println!("\nAll specified installations are valid!");
        Ok(())
    } else {
        bail!("Some installations are invalid")
    }
}

fn detected_path(game: Option<&DetectedGame>) -> Option<&Path> {
    game.map(|game| game.path.as_path())
}

pub(crate) fn run_detect() -> Result<()> {
    println!("=== Game Auto-Detection ===\n");
    let detected = GameDetection::detect();

    print_detected_game("Fallout 3", detected.fallout3.as_ref());
    print_detected_game("Fallout New Vegas", detected.falloutnv.as_ref());
    print_detected_game("Oblivion", detected.oblivion.as_ref());

    if detected.detected_count() == 0 {
        println!("\nNo supported games detected.");
        println!("Manual install paths can still be provided with --fo3, --fnv, and --oblivion.");
    }

    Ok(())
}

fn print_detected_game(label: &str, game: Option<&DetectedGame>) {
    match game {
        Some(game) => println!(
            "  [OK] {}: {} ({})",
            label,
            game.path.display(),
            game.source
        ),
        None => println!("  [--] {}: not found", label),
    }
}

fn verify_fo3_install(path: &Path) -> bool {
    // Check for essential Fallout 3 files
    let required = [
        "Data/Fallout3.esm",
        "Data/Fallout - Meshes.bsa",
        "Data/Fallout - Textures.bsa",
        "Data/Fallout - Voices.bsa",
    ];

    for file in required {
        let file_path = path.join(file);
        if !file_path.exists() {
            // Try lowercase
            let lower_path = path.join(file.to_lowercase());
            if !lower_path.exists() {
                return false;
            }
        }
    }

    true
}

fn verify_fnv_install(path: &Path) -> bool {
    // Check for essential FNV files
    let required = [
        "Data/FalloutNV.esm",
        "Data/Fallout - Meshes.bsa",
        "Data/Fallout - Textures.bsa",
        "Data/Fallout - Voices1.bsa",
    ];

    for file in required {
        let file_path = path.join(file);
        if !file_path.exists() {
            // Try lowercase
            let lower_path = path.join(file.to_lowercase());
            if !lower_path.exists() {
                return false;
            }
        }
    }

    true
}

fn verify_oblivion_install(path: &Path) -> bool {
    // Check for essential Oblivion files
    let required = [
        "Data/Oblivion.esm",
        "Data/Oblivion - Meshes.bsa",
        "Data/Oblivion - Textures - Compressed.bsa",
    ];

    for file in required {
        let file_path = path.join(file);
        if !file_path.exists() {
            // Try lowercase
            let lower_path = path.join(file.to_lowercase());
            if !lower_path.exists() {
                return false;
            }
        }
    }

    true
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }

    Ok(())
}

pub(crate) fn run_logs(count: usize) -> Result<()> {
    println!("=== Recent Installation Logs ===\n");

    let logs = Logger::list_recent_logs(count)?;

    if logs.is_empty() {
        println!("No log files found.");
        println!("\nLogs are stored in: ~/.local/share/mpi_installer/logs/");
        return Ok(());
    }

    for (i, log_path) in logs.iter().enumerate() {
        let filename = log_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "?".to_string());

        // Get file size
        let size = std::fs::metadata(log_path)
            .map(|m| format_size(m.len()))
            .unwrap_or_else(|_| "?".to_string());

        println!("  [{}] {} ({})", i + 1, filename, size);
    }

    println!("\nTo view a log: cat ~/.local/share/mpi_installer/logs/<filename>");
    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;

    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
