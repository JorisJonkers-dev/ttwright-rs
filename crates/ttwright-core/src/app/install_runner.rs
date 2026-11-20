// SPDX-License-Identifier: GPL-3.0-only
use anyhow::{Context, Result};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ttwright_contract::{Counts, ErrorCode, Event, Phase, VersionSupport, ttw_version_support};

use crate::app::{
    CancelToken, GameNeeds, InstallError, InstallEvent, Journal, check_manifest, classify,
};
use crate::models::{Check, InstallConfig};
use crate::services::{
    AssetProcessor, FileVerifier, GameDetection, LocationResolver, ManifestLoader, MpiExtractor,
    MpiPathIndex, MpiStore, XdeltaManager, apply_file_attrs, compute_md5, compute_sha1,
    files_written_by_journal,
};

/// What to install, from where, into where.
#[derive(Debug, Clone, Default)]
pub struct InstallRequest {
    /// The `.mpi` file, or a folder it was extracted to.
    pub mpi_path: PathBuf,
    /// The Fallout 3 copy.
    pub fallout3_path: Option<PathBuf>,
    /// The Fallout: New Vegas copy.
    pub falloutnv_path: Option<PathBuf>,
    /// The Oblivion copy.
    pub oblivion_path: Option<PathBuf>,
    /// The destination folder.
    pub destination_path: PathBuf,
    /// Read and check everything, write nothing.
    pub dry_run: bool,
    /// Look for game copies that were not given. Off when the Toolbox drives ttwright: it passes the copies it
    /// detected, and ttwright never scans on its own (the Component rule).
    pub detect_missing: bool,
    /// Continue an interrupted install from the journal in the destination.
    pub resume: bool,
    /// Install a TTW package of a version this build does not list as supported.
    pub allow_unsupported_version: bool,
    /// Set to stop between assets.
    pub cancel: CancelToken,
}

/// What an install did, also when it stopped early.
#[derive(Debug, Clone, Default)]
pub struct InstallReport {
    /// The manifest's title, once read.
    pub package_name: Option<String>,
    /// The manifest's version, once read.
    pub package_version: Option<String>,
    /// Whether the package and version are listed as supported.
    pub package_supported: bool,
    /// Every pre-installation check passed.
    pub checks_passed: bool,
    /// Asset operations.
    pub assets: Counts,
    /// Archives written.
    pub archives: Counts,
    /// Post-installation commands and file times.
    pub post_install: Counts,
    /// An earlier run's journal was continued.
    pub resumed: bool,
    /// Detail lines on failures, at most twenty.
    pub messages: Vec<String>,
    /// Wall time.
    pub elapsed: Duration,
}

impl InstallReport {
    fn note(&mut self, message: impl Into<String>) {
        if self.messages.len() < 20 {
            self.messages.push(message.into());
        }
    }
}

/// The report, and the error when the install did not succeed.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    /// What was done.
    pub report: InstallReport,
    /// Why it stopped, or `None` for a successful install.
    pub error: Option<InstallError>,
}

/// Install the package. Never panics on bad input: every failure comes back as an [`InstallError`] with a stable
/// code, together with what was done before it.
pub fn run_install<F>(request: InstallRequest, emit: F) -> InstallOutcome
where
    F: Fn(InstallEvent) + Sync,
{
    let start = Instant::now();
    let mut report = InstallReport::default();
    let error = run(&request, &emit, &mut report)
        .err()
        .map(|e| classify(&e));
    report.elapsed = start.elapsed();
    InstallOutcome { report, error }
}

fn mpi_invalid(error: anyhow::Error) -> anyhow::Error {
    InstallError::new(
        ErrorCode::MpiInvalid,
        format!("The MPI package cannot be read: {error:#}"),
    )
    .into()
}

fn digest(parts: &[&[u8]]) -> String {
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    hex::encode(hasher.finalize())
}

fn run<F>(request: &InstallRequest, emit: &F, report: &mut InstallReport) -> Result<()>
where
    F: Fn(InstallEvent) + Sync,
{
    emit(InstallEvent::phase(Phase::Package));
    log_requested_paths(request, emit);
    validate_request(request)?;

    emit(InstallEvent::log("Loading MPI package..."));
    let (mpi_dir, cleanup_needed, mpi_store, mpi_index) = load_mpi_package(request, emit)?;
    let result = install_from_package(request, emit, report, &mpi_dir, mpi_store, mpi_index);
    if cleanup_needed {
        emit(InstallEvent::log("Cleaning up..."));
        if let Err(e) = MpiExtractor::cleanup_temp(&mpi_dir) {
            emit(InstallEvent::warning(format!(
                "The extracted package could not be removed: {e:#}"
            )));
        }
    }
    result
}

fn install_from_package<F>(
    request: &InstallRequest,
    emit: &F,
    report: &mut InstallReport,
    mpi_dir: &Path,
    mpi_store: Option<MpiStore>,
    mpi_index: Option<MpiPathIndex>,
) -> Result<()>
where
    F: Fn(InstallEvent) + Sync,
{
    let manifest_path = find_manifest(mpi_dir)?;
    emit(InstallEvent::log(format!(
        "Loading manifest: {}",
        manifest_path.display()
    )));
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest = ManifestLoader::load_from_bytes(&manifest_bytes)?;

    let package_name = manifest
        .package
        .as_ref()
        .and_then(|p| p.title.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let package_version = manifest
        .package
        .as_ref()
        .and_then(|p| p.version.clone())
        .unwrap_or_else(|| "?".to_string());
    report.package_name = Some(package_name.clone());
    report.package_version = Some(package_version.clone());
    emit(InstallEvent::log(format!(
        "Package: {} v{}",
        package_name, package_version
    )));

    // Preflight: nothing is written before the manifest's features, version and game copies are known good.
    emit(InstallEvent::phase(Phase::Preflight));
    let assets = ManifestLoader::parse_assets(&manifest)?;
    report.assets.total = assets.len() as u64;
    let profile_index = ManifestLoader::select_best_profile(&manifest);
    emit(InstallEvent::log(format!(
        "Using profile {}",
        profile_index
    )));
    let needs = check_manifest(&manifest, &assets, profile_index)?;
    match ttw_version_support(&package_name, &package_version) {
        VersionSupport::Supported => report.package_supported = true,
        VersionSupport::NotTtw => report.package_supported = true,
        VersionSupport::Unsupported(version) => {
            if !request.allow_unsupported_version {
                return Err(InstallError::new(
                    ErrorCode::UnsupportedVersion,
                    format!(
                        "TTW {version} is not a version this ttwright supports ({}); \
                         --allow-unsupported-version installs it anyway",
                        ttwright_contract::TTW_VERSIONS.join(", ")
                    ),
                )
                .with_feature(format!("ttw_version:{version}"))
                .into());
            }
            emit(InstallEvent::warning(format!(
                "TTW {version} is not a supported version; installing it because it was asked for"
            )));
        }
    }
    let game_paths = resolve_game_paths(request, needs, emit)?;
    log_resolved_game_paths(&game_paths, emit);

    let locations = ManifestLoader::get_locations(&manifest, profile_index)?;
    let bsa_targets = ManifestLoader::get_bsa_target_locations(&manifest)?;
    emit(InstallEvent::log(format!(
        "Found {} BSA targets",
        bsa_targets.len()
    )));
    let variables = ManifestLoader::get_variables(&manifest, profile_index)
        .or_else(|_| ManifestLoader::get_variables(&manifest, 0))
        .unwrap_or_default();

    let destination = request.destination_path.to_string_lossy().to_string();
    let path_text = |p: &Option<PathBuf>| {
        p.as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let config = InstallConfig {
        fallout3_root: path_text(&game_paths.fallout3),
        falloutnv_root: path_text(&game_paths.falloutnv),
        oblivion_root: path_text(&game_paths.oblivion),
        destination_path: destination.clone(),
        mpi_package_path: mpi_dir.to_string_lossy().to_string(),
    };

    // A cancel that arrives before anything is written (the package read, the preflight) ends the install here, so
    // no journal and no destination folder is left behind and no resume is offered.
    if request.cancel.is_cancelled() {
        return Err(InstallError::new(
            ErrorCode::Cancelled,
            "The install was cancelled before it started; run it again to install",
        )
        .into());
    }
    let journal = if request.dry_run {
        Journal::in_memory()
    } else {
        fs::create_dir_all(&request.destination_path)
            .with_context(|| format!("Failed to create {}", request.destination_path.display()))?;
        let inputs = digest(&[
            config.fallout3_root.as_bytes(),
            config.falloutnv_root.as_bytes(),
            config.oblivion_root.as_bytes(),
            destination.as_bytes(),
        ]);
        Journal::open(
            &request.destination_path,
            &digest(&[&manifest_bytes]),
            &inputs,
            request.resume,
        )?
    };
    report.resumed = journal.resumed();
    let journal = Arc::new(journal);
    emit(InstallEvent::Event(Event::Started {
        package: package_name.clone(),
        version: package_version.clone(),
        assets_total: report.assets.total,
        resumed: report.resumed,
    }));

    let resolver = LocationResolver::new(locations.clone(), config).with_variables(&variables);

    // The manifest's checks: the game copies must be the expected, unmodified ones.
    emit(InstallEvent::phase(Phase::Checks));
    let checks = ManifestLoader::get_checks(&manifest);
    if !checks.is_empty() {
        log_check_file_diagnostics(&FileVerifier::new(&resolver), &checks, emit);
        emit(InstallEvent::log(format!(
            "Running {} pre-installation checks...",
            checks.len()
        )));
        // On resume, a "file must not exist" check passes for the files the interrupted run wrote.
        let own_files = if report.resumed {
            files_written_by_journal(&resolver, &assets, &journal, &request.destination_path)
        } else {
            Default::default()
        };
        let verification_result = FileVerifier::new(&resolver)
            .with_own_files(own_files)
            .run_checks(&checks)?;
        emit(InstallEvent::progress(
            Phase::Checks,
            (verification_result.passed + verification_result.failed) as u64,
            checks.len() as u64,
            "",
        ));
        if !verification_result.is_success() {
            // On resume, a file inside the destination that the journal does not explain is not the package's
            // mistake of installing over a game folder: the folder holds files this install did not write.
            let foreign: Vec<&crate::services::InvertedFailure> = if report.resumed {
                verification_result
                    .inverted_failures
                    .iter()
                    .filter(|f| f.path.starts_with(&request.destination_path))
                    .collect()
            } else {
                Vec::new()
            };
            for err in &verification_result.errors {
                emit(InstallEvent::log(format!("CHECK FAILED: {}", err)));
                if !foreign.iter().any(|f| &f.error == err) {
                    report.note(err.clone());
                }
            }
            for f in &foreign {
                report.note(format!(
                    "{} is in the destination, but this install did not write it",
                    f.path.display()
                ));
            }
            let message = if !foreign.is_empty() && foreign.len() == verification_result.failed {
                format!(
                    "Cannot resume: the destination {} holds files this install did not write ({}). \
                     Use an empty folder, or remove those files and run again",
                    request.destination_path.display(),
                    foreign
                        .iter()
                        .map(|f| f.path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!(
                    "Verification failed: {} checks failed. Please ensure you have valid, unmodified game files.",
                    verification_result.failed
                )
            };
            return Err(InstallError::new(ErrorCode::CheckFailed, message).into());
        }
        emit(InstallEvent::log(format!(
            "All {} checks passed",
            verification_result.passed
        )));
    }
    report.checks_passed = true;

    emit(InstallEvent::phase(Phase::Assets));
    // A dry run keeps the processor's staging files (archive builders, the archive cache) out of the
    // destination, in a folder of its own that is removed at the end.
    let staging = if request.dry_run {
        let dir = mpi_work_dir(request).with_extension("staging");
        fs::create_dir_all(&dir)?;
        Some(StagingDir(dir))
    } else {
        None
    };
    let work_dir = staging
        .as_ref()
        .map_or_else(|| request.destination_path.clone(), |s| s.0.clone());
    let xdelta = XdeltaManager::ensure_available(work_dir.clone())?;
    let mut processor = AssetProcessor::new(
        resolver,
        xdelta,
        mpi_dir.to_path_buf(),
        work_dir,
        &locations,
        &bsa_targets,
    )?
    .with_dry_run(request.dry_run)
    .with_cancel(request.cancel.clone())
    .with_journal(journal.clone());
    if let Some(store) = mpi_store {
        processor = processor.with_mpi_store(store);
    }
    if let Some(index) = mpi_index {
        processor = processor.with_mpi_index(index);
    }

    // On resume, skip the loose files already written and every file of an archive already written.
    let archive_locations = processor.archive_locations();
    let archive_total = archive_locations
        .iter()
        .filter(|loc| assets.iter().any(|a| a.target_loc == **loc))
        .count() as u64;
    let archives_resumed = archive_locations
        .iter()
        .filter(|loc| journal.archive_done(**loc))
        .count() as u64;
    let todo: Vec<_> = assets
        .iter()
        .filter(|a| {
            if archive_locations.contains(&a.target_loc) {
                !journal.archive_done(a.target_loc)
            } else {
                !journal.asset_done(a.index)
            }
        })
        .cloned()
        .collect();
    report.assets.resumed = (assets.len() - todo.len()) as u64;
    report.archives.total = archive_total;
    report.archives.resumed = archives_resumed;
    if report.assets.resumed > 0 {
        emit(InstallEvent::log(format!(
            "Resuming: {} of {} assets were done by the earlier run",
            report.assets.resumed,
            assets.len()
        )));
    }

    emit(InstallEvent::log("Processing assets (streaming mode)..."));
    let resumed = report.assets.resumed;
    let total = report.assets.total;
    let stats = processor.process_assets_streaming_with_callback(&todo, |current, _, msg| {
        emit(InstallEvent::progress(
            Phase::Assets,
            resumed + current as u64,
            total,
            msg,
        ));
    })?;
    journal.flush()?;
    report.assets.done = stats.success as u64;
    report.assets.failed = stats.failed as u64;
    report.archives.done = stats.bsa_success as u64;
    report.archives.failed = stats.bsa_failed as u64;
    emit(InstallEvent::log(format!(
        "Processed: {} success, {} failed; BSA archives: {} created, {} failed",
        stats.success, stats.failed, stats.bsa_success, stats.bsa_failed
    )));
    for err in &stats.errors {
        emit(InstallEvent::log(format!("Asset error: {}", err)));
        report.note(err.clone());
    }
    if stats.cancelled {
        return Err(InstallError::new(
            ErrorCode::Cancelled,
            "The install was cancelled; resuming it continues from where it stopped",
        )
        .into());
    }
    if stats.failed > 0 || stats.bsa_failed > 0 {
        let missing_patch_errors = stats
            .errors
            .iter()
            .filter(|e| e.contains("Patch file not found"))
            .count();
        let message = failure_message(stats.failed, stats.bsa_failed, 0, missing_patch_errors)
            .unwrap_or_default();
        let code = if stats.failed > 0 {
            ErrorCode::AssetFailed
        } else {
            ErrorCode::ArchiveFailed
        };
        return Err(InstallError::new(code, format!("Installation failed: {message}")).into());
    }

    emit(InstallEvent::phase(Phase::PostInstall));
    if journal.post_install_done() {
        emit(InstallEvent::log(
            "Post-installation steps were done by the earlier run",
        ));
    } else if !request.dry_run {
        let post_commands = ManifestLoader::get_post_commands(&manifest);
        let file_attrs = manifest.file_attrs.clone().unwrap_or_default();
        report.post_install.total = (post_commands.len() + file_attrs.len()) as u64;
        emit(InstallEvent::log("Executing post-installation commands..."));
        let (ok, failed) = ManifestLoader::execute_post_commands(&post_commands, &destination)?;
        let (set, unset) = apply_file_attrs(&file_attrs, &destination)?;
        report.post_install.done = (ok + set) as u64;
        report.post_install.failed = (failed + unset) as u64;
        emit(InstallEvent::progress(
            Phase::PostInstall,
            report.post_install.done + report.post_install.failed,
            report.post_install.total,
            "",
        ));
        if failed + unset > 0 {
            return Err(InstallError::new(
                ErrorCode::PostCommandFailed,
                format!(
                    "Installation failed: {failed} post-install commands and {unset} file times failed"
                ),
            )
            .into());
        }
        journal.record_post_install();
    }

    journal.finish()?;
    emit(InstallEvent::log("Installation complete"));
    Ok(())
}

/// A dry run's staging folder, removed when dropped.
struct StagingDir(PathBuf);

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn validate_request(request: &InstallRequest) -> Result<()> {
    if !request.mpi_path.exists() {
        return Err(InstallError::new(
            ErrorCode::MpiNotFound,
            format!("MPI package not found: {}", request.mpi_path.display()),
        )
        .into());
    }
    for (path, game) in [
        (&request.fallout3_path, "fallout3"),
        (&request.falloutnv_path, "falloutnv"),
        (&request.oblivion_path, "oblivion"),
    ] {
        if let Some(path) = path
            && !path.is_dir()
        {
            return Err(InstallError::new(
                ErrorCode::GameNotFound,
                format!("The {game} folder does not exist: {}", path.display()),
            )
            .with_feature(game)
            .into());
        }
    }
    Ok(())
}

fn log_requested_paths<F>(request: &InstallRequest, emit: &F)
where
    F: Fn(InstallEvent) + Sync,
{
    emit(InstallEvent::log("Install request paths:"));
    emit(InstallEvent::log(format!(
        "  MPI package: {}",
        request.mpi_path.display()
    )));
    emit(InstallEvent::log(format!(
        "  Fallout 3: {}",
        optional_path(request.fallout3_path.as_ref())
    )));
    emit(InstallEvent::log(format!(
        "  Fallout New Vegas: {}",
        optional_path(request.falloutnv_path.as_ref())
    )));
    emit(InstallEvent::log(format!(
        "  Oblivion: {}",
        optional_path(request.oblivion_path.as_ref())
    )));
    emit(InstallEvent::log(format!(
        "  Output: {}",
        request.destination_path.display()
    )));
}

fn log_resolved_game_paths<F>(paths: &ResolvedGamePaths, emit: &F)
where
    F: Fn(InstallEvent) + Sync,
{
    emit(InstallEvent::log("Resolved game paths:"));
    emit(InstallEvent::log(format!(
        "  Fallout 3: {}",
        optional_path(paths.fallout3.as_ref())
    )));
    emit(InstallEvent::log(format!(
        "  Fallout New Vegas: {}",
        optional_path(paths.falloutnv.as_ref())
    )));
    emit(InstallEvent::log(format!(
        "  Oblivion: {}",
        optional_path(paths.oblivion.as_ref())
    )));
}

#[derive(Debug, Clone, Default)]
struct ResolvedGamePaths {
    fallout3: Option<PathBuf>,
    falloutnv: Option<PathBuf>,
    oblivion: Option<PathBuf>,
}

/// The game copies given, plus (only when `detect_missing`) detected ones for those the manifest needs. A copy
/// the manifest needs and nobody gave is `game_path_missing`.
fn resolve_game_paths<F>(
    request: &InstallRequest,
    needs: GameNeeds,
    emit: &F,
) -> Result<ResolvedGamePaths>
where
    F: Fn(InstallEvent) + Sync,
{
    let mut paths = ResolvedGamePaths {
        fallout3: request.fallout3_path.clone(),
        falloutnv: request.falloutnv_path.clone(),
        oblivion: request.oblivion_path.clone(),
    };
    let missing = (needs.fallout3 && paths.fallout3.is_none())
        || (needs.falloutnv && paths.falloutnv.is_none())
        || (needs.oblivion && paths.oblivion.is_none());

    if missing && request.detect_missing {
        emit(InstallEvent::log(
            "Auto-detecting missing game installation paths...",
        ));
        let detected = GameDetection::detect();
        for (slot, game) in [
            (&mut paths.fallout3, detected.fallout3),
            (&mut paths.falloutnv, detected.falloutnv),
            (&mut paths.oblivion, detected.oblivion),
        ] {
            if slot.is_none()
                && let Some(game) = game
            {
                emit(InstallEvent::log(format!(
                    "Detected {} via {}: {}",
                    game.kind.display_name(),
                    game.source,
                    game.path.display()
                )));
                *slot = Some(game.path);
            }
        }
    }

    for (needed, path, game) in [
        (needs.fallout3, &paths.fallout3, "fallout3"),
        (needs.falloutnv, &paths.falloutnv, "falloutnv"),
        (needs.oblivion, &paths.oblivion, "oblivion"),
    ] {
        if needed && path.is_none() {
            return Err(InstallError::new(
                ErrorCode::GamePathMissing,
                format!("The package needs the {game} copy, and none was given"),
            )
            .with_feature(game)
            .into());
        }
    }
    Ok(paths)
}

fn log_check_file_diagnostics<F>(verifier: &FileVerifier<'_>, checks: &[Check], emit: &F)
where
    F: Fn(InstallEvent) + Sync,
{
    let file_checks: Vec<(usize, &Check)> = checks
        .iter()
        .enumerate()
        .filter(|(_, check)| check.check_type == 0 && check.file.is_some())
        .collect();

    if file_checks.is_empty() {
        return;
    }

    emit(InstallEvent::log("Pre-check file diagnostics:"));
    for (index, check) in file_checks {
        let expectation = if check.inverted {
            "expected absent"
        } else {
            "expected present"
        };

        match verifier.check_file_path(check) {
            Ok(path) => {
                match fs::metadata(&path) {
                    Ok(metadata) => {
                        if check.inverted {
                            emit(InstallEvent::log(format!(
                                "  [{}] {} exists, size {} (expected absent; this check should fail)",
                                index + 1,
                                path.display(),
                                format_bytes(metadata.len())
                            )));
                        } else {
                            emit(InstallEvent::log(format!(
                                "  [{}] {} exists, size {} ({}; this check should pass)",
                                index + 1,
                                path.display(),
                                format_bytes(metadata.len()),
                                expectation
                            )));
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        if check.inverted {
                            emit(InstallEvent::log(format!(
                                "  [{}] {} absent ({}; this check should pass)",
                                index + 1,
                                path.display(),
                                expectation
                            )));
                        } else {
                            emit(InstallEvent::log(format!(
                                "  [{}] {} missing ({}; this check should fail): {}",
                                index + 1,
                                path.display(),
                                expectation,
                                err
                            )));
                        }
                    }
                    Err(err) => emit(InstallEvent::log(format!(
                        "  [{}] {} unreadable ({}): {}",
                        index + 1,
                        path.display(),
                        expectation,
                        err
                    ))),
                }

                if check
                    .checksums
                    .as_ref()
                    .is_some_and(|checksums| !checksums.trim().is_empty())
                {
                    match fs::read(&path) {
                        Ok(data) => emit(InstallEvent::log(format!(
                            "  [{}] hashes: sha1={}, md5={}",
                            index + 1,
                            compute_sha1(&data),
                            compute_md5(&data)
                        ))),
                        Err(err) => emit(InstallEvent::log(format!(
                            "  [{}] hashes unavailable: {}",
                            index + 1,
                            err
                        ))),
                    }
                }
            }
            Err(err) => emit(InstallEvent::log(format!(
                "  [{}] could not resolve check file path: {}",
                index + 1,
                err
            ))),
        }
    }
}

fn optional_path(path: Option<&PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "<not provided>".to_string())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.2} {}", value, UNITS[unit])
    }
}

fn load_mpi_package<F>(
    request: &InstallRequest,
    emit: &F,
) -> Result<(PathBuf, bool, Option<MpiStore>, Option<MpiPathIndex>)>
where
    F: Fn(InstallEvent) + Sync,
{
    if MpiExtractor::is_mpi_file(&request.mpi_path) {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        let available_gb = sys.available_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let use_inmemory = available_gb >= 10.0;
        let extract_dir = mpi_work_dir(request);

        if use_inmemory {
            let store = MpiStore::load(&request.mpi_path).map_err(mpi_invalid)?;
            std::fs::create_dir_all(&extract_dir)?;
            let manifest_dir = extract_dir.join("_package");
            std::fs::create_dir_all(&manifest_dir)?;

            if let Some(manifest_data) = store.get_manifest() {
                std::fs::write(manifest_dir.join("index.json"), manifest_data)?;
            } else {
                return Err(InstallError::new(
                    ErrorCode::ManifestMissing,
                    "No manifest found in MPI package (_package/index.json)",
                )
                .into());
            }

            Ok((extract_dir, true, Some(store), None))
        } else {
            emit(InstallEvent::log(format!(
                "System has {:.1} GB available RAM, using disk extraction (need 10+ GB for in-memory mode)",
                available_gb
            )));
            let extracted =
                MpiExtractor::extract_to(&request.mpi_path, &extract_dir).map_err(mpi_invalid)?;
            let index = MpiPathIndex::build(&extracted)?;
            Ok((extracted, true, None, Some(index)))
        }
    } else if request.mpi_path.is_dir() {
        let index = MpiPathIndex::build(&request.mpi_path)?;
        Ok((request.mpi_path.clone(), false, None, Some(index)))
    } else if let Some(message) = MpiExtractor::not_a_package_message(&request.mpi_path) {
        Err(InstallError::new(ErrorCode::NotAPackage, message).into())
    } else {
        Err(InstallError::new(
            ErrorCode::MpiNotFound,
            format!("MPI package not found: {}", request.mpi_path.display()),
        )
        .into())
    }
}

fn mpi_work_dir(request: &InstallRequest) -> PathBuf {
    if request.dry_run {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ttw_mpi_package_{}_{}",
            std::process::id(),
            timestamp
        ))
    } else {
        request.destination_path.join(".mpi_package")
    }
}

pub fn find_manifest(mpi_dir: impl AsRef<std::path::Path>) -> Result<PathBuf> {
    let mpi_dir = mpi_dir.as_ref();
    let candidates = [
        "_package/index.json",
        "manifest.json",
        "Manifest.json",
        "TTW.manifest.json",
        "ttw.manifest.json",
        "index.json",
    ];

    for name in candidates {
        let path = mpi_dir.join(name);
        if path.exists() {
            return Ok(path);
        }
    }

    for entry in walkdir::WalkDir::new(mpi_dir).max_depth(3) {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if (name.contains("manifest") || name == "index.json")
            && entry
                .path()
                .extension()
                .map(|e| e == "json")
                .unwrap_or(false)
        {
            return Ok(entry.path().to_path_buf());
        }
    }

    Err(InstallError::new(
        ErrorCode::ManifestMissing,
        format!("Manifest not found in: {}", mpi_dir.display()),
    )
    .into())
}

fn failure_message(
    asset_failed: usize,
    bsa_failed: usize,
    post_failed: usize,
    missing_patch_errors: usize,
) -> Option<String> {
    let mut failure_reasons = Vec::new();
    if asset_failed > 0 {
        failure_reasons.push(format!("{} asset operations failed", asset_failed));
    }
    if bsa_failed > 0 {
        failure_reasons.push(format!("{} BSA archives failed to write", bsa_failed));
    }
    if post_failed > 0 {
        failure_reasons.push(format!("{} post-install commands failed", post_failed));
    }

    if failure_reasons.is_empty() {
        None
    } else if missing_patch_errors > 0 {
        Some(format!(
            "{}. Detected {} missing patch files (.xd3). This usually means the extracted MPI package is incomplete or mismatched for this manifest.",
            failure_reasons.join("; "),
            missing_patch_errors
        ))
    } else {
        Some(failure_reasons.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallRequest, failure_message, mpi_work_dir};
    use std::path::PathBuf;

    fn request(dry_run: bool) -> InstallRequest {
        InstallRequest {
            mpi_path: PathBuf::from("/tmp/package.mpi"),
            destination_path: PathBuf::from("/tmp/ttw-output"),
            dry_run,
            ..InstallRequest::default()
        }
    }

    #[test]
    fn dry_run_mpi_work_dir_uses_temp_dir() {
        let dir = mpi_work_dir(&request(true));
        assert!(dir.starts_with(std::env::temp_dir()));
        assert!(
            dir.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ttw_mpi_")
        );
    }

    #[test]
    fn real_install_mpi_work_dir_uses_destination_package_dir() {
        assert_eq!(
            mpi_work_dir(&request(false)),
            PathBuf::from("/tmp/ttw-output/.mpi_package")
        );
    }

    #[test]
    fn failure_message_mentions_missing_patches() {
        let message = failure_message(2, 0, 0, 2).unwrap();
        assert!(message.contains("2 asset operations failed"));
        assert!(message.contains("missing patch files"));
    }
}
