//! Re-exec before starting threads, detach the child, and confirm tray readiness.
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const READY: &str = "swaptray-daemon-ready";

fn log_path() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from)
        .filter(|p| p.is_absolute()).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .ok_or_else(|| std::io::Error::other("HOME or XDG_STATE_HOME is required"))?;
    let dir = base.join("swaptray");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("swaptray.log"))
}

pub fn launch() -> Result<(), String> {
    let log = log_path().map_err(|e| e.to_string())?;
    let stderr = OpenOptions::new().create(true).append(true).open(&log).map_err(|e| e.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command.arg("--internal-daemon").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(stderr);
    // Only the fresh, single-threaded child executes this async-signal-safe syscall.
    unsafe { command.pre_exec(|| {
        if libc::setsid() == -1 { return Err(std::io::Error::last_os_error()); }
        Ok(())
    }); }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().expect("piped daemon stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(line)) if line.trim_end() == READY => Ok(()),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("Daemon failed to start; see {}", log.display()))
        }
    }
}

pub fn ready() -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{READY}")?;
    stdout.flush()?;
    // No terminal or stale readiness pipe remains attached to the daemon.
    use std::os::fd::AsRawFd;
    let null = OpenOptions::new().write(true).open("/dev/null")?;
    if unsafe { libc::dup2(null.as_raw_fd(), libc::STDOUT_FILENO) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
