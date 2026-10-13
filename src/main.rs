//! SwapTray: a system tray app managing a single Linux swap file.
//!
//! The tray and serialized worker run in the main process; each size dialog
//! owns a separate main-thread native event loop in a child process.

mod autostart;
mod daemon;
mod dialog;
mod icon;
mod notify;
mod ops;
mod single_instance;
mod state;
mod tray;

use std::sync::mpsc;
use std::sync::Arc;

use ksni::blocking::TrayMethods as _;
use parking_lot::Mutex;

use ops::Op;
use tray::{JobRequest, SwapTray};

type TrayHandle = ksni::blocking::Handle<SwapTray>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None => run_tray(false),
        Some("--daemon") => match daemon::launch() {
            Ok(()) => 0,
            Err(e) => { eprintln!("swaptray: {e}"); 1 }
        },
        Some("--internal-daemon") => run_tray(true),
        Some("--internal-create-dialog") => dialog::run(),
        Some("--status") => {
            print_status();
            0
        }
        Some("--help" | "-h") => {
            print_help();
            0
        }
        Some("--version" | "-V") => {
            println!("swaptray {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some(other) => {
            eprintln!("swaptray: unknown option '{other}'");
            eprintln!("Try 'swaptray --help' for more information.");
            1
        }
    };
    std::process::exit(code);
}

fn print_help() {
    println!(
        "swaptray {} - system tray manager for a single Linux swap file

Usage:
  swaptray            Start the tray app (default)
  swaptray --daemon   Start the tray in the background and return
  swaptray --status   Print the current swap file state and exit
  swaptray --version  Print the version
  swaptray --help     Show this help

The managed swap file is /swapfile (override with SWAPTRAY_SWAPFILE).
Privileged operations run through pkexec and open the system
authentication dialog. \"Start at Login\" only starts the tray UI;
swap is never enabled automatically.",
        env!("CARGO_PKG_VERSION")
    );
}

fn print_status() {
    let snap = state::Snapshot::read();
    let path = state::swap_path();
    println!("SwapTray {}", env!("CARGO_PKG_VERSION"));
    println!("swap file : {}", path.display());
    println!(
        "exists    : {}",
        if snap.exists {
            format!("yes ({})", state::human(snap.size_bytes))
        } else {
            "no".to_string()
        }
    );
    println!(
        "active    : {}",
        if snap.active { "yes" } else { "no" }
    );
    println!(
        "autostart : {}",
        if autostart::enabled() {
            "enabled"
        } else {
            "disabled"
        }
    );
}

fn run_tray(detached: bool) -> i32 {
    let Some(_lock) = single_instance::acquire() else {
        eprintln!("[swaptray] another instance is already running");
        if detached {
            if let Err(e) = daemon::ready() { eprintln!("[swaptray] readiness failed: {e}"); return 1; }
        }
        return 0;
    };

    let (job_tx, job_rx) = mpsc::channel::<JobRequest>();

    let tray = SwapTray {
        snap: state::Snapshot::read(),
        autostart: autostart::enabled(),
        busy: None,
        job_tx: job_tx.clone(),
    };

    // Assume SNI availability: at login the desktop's tray may not be up yet,
    // and ksni re-registers when the watcher comes online.
    let handle = match tray.assume_sni_available(true).spawn() {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("[swaptray] cannot register the tray icon: {e:?}");
            eprintln!(
                "[swaptray] does this desktop provide a system tray \
                 (StatusNotifierWatcher / AppIndicator)?"
            );
            return 1;
        }
    };

    let slot: Arc<Mutex<Option<TrayHandle>>> = Arc::new(Mutex::new(Some(handle)));
    let worker_slot = slot.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("swaptray-worker".into())
        .spawn(move || worker(job_rx, worker_slot))
    {
        eprintln!("[swaptray] cannot start worker thread: {e}");
        return 1;
    }

    if detached {
        if let Err(e) = daemon::ready() { eprintln!("[swaptray] readiness failed: {e}"); return 1; }
    }
    loop {
        std::thread::park();
    }
}

/// Runs one privileged operation at a time, refreshes the tray, notifies.
fn worker(
    rx: mpsc::Receiver<JobRequest>,
    slot: Arc<Mutex<Option<TrayHandle>>>,
) {
    for job in rx {
        {
            let guard = slot.lock();
            if let Some(handle) = guard.as_ref() {
                handle.update(|tray| tray.busy = Some(job.op.label()));
            }
        }

        let result = ops::execute(&job.op);

        {
            let guard = slot.lock();
            if let Some(handle) = guard.as_ref() {
                handle.update(|tray| {
                    tray.busy = None;
                    tray.refresh();
                });
            }
        }

        match result {
            Ok(_) => {
                let title = job.op.success_title();
                let body = success_body(&job.op);
                notify::notify(title, &body);
            }
            Err(e) => {
                let text = e.to_string();
                eprintln!("[swaptray] {} failed: {text}", job.op.label());
                notify::notify("SwapTray", &format!("{} failed: {text}", job.op.label()));
            }
        }
    }
}

fn success_body(op: &Op) -> String {
    let path = state::swap_path();
    match op {
        Op::Create { .. } => {
            let size = std::fs::metadata(&path)
                .map(|m| state::human(m.len()))
                .unwrap_or_default();
            format!("{} ({size}). Turn it on from the tray menu.", path.display())
        }
        Op::TurnOn | Op::TurnOff | Op::Delete => path.display().to_string(),
    }
}
