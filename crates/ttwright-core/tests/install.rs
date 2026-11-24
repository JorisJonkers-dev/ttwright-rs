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
use ttwright_testkit::{Fixture, PATCH_TARGET, covers, ids, loc};

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
    // covers-table: one package that uses every supported operation, installed end to end and checked file by file
    covers(&[
        ids::install::features::MANIFEST_FIELD_PACKAGE,
        ids::install::features::MANIFEST_FIELD_VARIABLES,
        ids::install::features::MANIFEST_FIELD_LOCATIONS,
        ids::install::features::MANIFEST_FIELD_ASSETS,
        ids::install::features::MANIFEST_FIELD_CHECKS,
        ids::install::features::MANIFEST_FIELD_FILEATTRS,
        ids::install::features::MANIFEST_FIELD_POSTCOMMANDS,
        ids::install::features::ASSET_OP_0,
        ids::install::features::ASSET_OP_1,
        ids::install::features::ASSET_OP_2,
        ids::install::features::ASSET_OP_5,
        ids::install::features::LOCATION_TYPE_0,
        ids::install::features::LOCATION_TYPE_1,
        ids::install::features::LOCATION_TYPE_2,
        ids::install::features::VARIABLE_TYPE_4,
        ids::install::features::VARIABLE__DESTINATION_,
        ids::install::features::VARIABLE__FO3ROOT_,
        ids::install::features::CHECK_TYPE_0,
        ids::install::features::CHECK_TYPE_1,
        ids::install::features::CHECK_TYPE_2,
        ids::install::features::POST_COMMAND_REN,
        ids::install::features::AUDIO_FORMAT_OGG,
        ids::install::features::AUDIO_PARAM__F,
        ids::install::features::AUDIO_PARAM__Q,
    ]);
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
    covers(&[ids::install::features::ASSET_OP_2]);
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
    covers(&[ids::install::features::CHECK_TYPE_0]);
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
    covers(&[
        ids::install::features::ASSET_OP_0,
        ids::install::features::LOCATION_TYPE_1,
        ids::install::features::LOCATION_TYPE_2,
    ]);
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

// ---- ttwright never writes outside the destination ----

mod containment {
    use super::*;
    use ttwright_contract::{REDIRECT_DIR, RedirectedStep};
    use ttwright_testkit::{Manifest, TempDir};

    /// The manifest of [`Fixture`] plus one location at `outside` (a folder that is not the destination), which
    /// `extra` may use; `extra` gets the manifest and the index of that location.
    fn with_outside(outside: &Path, extra: impl FnOnce(&mut Manifest, usize)) -> Manifest {
        let mut m = Fixture::manifest();
        let idx = m.location("FNV Root", 0, &outside.display().to_string());
        extra(&mut m, idx);
        m
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(read) = std::fs::read_dir(dir) {
            for entry in read.flatten() {
                out.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        out.sort();
        out
    }

    #[test]
    fn an_asset_for_an_outside_location_is_redirected_and_nothing_is_written_outside() {
        covers(&[
            ids::install::features::ASSET_OP_0,
            ids::install::features::LOCATION_TYPE_0,
        ]);
        let scratch = TempDir::new("outside-asset");
        let outside = scratch.join("game");
        std::fs::create_dir_all(&outside).unwrap();
        let m = with_outside(&outside, |m, idx| {
            m.asset(
                0,
                "",
                loc::GAME_DATA,
                idx,
                "loose.txt",
                Some("libvorbis.dll"),
            );
            m.asset(
                0,
                "",
                loc::GAME_DATA,
                idx,
                "loose.txt",
                Some("Data\\sub\\b.txt"),
            );
        });
        let f = Fixture::with_manifest("redirect", &m);
        let (outcome, _) = install(request(&f));
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert!(files_in(&outside).is_empty(), "nothing is written outside");
        let root = f.dest.join(REDIRECT_DIR);
        let r = &outcome.report;
        assert_eq!(r.redirected_total, 2);
        assert_eq!(r.redirected.len(), 2);
        assert!(r.redirected.iter().all(|x| x.step == RedirectedStep::Asset));
        assert!(r.redirected.iter().all(|x| x.location == "FNV Root"));
        let written: Vec<&str> = r.redirected.iter().map(|x| x.written_to.as_str()).collect();
        assert!(
            written.iter().all(|w| w.starts_with(REDIRECT_DIR)),
            "{written:?}"
        );
        assert_eq!(
            read(&root.join("FNV_Root/libvorbis.dll")),
            ttwright_testkit::LOOSE
        );
        assert_eq!(
            read(&root.join("FNV_Root/Data/sub/b.txt")),
            ttwright_testkit::LOOSE
        );
        // The files for the destination are installed as before.
        assert_eq!(read(&f.dest.join("loose.txt")), ttwright_testkit::LOOSE);
    }

    #[test]
    fn a_dry_run_reports_what_is_redirected_and_writes_nothing() {
        let scratch = TempDir::new("outside-dry");
        let outside = scratch.join("game");
        std::fs::create_dir_all(&outside).unwrap();
        let m = with_outside(&outside, |m, idx| {
            m.asset(
                0,
                "",
                loc::GAME_DATA,
                idx,
                "loose.txt",
                Some("libvorbis.dll"),
            );
        });
        let f = Fixture::with_manifest("redirect-dry", &m);
        let mut req = request(&f);
        req.dry_run = true;
        let (outcome, _) = install(req);
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert_eq!(outcome.report.redirected_total, 1);
        assert_eq!(outcome.report.redirected[0].location, "FNV Root");
        assert!(files_in(&outside).is_empty());
        assert!(!f.dest.join(REDIRECT_DIR).exists());
        assert!(!f.dest.join("loose.txt").exists());
    }

    #[test]
    fn post_install_delete_and_rename_outside_touch_nothing_outside() {
        let scratch = TempDir::new("outside-post");
        let outside = scratch.join("game");
        std::fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, b"keep me").unwrap();
        let other = outside.join("other.txt");
        std::fs::write(&other, b"keep me too").unwrap();
        let m = with_outside(&outside, |m, _| {
            m.post_command(&format!("cmd.exe /C del \"{}\"", victim.display()));
            m.post_command(&format!(
                "cmd.exe /C ren \"{}\" \"moved.txt\"",
                other.display()
            ));
        });
        let f = Fixture::with_manifest("redirect-post", &m);
        let (outcome, _) = install(request(&f));
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert_eq!(read(&victim), b"keep me");
        assert_eq!(read(&other), b"keep me too");
        assert!(!outside.join("moved.txt").exists());
        let r = &outcome.report;
        assert_eq!(r.redirected_total, 2);
        assert!(
            r.redirected
                .iter()
                .all(|x| x.step == RedirectedStep::PostInstall)
        );
    }

    #[test]
    fn dot_dot_in_a_manifest_path_never_leaves_the_destination() {
        let scratch = TempDir::new("outside-dotdot");
        let f_dir = scratch.join("f");
        std::fs::create_dir_all(&f_dir).unwrap();
        let mut m = Fixture::manifest();
        m.asset(
            0,
            "",
            loc::GAME_DATA,
            loc::DEST,
            "loose.txt",
            Some("..\\..\\..\\escaped-dotdot.txt"),
        );
        let f = Fixture::with_manifest("dotdot", &m);
        let (outcome, _) = install(request(&f));
        let _ = outcome;
        for ancestor in f.dest.ancestors().skip(1).take(4) {
            assert!(
                !ancestor.join("escaped-dotdot.txt").exists(),
                "written to {}",
                ancestor.display()
            );
        }
        assert!(!f.dest.join("escaped-dotdot.txt").exists() || f.dest.starts_with(&f.dest));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_the_destination_pointing_outside_is_not_followed() {
        let scratch = TempDir::new("outside-link");
        let outside = scratch.join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        let mut m = Fixture::manifest();
        m.asset(
            0,
            "",
            loc::GAME_DATA,
            loc::DEST,
            "loose.txt",
            Some("link\\x.txt"),
        );
        let f = Fixture::with_manifest("symlink", &m);
        std::fs::create_dir_all(&f.dest).unwrap();
        std::os::unix::fs::symlink(&outside, f.dest.join("link")).unwrap();
        let (outcome, _) = install(request(&f));
        assert!(files_in(&outside).is_empty(), "written through the link");
        assert!(
            outcome.error.is_some() || outcome.report.assets.failed > 0,
            "the asset behind the link must fail"
        );
    }

    #[test]
    fn an_archive_to_create_outside_is_not_written_there() {
        let scratch = TempDir::new("outside-bsa");
        let outside = scratch.join("game");
        std::fs::create_dir_all(&outside).unwrap();
        let mut m = Fixture::manifest();
        let bsa = m.archive_target("Evil.bsa", &format!("{}/Evil.bsa", outside.display()));
        m.asset(
            0,
            "",
            loc::GAME_DATA,
            bsa,
            "loose.txt",
            Some("textures\\x.txt"),
        );
        let f = Fixture::with_manifest("bsa-outside", &m);
        let _ = install(request(&f));
        assert!(files_in(&outside).is_empty(), "{:?}", files_in(&outside));
    }

    #[test]
    fn a_file_time_outside_changes_nothing_outside() {
        let scratch = TempDir::new("outside-attr");
        let outside = scratch.join("game");
        std::fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, b"x").unwrap();
        let before = std::fs::metadata(&victim).unwrap().modified().unwrap();
        let m = with_outside(&outside, |m, _| {
            m.file_attr(&victim.display().to_string(), "2008-10-01T12:00:00+00:00");
        });
        let f = Fixture::with_manifest("attr-outside", &m);
        let (outcome, _) = install(request(&f));
        let after = std::fs::metadata(&victim).unwrap().modified().unwrap();
        assert_eq!(before, after);
        assert!(outcome.report.redirected_total >= 1);
        assert!(
            outcome
                .report
                .redirected
                .iter()
                .any(|x| x.step == RedirectedStep::PostInstall)
        );
    }

    #[test]
    fn a_resumed_install_keeps_the_redirected_files_and_reports_them_again() {
        covers(&[
            ids::install::features::ASSET_OP_0,
            ids::install::features::LOCATION_TYPE_0,
        ]);
        let scratch = TempDir::new("outside-resume");
        let outside = scratch.join("game");
        std::fs::create_dir_all(&outside).unwrap();
        let m = with_outside(&outside, |m, idx| {
            m.asset(
                0,
                "",
                loc::GAME_DATA,
                idx,
                "loose.txt",
                Some("libvorbis.dll"),
            );
        });
        let f = Fixture::with_manifest("redirect-resume", &m);
        let cancel = CancelToken::new();
        let mut req = request(&f);
        req.cancel = cancel.clone();
        let outcome = run_install(req, |e| {
            if let InstallEvent::Event(Event::Progress { message, .. }) = &e
                && message.contains("directory assets")
            {
                cancel.cancel();
            }
        });
        assert_eq!(code(&outcome).0, ErrorCode::Cancelled);

        let mut req = request(&f);
        req.resume = true;
        let (outcome, _) = install(req);
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert!(outcome.report.resumed);
        assert_eq!(outcome.report.redirected_total, 1);
        assert_eq!(
            read(&f.dest.join(REDIRECT_DIR).join("FNV_Root/libvorbis.dll")),
            ttwright_testkit::LOOSE
        );
        assert!(files_in(&outside).is_empty());
    }
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
    covers(&[ids::install::features::CHECK_TYPE_0]);
    let f = fixture_refusing("inverted-first", "meshes/a.nif");
    std::fs::create_dir_all(f.dest.join("meshes")).unwrap();
    std::fs::write(f.dest.join("meshes/a.nif"), b"somebody else's").unwrap();
    let (outcome, _) = install(request(&f));
    assert_eq!(code(&outcome).0, ErrorCode::CheckFailed);
    assert!(outcome.report.messages.iter().any(|n| n.contains("a.nif")));

    // --resume without a journal of this install explains nothing either.
    let mut req = request(&f);
    req.resume = true;
    assert_eq!(code(&install(req).0).0, ErrorCode::CheckFailed);
    assert_eq!(read(&f.dest.join("meshes/a.nif")), b"somebody else's");
}

#[test]
fn a_resume_passes_the_must_not_exist_check_for_a_file_the_interrupted_run_wrote() {
    covers(&[ids::install::features::CHECK_TYPE_0]);
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
    covers(&[ids::install::features::CHECK_TYPE_0]);
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
            .messages
            .iter()
            .all(|n| !n.contains("mod manager"))
    );
}

/// A fixture with one asset for a location outside the destination (so it is written under the redirect folder
/// as `libvorbis.dll`, from the same archive as `meshes/a.nif`, which the interrupted run finishes) and a "must not exist" check on `check_file` inside the destination.
fn fixture_redirected_refusing(label: &str, outside: &Path, check_file: &str) -> Fixture {
    let mut m = Fixture::manifest();
    let idx = m.location("FNV Root", 0, &outside.display().to_string());
    m.asset(
        0,
        "",
        loc::GAME_ARCHIVE,
        idx,
        "meshes\\a.nif",
        Some("libvorbis.dll"),
    );
    m.check(
        json!({"Type": 0, "Inverted": true, "Loc": loc::DEST, "File": check_file,
        "CustomMessage": "install using a mod manager into an empty folder"}),
    );
    Fixture::with_manifest(label, &m)
}

#[test]
fn a_resume_passes_the_must_not_exist_check_for_a_redirected_file_the_interrupted_run_wrote() {
    covers(&[ids::install::features::CHECK_TYPE_0]);
    let scratch = ttwright_testkit::TempDir::new("redirected-own");
    let outside = scratch.join("game");
    std::fs::create_dir_all(&outside).unwrap();
    let redirected = format!("{}/FNV_Root/libvorbis.dll", ttwright_contract::REDIRECT_DIR);
    let f = fixture_redirected_refusing("inverted-redirected-own", &outside, &redirected);
    interrupt(&f);
    assert!(
        f.dest.join(&redirected).is_file(),
        "the interrupted run wrote the redirected file"
    );
    let mut req = request(&f);
    req.resume = true;
    let (outcome, _) = install(req);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.report.resumed && outcome.report.checks_passed);
    assert!(f.dest.join(&redirected).is_file());
}

#[test]
fn a_resume_refuses_a_redirected_file_the_interrupted_run_did_not_write() {
    covers(&[ids::install::features::CHECK_TYPE_0]);
    let scratch = ttwright_testkit::TempDir::new("redirected-foreign");
    let outside = scratch.join("game");
    std::fs::create_dir_all(&outside).unwrap();
    let redirected = format!("{}/FNV_Root/foreign.dll", ttwright_contract::REDIRECT_DIR);
    let f = fixture_redirected_refusing("inverted-redirected-foreign", &outside, &redirected);
    interrupt(&f);
    std::fs::create_dir_all(f.dest.join(&redirected).parent().unwrap()).unwrap();
    std::fs::write(f.dest.join(&redirected), b"not written by the install").unwrap();
    let mut req = request(&f);
    req.resume = true;
    let outcome = install(req).0;
    assert_eq!(code(&outcome).0, ErrorCode::CheckFailed);
    let message = outcome.error.as_ref().unwrap().message.clone();
    assert!(
        message.contains("did not write") && message.contains("foreign.dll"),
        "{message}"
    );
    assert_eq!(
        read(&f.dest.join(&redirected)),
        b"not written by the install"
    );
}
