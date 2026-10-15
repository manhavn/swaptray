//! "Start at Login": an autostart desktop entry in the user's config dir.
//! This only launches the tray UI; swap stays off until turned on manually.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::ops;

const FILE_NAME: &str = "swaptray.desktop";

/// mise's alias for the newest installed version of a tool.
const VERSION_ALIAS: &str = "latest";

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

/// Path written to `Exec=`. Version-independent whenever the binary comes
/// from a mise install (see [`stable_path`]).
fn exec_path() -> String {
    entry_exec(&invoked_path())
}

/// `Exec=` value for a binary invoked as `invoked`.
fn entry_exec(invoked: &str) -> String {
    match stable_path(Path::new(invoked)) {
        Some(stable) => stable.to_string_lossy().into_owned(),
        None => invoked.to_string(),
    }
}

/// A path to the same binary that does not name an installed version, for
/// binaries living in a mise install tree.
///
/// mise keeps versions at `<data>/mise/installs/<tool>/<version>/` and execs
/// the versioned binary, so `argv[0]` (and `current_exe`) pins one version:
/// an entry written from it goes stale on the next upgrade and has to be
/// rewritten by toggling "Start at Login" off and on. mise offers two
/// indirections instead — `<data>/mise/shims/<bin>`, which runs mise to
/// resolve the version the config selects, and the `<tool>/latest` alias mise
/// itself hands out. Either keeps pointing at the current version.
fn stable_path(exe: &Path) -> Option<PathBuf> {
    let bin = exe.file_name()?;
    let tool_dir = exe.parent()?.parent()?;
    if tool_dir.parent()?.file_name()? != "installs" {
        return None;
    }
    let mise_dir = tool_dir.parent()?.parent()?;
    if mise_dir.file_name()? != "mise" {
        return None;
    }
    let shim = mise_dir.join("shims").join(bin);
    if shim.is_file() {
        return Some(shim);
    }
    let alias = tool_dir.join(VERSION_ALIAS).join(bin);
    alias.is_file().then_some(alias)
}

/// Point an entry written by an older build at the version-independent path,
/// so an upgrade no longer needs the menu item to be toggled off and on.
/// Entries without a mise install path in `Exec` are left alone.
pub fn refresh_exec_path() {
    if let Some(path) = autostart_file() {
        refresh_file(&path);
    }
}

fn refresh_file(path: &Path) {
    let Ok(entry) = std::fs::read_to_string(path) else {
        return;
    };
    let Some((range, command)) = exec_command(&entry) else {
        return;
    };
    let Some(stable) = stable_path(Path::new(&command)) else {
        return;
    };
    let stable = stable.to_string_lossy().into_owned();
    if stable == command {
        return;
    }
    let updated = format!(
        "{}{}{}",
        &entry[..range.start],
        quote_exec(&stable),
        &entry[range.end..]
    );
    if let Err(e) = std::fs::write(path, updated) {
        eprintln!("[swaptray] cannot update {}: {e}", path.display());
    }
}

/// The command of the `Exec=` line: its byte range in `entry` and its value
/// with the desktop entry quoting removed.
fn exec_command(entry: &str) -> Option<(Range<usize>, String)> {
    let mut line_start = 0;
    for line in entry.split('\n') {
        let line_end = line_start + line.len();
        if let Some(value) = line.strip_prefix("Exec=") {
            return split_command(value, line_end - value.len());
        }
        line_start = line_end + 1;
    }
    None
}

/// Split the command from the arguments of an `Exec` value, unquoting it.
fn split_command(value: &str, offset: usize) -> Option<(Range<usize>, String)> {
    let Some(rest) = value.strip_prefix('"') else {
        let end = value.find(char::is_whitespace).unwrap_or(value.len());
        return Some((offset..offset + end, value[..end].to_string()));
    };
    let mut command = String::new();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => command.push(chars.next()?.1),
            '"' => return Some((offset..offset + i + 2, command)),
            _ => command.push(c),
        }
    }
    None
}

/// Path we were invoked by, as-is: `argv[0]` when it names a location, a PATH
/// lookup for a bare name, otherwise the running executable.
fn invoked_path() -> String {
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
         Exec={} --daemon\n\
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

    /// Throwaway mise-like tree, removed on drop.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "swaptray-autostart-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("create temp tree");
            Self(root)
        }

        fn file(&self, rel: &str) -> PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().expect("temp file parent"))
                .expect("create temp dirs");
            std::fs::write(&path, b"#!/bin/sh\n").expect("create temp file");
            path
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn stable_path_prefers_the_mise_shim() {
        let tree = Tree::new("shim");
        let versioned = tree.file("mise/installs/github-x-tool/0.1.3/swaptray");
        let alias = tree.file("mise/installs/github-x-tool/latest/swaptray");
        let shim = tree.file("mise/shims/swaptray");

        assert_eq!(stable_path(&versioned).as_deref(), Some(shim.as_path()));
        assert_eq!(stable_path(&alias).as_deref(), Some(shim.as_path()));
        // A shim is already version-independent, so it is left as it is.
        assert_eq!(stable_path(&shim), None);
    }

    #[test]
    fn stable_path_falls_back_to_the_latest_alias() {
        let tree = Tree::new("alias");
        let versioned = tree.file("mise/installs/github-x-tool/0.1.3/swaptray");
        let alias = tree.file("mise/installs/github-x-tool/latest/swaptray");

        assert_eq!(stable_path(&versioned).as_deref(), Some(alias.as_path()));
        assert_eq!(stable_path(&alias).as_deref(), Some(alias.as_path()));
    }

    #[test]
    fn stable_path_ignores_other_trees() {
        let tree = Tree::new("outside");
        let foreign = tree.file("share/app/installs/tool/0.1.3/swaptray");
        tree.file("share/app/shims/swaptray");

        assert_eq!(stable_path(&foreign), None);
        assert_eq!(stable_path(Path::new("/usr/local/bin/swaptray")), None);
    }

    #[test]
    fn entry_exec_replaces_mise_install_paths() {
        let tree = Tree::new("entry");
        let versioned = tree.file("mise/installs/github-x-tool/0.1.3/swaptray");
        let shim = tree.file("mise/shims/swaptray");

        assert_eq!(
            entry_exec(&versioned.display().to_string()),
            shim.display().to_string()
        );
        assert_eq!(
            entry_exec("/usr/local/bin/swaptray"),
            "/usr/local/bin/swaptray"
        );
    }

    #[test]
    fn exec_command_unquotes_reads_the_command() {
        let text = "[Desktop Entry]\nExec=/x/swaptray --daemon\n";
        let (range, command) = exec_command(text).expect("Exec line");
        assert_eq!(command, "/x/swaptray");
        assert_eq!(&text[range], "/x/swaptray");

        let text = "Exec=\"/with space/swaptray\" --daemon\n";
        let (range, command) = exec_command(text).expect("Exec line");
        assert_eq!(command, "/with space/swaptray");
        assert_eq!(&text[range], "\"/with space/swaptray\"");

        let text = "Exec=\"/weird \\$dir/swaptray\"\n";
        let (_, command) = exec_command(text).expect("Exec line");
        assert_eq!(command, "/weird $dir/swaptray");

        assert_eq!(exec_command("Name=SwapTray\n"), None);
    }

    #[test]
    fn refresh_file_rewrites_versioned_mise_entry() {
        let tree = Tree::new("refresh");
        let versioned = tree.file("mise/installs/github-x-tool/0.1.3/swaptray");
        let shim = tree.file("mise/shims/swaptray");
        let entry = tree.0.join("swaptray.desktop");
        std::fs::write(
            &entry,
            format!("[Desktop Entry]\nExec={} --daemon\nNoDisplay=true\n", versioned.display()),
        )
        .expect("write entry");

        refresh_file(&entry);

        let healed = std::fs::read_to_string(&entry).expect("read entry");
        assert!(
            healed.contains(&format!("Exec={} --daemon", shim.display())),
            "{healed}"
        );
        assert!(healed.contains("NoDisplay=true"), "{healed}");

        refresh_file(&entry);
        assert_eq!(std::fs::read_to_string(&entry).expect("read entry"), healed);
    }

    #[test]
    fn refresh_file_keeps_arguments_after_a_quoted_command() {
        // A space in the path forces the quoting path of `quote_exec`.
        let tree = Tree::new("with space");
        let versioned = tree.file("mise/installs/github-x-tool/0.1.3/swaptray");
        let shim = tree.file("mise/shims/swaptray");
        let entry = tree.0.join("swaptray.desktop");
        std::fs::write(
            &entry,
            format!("Exec={} --daemon\n", quote_exec(&versioned.display().to_string())),
        )
        .expect("write entry");

        refresh_file(&entry);

        assert_eq!(
            std::fs::read_to_string(&entry).expect("read entry"),
            format!("Exec={} --daemon\n", quote_exec(&shim.display().to_string()))
        );
    }

    #[test]
    fn refresh_file_leaves_foreign_entry_alone() {
        let tree = Tree::new("foreign");
        let entry = tree.0.join("swaptray.desktop");
        let text = "[Desktop Entry]\nExec=/usr/local/bin/swaptray --daemon\n";
        std::fs::write(&entry, text).expect("write entry");

        refresh_file(&entry);

        assert_eq!(std::fs::read_to_string(&entry).expect("read entry"), text);
    }

    #[test]
    fn refresh_file_ignores_missing_entry() {
        let tree = Tree::new("missing");
        refresh_file(&tree.0.join("absent.desktop"));
    }
}
