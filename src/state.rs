//! Reading the current state of the managed swap file.

use std::path::{Path, PathBuf};

/// Default location of the single swap file managed by SwapTray.
pub const DEFAULT_SWAP_PATH: &str = "/swapfile";

const GIB: u64 = 1024 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;

/// The swap file path. `SWAPTRAY_SWAPFILE` overrides it (used for testing).
pub fn swap_path() -> PathBuf {
    std::env::var_os("SWAPTRAY_SWAPFILE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SWAP_PATH))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub exists: bool,
    pub size_bytes: u64,
    pub active: bool,
}

impl Snapshot {
    pub fn read() -> Self {
        Self::read_from(&swap_path())
    }

    pub fn read_from(path: &Path) -> Self {
        let meta = std::fs::metadata(path).ok();
        let exists = meta.as_ref().is_some_and(|m| m.is_file());
        let size_bytes = meta.map(|m| m.len()).unwrap_or(0);
        let active = exists && proc_swaps_contains(path);
        Self {
            exists,
            size_bytes,
            active,
        }
    }
}

/// Whether `path` is listed as an active swap device in `/proc/swaps`.
fn proc_swaps_contains(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string("/proc/swaps") else {
        return false;
    };
    swaps_contains(&content, path)
}

/// Parse `/proc/swaps` content (header line + one device per line).
/// The kernel escapes whitespace in filenames as `\040`, `\011`, `\012`, `\134`.
fn swaps_contains(content: &str, path: &Path) -> bool {
    let want = path.to_string_lossy();
    content
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .any(|field| unescape_proc(field) == want)
}

fn unescape_proc(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    s.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

/// Human readable size, e.g. `4.0 GiB`, `512 MiB`.
pub fn human(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} B")
    }
}

/// Parse the size entered in the dialog, returning bytes.
/// Accepts integers and decimals in GiB, e.g. `4`, `4.5`.
pub fn parse_gb(input: &str) -> Result<u64, String> {
    let text = input.trim();
    let value: f64 = text
        .parse()
        .map_err(|_| format!("\"{text}\" is not a number. Enter a size in GiB, e.g. 4 or 4.5."))?;
    if !value.is_finite() || value <= 0.0 {
        return Err("Size must be greater than 0.".to_string());
    }
    if value < 0.1 {
        return Err("Minimum size is 0.1 GiB.".to_string());
    }
    if value > 4096.0 {
        return Err("Maximum size is 4096 GiB.".to_string());
    }
    Ok((value * GIB as f64).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_parsing_skips_header_and_matches_path() {
        let content = "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
                       /swapfile                               file\t\t4194300\t\t0\t\t-2\n";
        assert!(swaps_contains(content, Path::new("/swapfile")));
        assert!(!swaps_contains(content, Path::new("/swap")));
        assert!(!swaps_contains(content, Path::new("/swapfile.bak")));
    }

    #[test]
    fn swaps_parsing_unescapes_kernel_escaping() {
        let content = "Filename Type Size Used Priority\n/my\\040swap file 1 0 -2\n";
        assert!(swaps_contains(content, Path::new("/my swap")));
    }

    #[test]
    fn human_formats_sizes() {
        assert_eq!(human(4 * GIB), "4.0 GiB");
        assert_eq!(human(4 * GIB + GIB / 2), "4.5 GiB");
        assert_eq!(human(512 * MIB), "512 MiB");
        assert_eq!(human(100), "100 B");
    }

    #[test]
    fn parse_gb_accepts_valid_input() {
        assert_eq!(parse_gb("4").unwrap(), 4 * GIB);
        assert_eq!(parse_gb(" 4.5 ").unwrap(), 4 * GIB + GIB / 2);
        assert_eq!(parse_gb("0.1").unwrap(), (GIB as f64 / 10.0).round() as u64);
        assert_eq!(parse_gb("4096").unwrap(), 4096 * GIB);
    }

    #[test]
    fn parse_gb_rejects_invalid_input() {
        for input in ["", "abc", "-4", "0", "0.05", "4097", "NaN", "inf", "4gb"] {
            assert!(parse_gb(input).is_err(), "should reject {input:?}");
        }
    }

    #[test]
    fn read_from_reports_missing_file() {
        let snap = Snapshot::read_from(Path::new("/nonexistent/swaptray-test"));
        assert!(!snap.exists);
        assert!(!snap.active);
        assert_eq!(snap.size_bytes, 0);
    }
}
