// SPDX-License-Identifier: GPL-3.0-only
//! [`ttwright_core::api::install`] called in process: the result, progress, cancel and resume, and the errors.

use serde_json::json;
use ttwright_contract::{ErrorCode, Event, Phase, Status};
use ttwright_core::api::{CancelToken, InstallJob, InstallOutput, NoProgress, install};
use ttwright_core::app::JOURNAL_FILE;
use ttwright_testkit::{Fixture, covers, ids, loc};

fn job(f: &Fixture) -> InstallJob {
    InstallJob {
        mpi_path: f.mpi.clone(),
        fallout3_path: Some(f.fallout3.clone()),
        destination_path: f.dest.clone(),
        ..InstallJob::default()
    }
}

#[test]
fn install_writes_the_package_and_answers_done_with_a_success_document() {
    covers(&[
        ids::install::features::ASSET_OP_0,
        ids::install::features::LOCATION_TYPE_0,
        ids::install::features::VARIABLE__DESTINATION_,
    ]);
    let f = Fixture::new("api-done");
    let out = install(&job(&f), &mut NoProgress, &CancelToken::new()).unwrap();
    let InstallOutput::Done { result } = out else {
        panic!("not done: {out:?}")
    };
    assert_eq!(result.status, Status::Success);
    assert_eq!((result.assets.total, result.assets.done), (6, 6));
    assert_eq!(
        result.output_dir.as_deref(),
        Some(f.dest.display().to_string().as_str())
    );
    assert!(f.dest.join("loose.txt").is_file());
}

#[test]
fn progress_gets_every_phase_and_ends_with_finished() {
    let f = Fixture::new("api-progress");
    let mut events: Vec<Event> = Vec::new();
    let mut progress = |e: &Event| events.push(e.clone());
    install(&job(&f), &mut progress, &CancelToken::new()).unwrap();
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
    assert!(events.iter().any(|e| matches!(e, Event::Progress { .. })));
    assert_eq!(
        events.last(),
        Some(&Event::Finished {
            status: Status::Success,
            code: None
        })
    );
}

#[test]
fn a_cancel_is_a_result_and_a_resumed_call_finishes_the_install() {
    let f = Fixture::new("api-resume");
    let cancel = CancelToken::new();
    let stopper = cancel.clone();
    let mut progress = |e: &Event| {
        if let Event::Progress { message, .. } = e
            && message.contains("directory assets")
        {
            stopper.cancel();
        }
    };
    let out = install(&job(&f), &mut progress, &cancel).unwrap();
    let InstallOutput::Cancelled { result } = out else {
        panic!("not cancelled: {out:?}")
    };
    assert_eq!(result.status, Status::Cancelled);
    assert!(f.dest.join(JOURNAL_FILE).is_file(), "the journal is kept");

    let mut again = job(&f);
    again.resume = true;
    let out = install(&again, &mut NoProgress, &CancelToken::new()).unwrap();
    let InstallOutput::Done { result } = out else {
        panic!("not done: {out:?}")
    };
    assert!(result.resumed);
    assert!(result.assets.resumed > 0);
    assert!(!f.dest.join(JOURNAL_FILE).exists());
}

#[test]
fn a_token_cancelled_before_the_call_is_a_cancelled_result_that_ends_with_finished() {
    let f = Fixture::new("api-cancel-first");
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut events: Vec<Event> = Vec::new();
    let mut progress = |e: &Event| events.push(e.clone());
    let out = install(&job(&f), &mut progress, &cancel).unwrap();
    let InstallOutput::Cancelled { result } = out else {
        panic!("not cancelled: {out:?}")
    };
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!(result.exit_code, 4);
    assert_eq!(
        result.error.as_ref().map(|e| e.code),
        Some(ErrorCode::Cancelled)
    );
    let message = result
        .error
        .as_ref()
        .map(|e| e.message.as_str())
        .unwrap_or_default();
    assert!(
        !message.contains("resum"),
        "no journal was written, so no resume is offered: {message}"
    );
    assert!(
        !f.dest.join(JOURNAL_FILE).exists(),
        "no journal was written"
    );
    assert_eq!(
        events.last(),
        Some(&Event::Finished {
            status: Status::Cancelled,
            code: Some(ErrorCode::Cancelled)
        })
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Finished { .. }))
            .count(),
        1,
        "exactly one Finished, and it is last"
    );
    assert!(!f.dest.join("loose.txt").exists(), "nothing was written");
}

#[test]
fn a_missing_game_copy_is_a_typed_error_whose_copy_text_has_no_command_line() {
    let f = Fixture::new("api-no-game");
    let mut j = job(&f);
    j.fallout3_path = None;
    let err = install(&j, &mut NoProgress, &CancelToken::new()).unwrap_err();
    assert_eq!(err.code, ErrorCode::GamePathMissing);
    assert_eq!(err.feature.as_deref(), Some("fallout3"));
    assert_eq!(err.result.status, Status::Failed);
    let text = err.copy_text();
    assert!(text.contains("game_path_missing"), "{text}");
    assert!(
        text.contains("The package needs the fallout3 copy, and none was given"),
        "{text}"
    );
    assert!(!text.contains("--fo3"), "no command line option: {text}");
}

#[test]
fn a_cancel_set_after_the_install_finished_leaves_it_a_success() {
    let f = Fixture::new("api-cancel-late");
    let cancel = CancelToken::new();
    let late = cancel.clone();
    // The install is over when the last phase is reported; the caller's token is set before the call returns.
    let mut events: Vec<Event> = Vec::new();
    let mut progress = |e: &Event| {
        events.push(e.clone());
        if matches!(
            e,
            Event::Phase {
                phase: Phase::PostInstall
            }
        ) {
            late.cancel();
        }
    };
    let out = install(&job(&f), &mut progress, &cancel).unwrap();
    let InstallOutput::Done { result } = out else {
        panic!("not done: {out:?}")
    };
    assert_eq!(result.status, Status::Success);
    assert!(result.error.is_none());
    assert_eq!(result.exit_code, 0);
    assert!(f.dest.join("loose.txt").is_file());
    assert_eq!(
        events.last(),
        Some(&Event::Finished {
            status: Status::Success,
            code: None
        })
    );
}

#[test]
fn a_failure_for_another_reason_keeps_its_code_when_the_token_is_set_late() {
    // A check that fails: the install stops at the checks, and the caller's token is set before the call returns.
    let mut m = Fixture::manifest();
    m.check(json!({
        "Type": 0, "Loc": loc::GAME_DATA, "File": "loose.txt", "Checksums": "deadbeef",
    }));
    let f = Fixture::with_manifest("api-cancel-failed", &m);
    let cancel = CancelToken::new();
    let late = cancel.clone();
    let mut events: Vec<Event> = Vec::new();
    let mut progress = |e: &Event| {
        events.push(e.clone());
        if matches!(
            e,
            Event::Phase {
                phase: Phase::Checks
            }
        ) {
            late.cancel();
        }
    };
    let err = install(&job(&f), &mut progress, &cancel).unwrap_err();
    assert_eq!(err.code, ErrorCode::CheckFailed);
    assert_eq!(err.result.status, Status::Failed);
    assert_eq!(
        err.result.error.as_ref().map(|e| e.code),
        Some(ErrorCode::CheckFailed)
    );
    assert_eq!(
        events.last(),
        Some(&Event::Finished {
            status: Status::Failed,
            code: Some(ErrorCode::CheckFailed)
        })
    );
}

#[test]
fn a_cancel_before_any_work_is_cancelled_without_a_resume_when_nothing_was_written() {
    // A package with one asset: the caller's token is set at the preflight, before the journal or the destination is
    // written, so the run stops without a journal, and nothing offers a resume.
    let dir = ttwright_testkit::TempDir::new("api-cancel-journal");
    let mut m = ttwright_testkit::Manifest::new("Tale of Two Wastelands", "3.4.0");
    let game = ttwright_testkit::game_copy(&dir.join("Fallout 3"), "Fallout3.exe", &[]);
    let source = m.location("FO3 Data", 0, "%FO3DATA%");
    let dest = m.location("Destination", 0, "%DESTINATION%");
    m.variable(
        "FO3ROOT",
        4,
        Some("HKLM\\Software\\Synthetic\\Installed Path"),
    );
    m.asset(0, "", source, dest, "absent.txt", None);
    let mpi = dir.join("package/Synthetic.mpi");
    ttwright_testkit::write(&mpi, &ttwright_testkit::mpi_package(&m.build(), &[]));
    let destination = dir.join("out");
    let j = InstallJob {
        mpi_path: mpi,
        fallout3_path: Some(game),
        destination_path: destination.clone(),
        ..InstallJob::default()
    };

    let cancel = CancelToken::new();
    let stopper = cancel.clone();
    let mut progress = |e: &Event| {
        if matches!(
            e,
            Event::Phase {
                phase: Phase::Preflight
            }
        ) {
            stopper.cancel();
        }
    };
    let out = install(&j, &mut progress, &cancel).unwrap();
    let InstallOutput::Cancelled { result } = out else {
        panic!("not cancelled: {out:?}")
    };
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!(
        result.error.as_ref().map(|e| e.code),
        Some(ErrorCode::Cancelled)
    );
    assert_eq!(result.exit_code, 4);
    let message = result
        .error
        .as_ref()
        .map(|e| e.message.as_str())
        .unwrap_or_default();
    assert!(
        !message.contains("resum"),
        "nothing was written, so no resume is offered: {message}"
    );
    assert!(
        !destination.join(JOURNAL_FILE).exists(),
        "no journal was written"
    );
}

#[test]
fn a_dry_run_writes_nothing_and_says_so() {
    let f = Fixture::new("api-dry");
    let mut j = job(&f);
    j.dry_run = true;
    let InstallOutput::Done { result } = install(&j, &mut NoProgress, &CancelToken::new()).unwrap()
    else {
        panic!("not done")
    };
    assert!(result.dry_run);
    assert!(!f.dest.join("loose.txt").exists());
}

#[test]
fn the_job_round_trips_through_json_and_missing_fields_take_their_defaults() {
    let f = Fixture::new("api-json");
    let j = job(&f);
    let back: InstallJob = serde_json::from_str(&serde_json::to_string(&j).unwrap()).unwrap();
    assert_eq!(back, j);
    let minimal: InstallJob =
        serde_json::from_str(r#"{"mpi_path":"a.mpi","destination_path":"out"}"#).unwrap();
    assert!(!minimal.detect_missing && !minimal.resume && !minimal.dry_run);
}
