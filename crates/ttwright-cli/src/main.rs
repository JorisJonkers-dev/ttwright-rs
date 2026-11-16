// SPDX-License-Identifier: GPL-3.0-only
//! `ttwright`: install Tale of Two Wastelands and other MPI packages from the player's own game copies.
//!
//! For people it prints progress and a summary. For the Toolbox (`--json`) it prints one `ttwright-result`
//! document on stdout, `ttwright-events` lines on stderr (or into `--events`), never scans for game copies, and
//! exits 0 (success), 1 (failed), 2 (usage), 3 (unsupported feature or version) or 4 (cancelled).

mod commands;

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

use clap::{Parser, Subcommand};
use ttwright_contract::{
    DocumentKind, ErrorCode, Event, EventLine, Handshake, PackageInfo, ResultDoc, Status,
    schema_json, to_line,
};
use ttwright_core::app::{CancelToken, InstallEvent, InstallOutcome, InstallRequest, run_install};
use ttwright_core::services::Logger;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "ttwright", version)]
#[command(about = "Install Tale of Two Wastelands and other MPI packages from your own game copies", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install from an MPI package
    Install(InstallArgs),

    /// Print the Contracts this build speaks and what it supports
    Contracts {
        /// Print JSON (the only format)
        #[arg(long)]
        json: bool,
    },

    /// Print the JSON Schema of a document: result, events or contracts
    Schema {
        /// result, events or contracts
        kind: String,
        /// The schema version
        #[arg(long, default_value_t = 1)]
        schema_version: u32,
    },

    /// Extract an MPI package to a directory
    Extract {
        /// Path to MPI file
        #[arg(short, long)]
        mpi: PathBuf,
        /// Output directory
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Inspect an MPI package: its title, version and whether this build supports its features
    Inspect {
        /// Path to MPI file or extracted directory
        #[arg(short, long)]
        mpi: PathBuf,
        /// Show detailed asset information
        #[arg(long, default_value = "false")]
        verbose: bool,
    },

    /// Verify game installations
    Verify {
        /// Fallout 3 installation directory
        #[arg(long)]
        fo3: Option<PathBuf>,
        /// Fallout New Vegas installation directory
        #[arg(long)]
        fnv: Option<PathBuf>,
        /// Oblivion installation directory
        #[arg(long)]
        oblivion: Option<PathBuf>,
    },

    /// Auto-detect supported game installations
    Detect,

    /// List recent installation logs
    Logs {
        /// Number of recent logs to show
        #[arg(short, long, default_value = "10")]
        count: usize,
    },
}

#[derive(clap::Args, Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
struct InstallArgs {
    /// Path to MPI file or extracted directory
    #[arg(short, long)]
    mpi: PathBuf,
    /// Fallout 3 installation directory
    #[arg(long)]
    fo3: Option<PathBuf>,
    /// Fallout New Vegas installation directory
    #[arg(long)]
    fnv: Option<PathBuf>,
    /// Oblivion installation directory
    #[arg(long)]
    oblivion: Option<PathBuf>,
    /// Destination directory for installation output
    #[arg(short, long)]
    dest: PathBuf,
    /// Read and check everything, write nothing
    #[arg(long)]
    dry_run: bool,
    /// Continue an interrupted install from the journal in the destination
    #[arg(long)]
    resume: bool,
    /// Print the result document on stdout and events on stderr; never scan for game copies
    #[arg(long)]
    json: bool,
    /// Write the events into this file instead of stderr (with --json)
    #[arg(long)]
    events: Option<PathBuf>,
    /// Look for game copies that were not given (the default without --json)
    #[arg(long, conflicts_with = "no_detect")]
    detect: bool,
    /// Never look for game copies: use only the ones given
    #[arg(long)]
    no_detect: bool,
    /// Install a TTW version this build does not list as supported
    #[arg(long)]
    allow_unsupported_version: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(e) => {
            let usage = !matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            if usage && args.iter().any(|a| a == "--json") {
                let mut doc = ResultDoc::new(VERSION);
                doc.fail(ErrorCode::Usage, e.to_string().trim().to_owned(), None);
                println!("{}", to_line(&doc));
                return ExitCode::from(2);
            }
            let _ = e.print();
            return ExitCode::from(if usage { 2 } else { 0 });
        }
    };

    let outcome = match cli.command {
        Commands::Install(args) => return install(&args),
        Commands::Contracts { json } => {
            if !json {
                eprintln!("contracts takes --json");
                return ExitCode::from(2);
            }
            println!("{}", to_line(&Handshake::current(VERSION)));
            Ok(())
        }
        Commands::Schema {
            kind,
            schema_version,
        } => {
            let Some(kind) = DocumentKind::parse(&kind) else {
                eprintln!("unknown document {kind:?}: use result, events or contracts");
                return ExitCode::from(2);
            };
            match schema_json(kind, schema_version) {
                Ok(text) => {
                    print!("{text}");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(2);
                }
            }
        }
        Commands::Extract { mpi, output } => commands::run_extract(&mpi, &output),
        Commands::Inspect { mpi, verbose } => commands::run_inspect(&mpi, verbose),
        Commands::Verify { fo3, fnv, oblivion } => {
            commands::run_verify(fo3.as_deref(), fnv.as_deref(), oblivion.as_deref())
        }
        Commands::Detect => commands::run_detect(),
        Commands::Logs { count } => commands::run_logs(count),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Where events go in JSON mode.
enum Events {
    Stderr,
    File(Mutex<File>),
}

impl Events {
    fn send(&self, event: Event) {
        let line = to_line(&EventLine::new(event)) + "\n";
        match self {
            Self::Stderr => {
                let _ = std::io::stderr().lock().write_all(line.as_bytes());
            }
            Self::File(file) => {
                if let Ok(mut f) = file.lock() {
                    let _ = f.write_all(line.as_bytes());
                }
            }
        }
    }
}

fn install(args: &InstallArgs) -> ExitCode {
    let cancel = CancelToken::new();
    {
        let cancel = cancel.clone();
        // A second interrupt while the first is being handled ends the process at once.
        let _ = ctrlc::set_handler(move || {
            if cancel.is_cancelled() {
                std::process::exit(130);
            }
            cancel.cancel();
        });
    }

    let log_label = args
        .mpi
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Installation");
    let logger = Logger::init_with_console(log_label, !args.json).ok();

    let events = if args.json {
        match &args.events {
            None => Some(Events::Stderr),
            Some(path) => match File::create(path) {
                Ok(f) => Some(Events::File(Mutex::new(f))),
                Err(e) => {
                    let mut doc = ResultDoc::new(VERSION);
                    doc.fail(
                        ErrorCode::Usage,
                        format!("cannot create the events file {}: {e}", path.display()),
                        None,
                    );
                    println!("{}", to_line(&doc));
                    return ExitCode::from(2);
                }
            },
        }
    } else {
        None
    };

    let request = InstallRequest {
        mpi_path: args.mpi.clone(),
        fallout3_path: args.fo3.clone(),
        falloutnv_path: args.fnv.clone(),
        oblivion_path: args.oblivion.clone(),
        destination_path: args.dest.clone(),
        dry_run: args.dry_run,
        detect_missing: !args.no_detect && (args.detect || !args.json),
        resume: args.resume,
        allow_unsupported_version: args.allow_unsupported_version,
        cancel,
    };

    let outcome = run_install(request, |event| match event {
        InstallEvent::Log(message) => tracing::info!("{}", message),
        InstallEvent::Event(event) => {
            if let Event::Warning { message } = &event {
                tracing::warn!("{}", message);
            }
            if let Event::Progress {
                done, of, message, ..
            } = &event
                && (done % 1000 == 0 || done == of)
            {
                tracing::info!("Progress: {done}/{of} {message}");
            }
            if let Some(events) = &events {
                events.send(event);
            }
        }
    });

    let doc = result_document(args, &outcome, logger.as_ref());
    if let Some(events) = &events {
        events.send(Event::Finished {
            status: doc.status,
            code: doc.error.as_ref().map(|e| e.code),
        });
    }
    if args.json {
        println!("{}", to_line(&doc));
    } else {
        print_summary(&doc);
    }
    ExitCode::from(doc.exit_code)
}

fn result_document(
    args: &InstallArgs,
    outcome: &InstallOutcome,
    logger: Option<&Logger>,
) -> ResultDoc {
    let report = &outcome.report;
    let mut doc = ResultDoc::new(VERSION);
    doc.package = report.package_name.as_ref().map(|title| PackageInfo {
        title: title.clone(),
        version: report.package_version.clone().unwrap_or_default(),
        supported: report.package_supported,
    });
    doc.output_dir = Some(args.dest.display().to_string());
    doc.checks_passed = report.checks_passed;
    doc.assets = report.assets;
    doc.archives = report.archives;
    doc.post_install = report.post_install;
    doc.resumed = report.resumed;
    doc.dry_run = args.dry_run;
    doc.elapsed_ms = u64::try_from(report.elapsed.as_millis()).unwrap_or(u64::MAX);
    doc.log_file = logger.map(|l| l.log_path().display().to_string());
    for message in &report.messages {
        doc.note(message.clone());
    }
    if let Some(error) = &outcome.error {
        doc.fail(error.code, error.message.clone(), error.feature.clone());
    }
    doc
}

fn print_summary(doc: &ResultDoc) {
    if doc.status == Status::Success {
        println!("\n=== Installation Complete ===");
    } else if let Some(error) = &doc.error {
        println!("\n=== Installation {:?} ===", doc.status);
        println!("Error [{}]: {}", error.code, error.message);
        if let Some(feature) = &error.feature {
            println!("Missing: {feature}");
        }
        for message in &doc.messages {
            println!("  {message}");
        }
    }
    let seconds = doc.elapsed_ms / 1000;
    println!(
        "Assets: {} done, {} failed, {} kept from the earlier run, of {}",
        doc.assets.done, doc.assets.failed, doc.assets.resumed, doc.assets.total
    );
    println!("Total time: {}m {}s", seconds / 60, seconds % 60);
    if let Some(log) = &doc.log_file {
        println!("Log saved to: {log}");
    }
}
