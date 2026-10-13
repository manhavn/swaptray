//! System tray icon and menu (freedesktop StatusNotifierItem via ksni).

use std::sync::mpsc::Sender;

use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};
use ksni::ToolTip;

use crate::autostart;
use crate::dialog;
use crate::icon;
use crate::notify;
use crate::ops::Op;
use crate::state::{self, Snapshot};

/// A privileged operation request handed to the worker thread.
pub struct JobRequest {
    pub op: Op,
}

pub struct SwapTray {
    pub snap: Snapshot,
    pub autostart: bool,
    pub busy: Option<&'static str>,
    pub job_tx: Sender<JobRequest>,
}

impl SwapTray {
    /// Re-read swap state from the system (called before every menu display
    /// and after every operation).
    pub fn refresh(&mut self) {
        self.snap = Snapshot::read();
        self.autostart = autostart::enabled();
    }

    fn status_line(&self) -> String {
        let path = state::swap_path();
        let path = path.to_string_lossy();
        if let Some(op) = self.busy {
            return format!("Working: {op}...");
        }
        if !self.snap.exists {
            return format!("No swap file ({path})");
        }
        let size = state::human(self.snap.size_bytes);
        if self.snap.active {
            format!("Swap: ON — {size} ({path})")
        } else {
            format!("Swap: OFF — {size} ({path})")
        }
    }
}

impl ksni::Tray for SwapTray {
    // Left click opens the menu directly: quick control from the tray.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "swaptray".into()
    }

    fn title(&self) -> String {
        "SwapTray".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        icon::pixmaps()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_pixmap: icon::pixmaps(),
            title: "SwapTray".into(),
            description: self.status_line(),
            ..Default::default()
        }
    }

    fn menu_about_to_show(&mut self) {
        self.refresh();
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        eprintln!(
            "[swaptray] no StatusNotifierWatcher yet ({reason:?}); \
             the icon appears once the desktop loads its tray support"
        );
        true
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let free = self.busy.is_none();
        let exists = self.snap.exists;
        let active = self.snap.active;

        vec![
            StandardItem {
                label: self.status_line(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Create Swap File...".into(),
                enabled: free && !exists,
                activate: Box::new(|_: &mut Self| dialog::open()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Turn On Swap".into(),
                enabled: free && exists && !active,
                activate: Box::new(|this: &mut Self| {
                    let _ = this.job_tx.send(JobRequest {
                        op: Op::TurnOn,
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Turn Off Swap".into(),
                enabled: free && active,
                activate: Box::new(|this: &mut Self| {
                    let _ = this.job_tx.send(JobRequest {
                        op: Op::TurnOff,
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Delete Swap File".into(),
                enabled: free && exists,
                activate: Box::new(|this: &mut Self| {
                    let _ = this.job_tx.send(JobRequest {
                        op: Op::Delete,
                    });
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Start at Login".into(),
                checked: self.autostart,
                enabled: free,
                activate: Box::new(|this: &mut Self| {
                    let next = !this.autostart;
                    match autostart::set_enabled(next) {
                        Ok(()) => this.autostart = next,
                        Err(e) => notify::notify("SwapTray", &format!("Start at Login: {e}")),
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
