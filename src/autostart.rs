//! "Start at Login": an autostart desktop entry in the user's config dir.
//! This only launches the tray UI; swap stays off until turned on manually.

use std::path::{Path, PathBuf};

use crate::ops;

const FILE_NAME: &str = "swaptray.desktop";

fn config_home() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return Some(p);
        }
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|h| PathBuf::from(h).join(".config"))
}

pub fn autostart_file() -> Option<PathBuf> {
    Some(config_home()?.join("autostart").join(FILE_NAME))
}

pub fn enabled() -> bool {
    autostart_file().is_some_and(|p| p.exists())
}

pub fn set_enabled(on: bool) -> Result<(), String> {
    let path = autostart_file().ok_or("HOME is not set")?;
    if on {
        let dir = path
            .parent()
            .ok_or_else(|| "invalid autostart path".to_string())?;
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let entry = desktop_entry(&exec_path());
        std::fs::write(&path, entry)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(())
    } else {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
        }
    }
}

/// Path written to `Exec=`. Prefers the invoked path (stable across mise
/// upgrades when started via a shim), falls back to the resolved executable.
fn exec_path() -> String {
    let argv0 = std::env::args().next().unwrap_or_default();
    if argv0.contains('/') {
        let p = Path::new(&argv0);
        if p.is_absolute() {
            return p.to_string_lossy().into_owned();
        }
        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(p).to_string_lossy().into_owned();
        }
        return argv0;
    }
    if !argv0.is_empty() {
        if let Some(found) = ops::find_bin(&argv0) {
            return found.to_string_lossy().into_owned();
        }
    }
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(argv0)
}

fn desktop_entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=SwapTray\n\
         Comment=Manage the swap file from the system tray\n\
         Exec={}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n\
         Categories=Utility;\n",
        quote_exec(exec)
    )
}

/// Quote an Exec value per the desktop entry specification.
fn quote_exec(s: &str) -> String {
    let needs_quotes = s.chars().any(|c| {
        matches!(
            c,
            ' ' | '\t' | '"' | '\\' | '>' | '<' | '~' | '|' | '&' | ';' | '$' | '*' | '(' | ')'
                | '`' | '\''
        )
    });
    if !needs_quotes {
        return s.to_string();
    }
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn exec_quoting_handles_spaces_and_shell_chars() {
        assert_eq!(quote_exec("/plain/path"), "/plain/path");
        assert_eq!(quote_exec("/with space/bin"), "\"/with space/bin\"");
        assert_eq!(
            quote_exec("/weird $dir/bin"),
            "\"/weird \\$dir/bin\""
        );
    }
}
