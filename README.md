# SwapTray

A Linux system tray app (Rust) that manages **one** swap file: create it,
turn swap on/off, delete it. Privileged steps open the system authentication
dialog (polkit/`pkexec`) — no passwords typed into the app, no sudo in a
terminal.

- **Single swap file** (`/swapfile`) — no lists, no configs to juggle
- **Tray logo + menu** (freedesktop StatusNotifierItem, works on
  GNOME/KDE/Cinnamon/MATE/XFCE and other SNI hosts)
- **Create Swap File…** opens a small dialog to enter the size in GiB
- **Turn On / Turn Off / Delete** straight from the menu
- **Start at Login** — starts the tray UI at login only; swap is *never*
  enabled automatically (you flip it on from the tray when you want it)
- Pure Rust: no GTK/Qt build dependencies, builds for `amd64` and `aarch64`

## Requirements

Runtime, per distribution:

- A desktop with a system tray (StatusNotifierHost). Ubuntu and most
  derivatives work out of the box; on upstream GNOME enable the
  *AppIndicator* extension
- `pkexec` (polkit) — for the system authentication popup
- `swapon`/`swapoff`/`mkswap` (util-linux), `fallocate` (util-linux), `awk`
- A notification daemon (optional; results are logged to stderr without one)

## Install (mise)

```sh
mise use -g github:manhavn/swaptray
```

mise picks the right asset for your OS, architecture and libc (glibc or musl).

Without mise: download `swaptray-v*-<target>.tar.gz` from the GitHub
release, extract it, and put `swaptray` on your `PATH`.

## Usage

Run `swaptray` (or enable **Start at Login** from the tray menu, then it
starts with your session).

Menu:

| Item | Effect |
| --- | --- |
| status line | `Swap: ON/OFF — size (path)` — refreshed every time the menu opens |
| Create Swap File... | dialog: enter GiB, creates + formats the file via pkexec |
| Turn On Swap | `swapon` (authentication popup) |
| Turn Off Swap | `swapoff` (authentication popup) |
| Delete Swap File | turns swap off if needed, then removes the file |
| Start at Login | writes/removes `~/.config/autostart/swaptray.desktop` (version-independent `Exec`) |
| Quit | exits |

CLI:

```
swaptray            start the tray app
swaptray --daemon   start the tray in the background and return
swaptray --status   print current state and exit
swaptray --version  print version
swaptray --help     usage
```

`--daemon` detaches from the terminal, preserves the desktop session
environment, and returns only after tray startup succeeds. Logs are appended
to `$XDG_STATE_HOME/swaptray/swaptray.log` (default:
`~/.local/state/swaptray/swaptray.log`). Use **Quit** in the tray to stop it.
An already running tray is reused. Start at Login launches with `--daemon`;
neither mode automatically enables swap.

## Development

```sh
cargo build            # needs a C linker (gcc or clang) on PATH
cargo test
./target/debug/swaptray --status
```

Environment variables:

- `SWAPTRAY_SWAPFILE` — override the managed path (testing)
- `SWAPTRAY_DRY_RUN=1` — print the privileged script instead of running it
  (exercises the whole flow without root)

### Release

One command builds all variants (`x86_64`/`aarch64`, glibc with a 2.17
floor, and dynamically linked musl for Alpine), packages them and prints the upload command:

```sh
./release.sh 0.1.5
```

Backend: `cargo-zigbuild` + `zig` (`cargo install cargo-zigbuild`), or
`cross` as a fallback. Then push the built assets to a GitHub Release
(exact `gh release create` command is printed at the end); users install
with `mise use -g github:manhavn/swaptray`.

The build also needs Python 3 (standard library only), `file`, `tar`, and
`sha256sum`. Musl cross-linking downloads checksum-pinned Alpine musl 1.2.5
loaders; they are not bundled. Musl desktops need musl 1.2.5 or newer plus
their native GUI libraries. Update Cargo.toml before releasing a new version.
The zigbuild backend is verified; cross additionally needs a container runtime.

The Zig adapter removes rustc's obsolete `-Wl,-O1` linker argument, which
Zig otherwise ignores with a deprecation warning. Rust optimization and LTO
remain enabled; linker warnings and errors are not suppressed.

## Notes

- The swap file is created with `fallocate` (falls back to `dd`),
  `chmod 600`, `chattr +C` where supported (btrfs), then `mkswap`.
- The create dialog can be closed while the privileged command is still
  running: the command keeps going in the background, and the tray menu shows
  the result the next time it is opened.
- No `/etc/fstab` changes are made. SwapTray never enables swap automatically;
  an existing boot configuration managed by another tool remains unchanged.
- GUI libraries are loaded at runtime: OpenGL/EGL, libxkbcommon, and
  libxkbcommon-x11 for X11 sessions. A polkit authentication agent must run
  in your desktop session. Headless servers do not provide a tray.
- The privileged script is piped to `/bin/sh` on stdin instead of passed with
  `-c`. The authentication dialog shows pkexec's command line and pkexec
  truncates anything past 80 bytes (first 38, ` ... `, last 37), so `-c` made
  the dialog display the swap script as an unreadable fragment; now it reads
  "Authentication is needed to run `/bin/sh' as the super user", and the
  script no longer appears in the dialog or in `ps` output.
- **Start at Login** points `Exec=` at a version-independent path: the mise
  shim `~/.local/share/mise/shims/swaptray` (or the `<tool>/latest` alias,
  whichever exists) instead of the versioned install path mise execs the
  binary from. Upgrading with `mise` therefore keeps the entry working without
  toggling *Start at Login* off and on; an entry left behind by an older
  SwapTray is rewritten at startup. An `Exec=` without a mise install path
  (your own entry) is never touched.
