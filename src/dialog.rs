//! A short-lived child process owns each native dialog/event loop.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use crate::{icon, notify, ops, state};
static OPEN: AtomicBool = AtomicBool::new(false);

pub fn open() {
    if OPEN.swap(true, Ordering::AcqRel) { return; }
    std::thread::spawn(|| {
        let result = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg("--internal-create-dialog").status());
        if let Err(e) = result { notify::notify("SwapTray", &format!("Cannot open dialog: {e}")); }
        OPEN.store(false, Ordering::Release);
    });
}

/// Width of the content column; the window width follows from it.
const CONTENT_W: f32 = 360.0;
/// `egui::Frame::central_panel` inner margin, mirrored to size the window.
const MARGIN: f32 = 8.0;
/// How often to re-ask for the fitted inner size before giving up.
const FIT_TRIES: u8 = 8;

pub fn run() -> i32 {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Create Swap File - SwapTray")
            .with_app_id("swaptray")
            // Content height measured from the layout below; the first frame
            // fits the window to the panel exactly.
            .with_inner_size([CONTENT_W + 2.0 * MARGIN, 136.0])
            .with_icon(icon::window_icon(64).unwrap_or_default()),
        ..Default::default()
    };
    match eframe::run_native("SwapTray", options, Box::new(|_| Ok(Box::new(Dialog {
        gb: "4".into(), error: None, pending: None, fitted: None, fit_tries: 0,
    })))) {
        Ok(()) => 0,
        Err(e) => { notify::notify("SwapTray", &format!("Cannot open dialog: {e}")); 1 }
    }
}
struct Dialog {
    gb: String,
    error: Option<String>,
    pending: Option<mpsc::Receiver<Result<String, ops::OpError>>>,
    /// Inner size last asked of the window manager; guards against resize loops.
    fitted: Option<egui::Vec2>,
    fit_tries: u8,
}
impl eframe::App for Dialog {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if let Some(rx) = &self.pending {
            if let Ok(result) = rx.try_recv() {
                self.pending = None;
                match result {
                    Ok(_) => { notify::notify("Swap file created", "Turn it on manually from the tray menu."); ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
        let busy = self.pending.is_some();
        // The root `Ui` covers the whole viewport but has no background of its
        // own: fill it so no clear-colour border shows around the panel while
        // the window is still being fitted to the content.
        ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);

        let mut submit = false;
        let panel = egui::Frame::central_panel(&ui.style()).show(ui, |ui| {
            ui.set_width(CONTENT_W);
            ui.heading("Create Swap File");
            ui.add_space(2.0);
            ui.label(format!("Path: {}", state::swap_path().display()));
            ui.label("Creation formats the file but does not enable swap.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Size (GiB):");
                ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.gb).desired_width(120.0));
            });
            if let Some(error) = &self.error { ui.add_space(4.0); ui.colored_label(ui.visuals().error_fg_color, error); }
            if busy {
                ui.add_space(4.0);
                ui.horizontal(|ui| { ui.spinner(); ui.label("Waiting for authentication / creating file..."); });
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            ui.add_space(10.0);
            // `with_layout` would claim all remaining space and defeat the
            // shrink-wrap below, so allocate exactly one button row.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    submit = ui.add_enabled(!busy, egui::Button::new("Create")).clicked();
                    if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                },
            );
        });

        // Shrink-wrap the window to the panel so no empty area is left over.
        // The content layout is fixed width, so the requested size is stable.
        let want = panel.response.rect.size().round();
        let current = ui.ctx().viewport_rect().size();
        if self.fitted != Some(want) { self.fitted = Some(want); self.fit_tries = 0; }
        // Keep asking for a few frames: some compositors re-apply their own size
        // once after ours (seen on Mutter), others ignore the request entirely.
        if self.fit_tries < FIT_TRIES && (current - want).length_sq() > 0.25 {
            self.fit_tries += 1;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
            ui.ctx().request_repaint();
        }
        if busy && ui.input(|i| i.viewport().close_requested()) { ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose); }
        if submit {
            match state::parse_gb(&self.gb) {
                Err(e) => self.error = Some(e),
                Ok(bytes) => {
                    let (tx, rx) = mpsc::channel(); self.pending = Some(rx); self.error = None;
                    std::thread::spawn(move || { let _ = tx.send(ops::execute(&ops::Op::Create { bytes })); });
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
    }
}
