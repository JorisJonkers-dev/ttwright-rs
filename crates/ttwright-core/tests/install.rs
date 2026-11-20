// SPDX-License-Identifier: GPL-3.0-only
//! End-to-end installs of synthetic packages through the library.

use std::path::Path;
use std::sync::Mutex;

use serde_json::json;
use ttwright_contract::{ErrorCode, Event, Phase};
use ttwright_core::app::{
    CancelToken, InstallEvent, InstallOutcome, InstallRequest, JOURNAL_FILE, run_install,
};
use ttwright_core::services::BsaHandler;
use ttwright_testkit::{Fixture, PATCH_TARGET, loc};

fn request(f: &Fixture) -> InstallRequest {
    InstallRequest {
        mpi_path: f.mpi.clone(),
        fallout3_path: Some(f.fallout3.clone()),
        destination_path: f.dest.clone(),
        ..InstallRequest::default()
    }
}

fn install(request: InstallRequest) -> (InstallOutcome, Vec<Event>) {
    let events = Mutex::new(Vec::new());
    let outcome = run_install(request, |e| {
        if let InstallEvent::Event(e) = e {
            events.lock().unwrap().push(e);
        }
    });
    (outcome, events.into_inner().unwrap())
}

fn code(outcome: &InstallOutcome) -> (ErrorCode, Option<String>) {
    let e = outcome.error.as_ref().expect("the install should fail");
    (e.code, e.feature.clone())
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn every_supported_operation_installs() {
    let f = Fixture::new("all-ops");
    let (outcome, events) = install(request(&f));
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let r = &outcome.report;
    assert!(r.checks_passed && r.package_supported && !r.resumed);
    assert_eq!((r.assets.total, r.assets.done, r.assets.failed), (6, 6, 0));
    assert_eq!((r.archives.total, r.archives.done), (1, 1));
    assert_eq!((r.post_install.total, r.post_install.done), (2, 2));

    assert_eq!(read(&f.dest.join("loose.txt")), ttwright_testkit::LOOSE);
    assert_eq!(read(&f.dest.join("meshes/a.nif")), b"synthetic mesh");
    assert_eq!(read(&f.dest.join("renamed.txt")), b"new synthetic file");
    assert!(!f.dest.join("new.txt").exists());
    assert_eq!(read(&f.dest.join("patched.bin")), PATCH_TARGET);
    assert!(read(&f.dest.join("sound/a.ogg")).starts_with(b"OggS"));
    let archive = read(&f.dest.join("Out.bsa"));
    assert_eq!(
        BsaHandler::extract_from_bytes(&archive, "textures/in-archive.txt").unwrap(),
        ttwright_testkit::LOOSE
    );
    let modified = std::fs::metadata(f.dest.join("Out.bsa"))
        .unwrap()
        .modified()
        .unwrap();
    let expected = time::OffsetDateTime::parse(
        "2008-10-01T12:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert_eq!(
        modified,
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(expected.unix_timestamp() as u64)
    );
    assert!(
        !f.dest.join(JOURNAL_FILE).exists(),
        "a finished install removes its journal"
    );
    assert!(!f.dest.join(".mpi_package").exists());

    let phases: Vec<Phase> = events
        .iter()
        .filter_map(|e| match e {
            Event::Phase { phase } => Some(*phase),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        [
            Phase::Package,
            Phase::Preflight,
            Phase::Checks,
            Phase::Assets,
            Phase::PostInstall
        ]
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Started {
            assets_total: 6,
            ..
        }
    )));
}

#[test]
fn an_extracted_package_folder_installs() {
    let f = Fixture::new("folder");
    let folder = f.dir.join("extracted");
    let bytes = read(&f.mpi);
    let (archive, _) = {
        use ba2::Reader;
        ba2::tes4::Archive::read(ba2::Borrowed(&bytes)).unwrap()
    };
    for (dir, files) in &archive {
        use ba2::ByteSlice;
        for (name, file) in files {
            let path = folder
                .join(String::from_utf8_lossy(dir.name().as_bytes()).replace('\\', "/"))
                .join(String::from_utf8_lossy(name.name().as_bytes()).as_ref());
            ttwright_testkit::write(&path, file.as_bytes());
        }
    }
    let mut req = request(&f);
    req.mpi_path = folder;
    let (outcome, _) = install(req);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(read(&f.dest.join("patched.bin")), PATCH_TARGET);
}

#[test]
fn an_unsupported_operation_stops_before_anything_is_written() {
    let mut m = Fixture::manifest();
    m.asset(
        3,
        "",
        loc::GAME_DATA,
        loc::DEST,
        "sound\\a.xwm",
        Some("sound\\a.fuz"),
    );
    let f = Fixture::with_manifest("unsupported-op", &m);
    let (outcome, _) = install(request(&f));
    assert_eq!(
        code(&outcome),
        (ErrorCode::UnsupportedFeature, Some("asset_op:3".into()))
    );
    let written: Vec<_> = std::fs::read_dir(&f.dest)
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(
        written.iter().all(|n| n == ".mpi_package"),
        "nothing but the extracted manifest: {written:?}"
    );
}

#[test]
fn an_unknown_manifest_field_is_refused() {
    let mut m = Fixture::manifest();
    m.set("Signatures", json!([]));
    let f = Fixture::with_manifest("unknown-field", &m);
    let (outcome, _) = install(request(&f));
    assert_eq!(
        code(&outcome),
        (
            ErrorCode::UnsupportedFeature,
            Some("manifest_field:Signatures".into())
        )
    );
}

#[test]
fn an_unsupported_ttw_version_is_refused_unless_allowed() {
    let mut m = Fixture::manifest();
    m.set(
        "Package",
        json!({"Title": "Tale of Two Wastelands", "Version": "3.9.0"}),
    );
    let f = Fixture::with_manifest("version", &m);
    let (outcome, _) = install(request(&f));
    assert_eq!(
        code(&outcome),
        (
            ErrorCode::UnsupportedVersion,
            Some("ttw_version:3.9".into())
        )
    );
    let mut req = request(&f);
    req.allow_unsupported_version = true;
    let (outcome, events) = install(req);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(!outcome.report.package_supported);
    assert!(events.iter().any(|e| matches!(e, Event::Warning { .. })));
}

#[test]
fn another_package_has_no_version_list() {
    let mut m = Fixture::manifest();
    m.set(
        "Package",
        json!({"Title": "Synthetic Archive Tool", "Version": "0.1"}),
    );
    let f = Fixture::with_manifest("other", &m);
    let (outcome, _) = install(request(&f));
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.report.package_supported);
}

#[test]
fn a_needed_game_copy_must_be_given_when_not_detecting() {
    let f = Fixture::new("no-game");
    let mut req = request(&f);
    req.fallout3_path = None;
    let (outcome, _) = install(req);
    assert_eq!(
        code(&outcome),
        (ErrorCode::GamePathMissing, Some("fallout3".into()))
    );
    let mut req = request(&f);
    req.fallout3_path = Some(f.dir.join("not there"));
    let (outcome, _) = install(req);
    assert_eq!(code(&outcome).0, ErrorCode::GameNotFound);
}

#[test]
fn a_failed_check_stops_the_install() {
    let mut m = Fixture::manifest();
    m.check(
        json!({"Type": 0, "Loc": loc::GAME_DATA, "File": "loose.txt", "Checksums": "deadbeef"}),
    );
    let f = Fixture::with_manifest("check", &m);
    let (outcome, _) = install(request(&f));
    assert_eq!(code(&outcome).0, ErrorCode::CheckFailed);
    assert!(!outcome.report.checks_passed);
    assert!(
        outcome
            .report
            .messages
            .iter()
            .any(|m| m.contains("deadbeef"))
    );
}

#[test]
fn bad_packages_have_their_own_codes() {
    let f = Fixture::new("bad");
    let mut req = request(&f);
    req.mpi_path = f.dir.join("none.mpi");
    assert_eq!(code(&install(req).0).0, ErrorCode::MpiNotFound);

    let garbage = f.dir.join("garbage.mpi");
    ttwright_testkit::write(&garbage, b"this is not an archive");
    let mut req = request(&f);
    req.mpi_path = garbage;
    assert_eq!(code(&install(req).0).0, ErrorCode::MpiInvalid);

    let folder = f.dir.join("broken");
    ttwright_testkit::write(&folder.join("_package/index.json"), b"{ not json");
    let mut req = request(&f);
    req.mpi_path = folder;
    assert_eq!(code(&install(req).0).0, ErrorCode::ManifestInvalid);

    let empty = f.dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let mut req = request(&f);
    req.mpi_path = empty;
    assert_eq!(code(&install(req).0).0, ErrorCode::ManifestMissing);
}

#[test]
fn a_dry_run_writes_nothing() {
    let f = Fixture::new("dry");
    let mut req = request(&f);
    req.dry_run = true;
    let (outcome, _) = install(req);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(!f.dest.join("loose.txt").exists());
    assert!(!f.dest.join(JOURNAL_FILE).exists());
}

#[test]
fn a_cancelled_install_resumes_and_skips_what_is_done() {
    let f = Fixture::new("resume");
    let cancel = CancelToken::new();
    let mut req = request(&f);
    req.cancel = cancel.clone();
    // Cancel once the archive-sourced asset is done and the folder assets are about to start.
    let outcome = run_install(req, |e| {
        if let InstallEvent::Event(Event::Progress { message, .. }) = &e
            && message.contains("directory assets")
        {
            cancel.cancel();
        }
    });
    assert_eq!(code(&outcome).0, ErrorCode::Cancelled);
    assert!(f.dest.join(JOURNAL_FILE).is_file(), "the journal is kept");
    assert_eq!(read(&f.dest.join("meshes/a.nif")), b"synthetic mesh");
    assert!(
        !f.dest.join("Out.bsa").exists(),
        "no partial archive is written"
    );

    // Mark the finished file: a resumed run must keep it, not write it again.
    std::fs::write(f.dest.join("meshes/a.nif"), b"kept").unwrap();
    let mut req = request(&f);
    req.resume = true;
    let (outcome, events) = install(req);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let r = &outcome.report;
    assert!(r.resumed);
    assert_eq!((r.assets.resumed, r.assets.done), (1, 5));
    assert_eq!(read(&f.dest.join("meshes/a.nif")), b"kept");
    assert!(f.dest.join("Out.bsa").is_file());
    assert!(!f.dest.join(JOURNAL_FILE).exists());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Started { resumed: true, .. }))
    );
}

#[test]
fn resuming_another_package_is_refused() {
    let f = Fixture::new("mismatch");
    // A run cancelled once the asset phase starts leaves a journal for this package (a cancel before that point
    // writes nothing at all).
    let cancel = CancelToken::new();
    let stopper = cancel.clone();
    let mut req = request(&f);
    req.cancel = cancel;
    let outcome = run_install(req, |e| {
        if let InstallEvent::Event(Event::Phase {
            phase: Phase::Assets,
        }) = e
        {
            stopper.cancel();
        }
    });
    assert_eq!(code(&outcome).0, ErrorCode::Cancelled);

    let mut m = Fixture::manifest();
    m.set(
        "Package",
        json!({"Title": "Tale of Two Wastelands", "Version": "3.4.1"}),
    );
    let other = Fixture::with_manifest("mismatch-other", &m);
    let mut req = request(&f);
    req.mpi_path = other.mpi.clone();
    req.resume = true;
    assert_eq!(code(&install(req).0).0, ErrorCode::ResumeMismatch);
}

#[test]
fn a_7z_archive_is_named_as_one_and_not_reported_missing() {
    let f = Fixture::new("sevenz");
    let archive = f.dir.join("download.7z");
    ttwright_testkit::write(&archive, &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0, 4, 0, 0]);
    let mut req = request(&f);
    req.mpi_path = archive;
    let outcome = install(req).0;
    let error = outcome.error.as_ref().unwrap();
    let (code, message) = (error.code, error.message.clone());
    assert_eq!(code, ErrorCode::NotAPackage);
    assert!(
        message.contains("7-Zip") && message.contains("extract"),
        "{message}"
    );
}

#[test]
fn another_existing_file_is_not_reported_missing_either() {
    let f = Fixture::new("notpkg");
    let other = f.dir.join("readme.txt");
    ttwright_testkit::write(&other, b"hello");
    let mut req = request(&f);
    req.mpi_path = other;
    assert_eq!(code(&install(req).0).0, ErrorCode::NotAPackage);
}

#[test]
fn a_package_header_under_another_extension_is_read_as_a_package() {
    let f = Fixture::new("renamed");
    let renamed = f.dir.join("package.bin");
    ttwright_testkit::write(&renamed, b"BSA\0i\0\0\0 truncated");
    let mut req = request(&f);
    req.mpi_path = renamed;
    // It is treated as a package (and fails as an unreadable one), not refused as a non-package.
    assert_eq!(code(&install(req).0).0, ErrorCode::MpiInvalid);
}

/// A fixture whose manifest also refuses an install where `file` already exists in the destination (the shape of
/// the TTW package's "must not exist" check on `FalloutNV.esm`).
fn fixture_refusing(label: &str, file: &str) -> Fixture {
    let mut m = Fixture::manifest();
    m.check(
        json!({"Type": 0, "Inverted": true, "Loc": loc::DEST, "File": file,
        "CustomMessage": "install using a mod manager into an empty folder"}),
    );
    Fixture::with_manifest(label, &m)
}

/// Run the install and cancel it once the directory assets are about to start: `meshes/a.nif` is written and
/// journaled, the rest is not.
fn interrupt(f: &Fixture) {
    let cancel = CancelToken::new();
    let mut req = request(f);
    req.cancel = cancel.clone();
    let outcome = run_install(req, |e| {
        if let InstallEvent::Event(Event::Progress { message, .. }) = &e
            && message.contains("directory assets")
        {
            cancel.cancel();
        }
    });
    assert_eq!(code(&outcome).0, ErrorCode::Cancelled);
    assert!(f.dest.join(JOURNAL_FILE).is_file());
}

#[test]
fn a_first_run_into_a_folder_holding_the_file_is_still_refused() {
    let f = fixture_refusing("inverted-first", "meshes/a.nif");
    std::fs::create_dir_all(f.dest.join("meshes")).unwrap();
    std::fs::write(f.dest.join("meshes/a.nif"), b"somebody else's").unwrap();
    let (outcome, _) = install(request(&f));
    assert_eq!(code(&outcome).0, ErrorCode::CheckFailed);
    assert!(outcome.report.notes.iter().any(|n| n.contains("a.nif")));

    // --resume without a journal of this install explains nothing either.
    let mut req = request(&f);
    req.resume = true;
    assert_eq!(code(&install(req).0).0, ErrorCode::CheckFailed);
    assert_eq!(read(&f.dest.join("meshes/a.nif")), b"somebody else's");
}

#[test]
fn a_resume_passes_the_must_not_exist_check_for_a_file_the_interrupted_run_wrote() {
    let f = fixture_refusing("inverted-resume", "meshes/a.nif");
    interrupt(&f);
    assert!(f.dest.join("meshes/a.nif").is_file());
    let mut req = request(&f);
    req.resume = true;
    let (outcome, _) = install(req);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.report.resumed && outcome.report.checks_passed);
    assert!(f.dest.join("Out.bsa").is_file());
    assert!(!f.dest.join(JOURNAL_FILE).exists());
}

#[test]
fn a_resume_refuses_a_file_the_journal_does_not_explain_in_ttwrights_words() {
    let f = fixture_refusing("inverted-foreign", "foreign.txt");
    interrupt(&f);
    std::fs::write(f.dest.join("foreign.txt"), b"not written by the install").unwrap();
    let mut req = request(&f);
    req.resume = true;
    let outcome = install(req).0;
    let (error_code, _) = code(&outcome);
    assert_eq!(error_code, ErrorCode::CheckFailed);
    let message = outcome.error.as_ref().unwrap().message.clone();
    assert!(
        message.contains("did not write") && message.contains("foreign.txt"),
        "{message}"
    );
    assert!(
        !message.contains("unmodified game files") && !message.contains("mod manager"),
        "{message}"
    );
    assert!(
        outcome
            .report
            .notes
            .iter()
            .all(|n| !n.contains("mod manager"))
    );
}
