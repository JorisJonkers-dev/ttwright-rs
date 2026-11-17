// SPDX-License-Identifier: GPL-3.0-only
//! [`ttwright_core::api::install`] called in process: the result, progress, cancel and resume, and the errors.

use ttwright_contract::{ErrorCode, Event, Phase, Status};
use ttwright_core::api::{CancelToken, InstallJob, InstallOutput, NoProgress, install};
use ttwright_core::app::JOURNAL_FILE;
use ttwright_testkit::Fixture;

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
    assert!(!text.contains("--"), "{text}");
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
