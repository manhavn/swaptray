//! Privileged operations: shell scripts executed through `pkexec`, which
//! triggers the system authentication dialog (polkit agent).
//!
//! The script is handed to `/bin/sh` on **stdin**, not with `-c`: the dialog
//! shows pkexec's command line, and pkexec truncates it (first 38 bytes,
//! `" ... "`, last 37) when it exceeds 80, which turns a full script into
//! unreadable fragments. `/bin/sh` alone keeps that message short.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::state;

const SAFE_PATH: &str =
    "export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Create { bytes: u64 },
    TurnOn,
    TurnOff,
    Delete,
}

impl Op {
    /// Short label used in the tray "Working: ..." line.
    pub fn label(&self) -> &'static str {
        match self {
            Op::Create { .. } => "creating swap file",
            Op::TurnOn => "turning on swap",
            Op::TurnOff => "turning off swap",
            Op::Delete => "deleting swap file",
        }
    }

    /// Notification title on success.
    pub fn success_title(&self) -> &'static str {
        match self {
            Op::Create { .. } => "Swap file created",
            Op::TurnOn => "Swap turned on",
            Op::TurnOff => "Swap turned off",
            Op::Delete => "Swap file deleted",
        }
    }

    pub fn script(&self, path: &Path) -> String {
        match *self {
            Op::Create { bytes } => script_create(path, bytes),
            Op::TurnOn => script_on(path),
            Op::TurnOff => script_off(path),
            Op::Delete => script_delete(path),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpError {
    /// User dismissed the authentication dialog.
    Cancelled,
    /// Authentication failed or the action was not authorized.
    NotAuthorized,
    /// A required program is not installed.
    Missing(&'static str),
    /// The operation itself failed; message is meant for the user.
    Failed(String),
    /// Failed to spawn pkexec.
    Spawn(String),
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpError::Cancelled => write!(f, "Authentication dialog was dismissed."),
            OpError::NotAuthorized => write!(f, "Not authorized to run this operation."),
            OpError::Missing(tool) => {
                write!(f, "Required program not found: {tool}. Is it installed?")
            }
            OpError::Failed(msg) => write!(f, "{msg}"),
            OpError::Spawn(msg) => write!(f, "Failed to launch pkexec: {msg}"),
        }
    }
}

/// True when `SWAPTRAY_DRY_RUN` is set: print the command instead of running it.
pub fn dry_run() -> bool {
    matches!(
        std::env::var("SWAPTRAY_DRY_RUN").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// Run a privileged script via pkexec. Returns stdout on success.
pub fn run(script: &str) -> Result<String, OpError> {
    if dry_run() {
        eprintln!("[swaptray][dry-run] {script}");
        return Ok(format!("[dry-run] script:\n{script}"));
    }

    let pkexec = find_bin("pkexec").ok_or(OpError::Missing("pkexec (polkit)"))?;
    let sh = find_sh().ok_or(OpError::Missing("/bin/sh"))?;

    // pkexec execs the program with our file descriptors, so the script can go
    // on the child's stdin. Authentication dialogs read the password from the
    // terminal, never from stdin.
    let mut child = Command::new(&pkexec)
        .arg(&sh)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| OpError::Spawn(e.to_string()))?;

    let mut pipe = child
        .stdin
        .take()
        .ok_or_else(|| OpError::Spawn("stdin was not piped".to_string()))?;
    if let Err(e) = pipe.write_all(script.as_bytes()) {
        // pkexec is already gone (dialog dismissed, not authorized) and the
        // exit status below says why; anything else would leave a partial
        // script for a root shell to run.
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            let _ = child.kill();
            let _ = child.wait();
            return Err(OpError::Spawn(format!("cannot send script: {e}")));
        }
    }
    // Closing the pipe is what tells `/bin/sh` the script ended.
    drop(pipe);

    let out = child
        .wait_with_output()
        .map_err(|e| OpError::Spawn(e.to_string()))?;

    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }

    // pkexec exit codes: 126 = dialog dismissed, 127 = not authorized.
    match out.status.code() {
        Some(126) => return Err(OpError::Cancelled),
        Some(127) => return Err(OpError::NotAuthorized),
        _ => {}
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(OpError::Failed(extract_error(
        &stderr,
        out.status.code().unwrap_or(-1),
    )))
}

/// Our scripts prefix user-facing failures with "error: "; fall back to the
/// last non-empty stderr line, then to the exit code.
fn extract_error(stderr: &str, code: i32) -> String {
    for line in stderr.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("error: ") {
            return rest.to_string();
        }
        // Prefer our own messages, otherwise keep scanning; remember the last line.
        if line.starts_with("swaptray:") {
            return line.trim_start_matches("swaptray: ").to_string();
        }
    }
    stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .next_back()
        .map(str::to_string)
        .unwrap_or_else(|| format!("Command failed with exit code {code}"))
}

/// Single-quote a string for POSIX sh.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn base(path: &Path) -> String {
    format!("{SAFE_PATH}\nP={}", sh_quote(&path.to_string_lossy()))
}

/// Create + format the swap file. Cleans up on any failure.
/// Uses `fallocate` (fast); falls back to `dd` when the filesystem does not
/// support preallocation (e.g. busybox environments, some network FS).
pub fn script_create(path: &Path, bytes: u64) -> String {
    let mb = bytes.div_ceil(1024 * 1024);
    format!(
        r#"{base}
umask 077
if [ -e "$P" ] || [ -L "$P" ]; then echo "error: $P already exists" >&2; exit 3; fi
if ! (set -C; : > "$P"); then echo "error: cannot exclusively create $P" >&2; exit 4; fi
chattr +C "$P" 2>/dev/null || true
if ! fallocate -l {bytes} "$P" 2>/dev/null; then
  rm -f "$P"
  if ! dd if=/dev/zero of="$P" bs=1048576 count={mb}; then
    rm -f "$P"
    echo "error: not enough free space on the target filesystem" >&2
    exit 4
  fi
fi
if ! chmod 600 "$P"; then rm -f "$P"; echo "error: chmod 600 failed" >&2; exit 4; fi
if ! mkswap "$P" >/dev/null; then rm -f "$P"; echo "error: mkswap failed (filesystem may not support swap files)" >&2; exit 4; fi
echo "created $P"
"#,
        base = base(path)
    )
}

pub fn script_on(path: &Path) -> String {
    format!(
        r#"{base}
if [ ! -f "$P" ]; then echo "error: swap file not found: $P" >&2; exit 3; fi
swapon "$P" || {{ echo "error: swapon failed" >&2; exit 4; }}
echo "enabled $P"
"#,
        base = base(path)
    )
}

pub fn script_off(path: &Path) -> String {
    format!(
        r#"{base}
if [ ! -f "$P" ]; then echo "error: swap file not found: $P" >&2; exit 3; fi
swapoff "$P" || {{ echo "error: swapoff failed (is memory tight? try closing applications)" >&2; exit 4; }}
echo "disabled $P"
"#,
        base = base(path)
    )
}

/// Turn swap off first if it is active, then remove the file.
pub fn script_delete(path: &Path) -> String {
    format!(
        r#"{base}
if [ -r /proc/swaps ] && awk -v p="$P" 'NR > 1 && $1 == p {{ found = 1 }} END {{ exit !found }}' /proc/swaps; then
  swapoff "$P" || {{ echo "error: cannot turn off swap before deleting" >&2; exit 4; }}
fi
if [ ! -e "$P" ]; then echo "error: swap file not found: $P" >&2; exit 3; fi
rm -f "$P" || {{ echo "error: failed to remove $P" >&2; exit 4; }}
echo "removed $P"
"#,
        base = base(path)
    )
}

/// Execute an operation against the managed swap path.
pub fn execute(op: &Op) -> Result<String, OpError> {
    let path = state::swap_path();
    run(&op.script(&path))
}

/// Find an executable in PATH, with common directories as fallback.
pub fn find_bin(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(
        ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
            .iter()
            .map(PathBuf::from),
    );
    dirs.into_iter()
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

fn find_sh() -> Option<PathBuf> {
    ["/bin/sh", "/usr/bin/sh"]
        .iter()
        .map(PathBuf::from)
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn quote_handles_spaces_and_quotes() {
        assert_eq!(sh_quote("/swap file"), "'/swap file'");
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn extract_error_prefers_our_error_line() {
        let stderr = "some noise\nerror: mkswap failed\ntrailing\n";
        assert_eq!(extract_error(stderr, 4), "mkswap failed");
        assert_eq!(extract_error("", 4), "Command failed with exit code 4");
    }
}
