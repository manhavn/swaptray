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

pub fn run() -> i32 {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Create Swap File - SwapTray")
            .with_app_id("swaptray").with_inner_size([420.0, 240.0])
            .with_icon(icon::window_icon(64).unwrap_or_default()),
        ..Default::default()
    };
    match eframe::run_native("SwapTray", options, Box::new(|_| Ok(Box::new(Dialog {
        gb: "4".into(), error: None, pending: None,
    })))) {
        Ok(()) => 0,
        Err(e) => { notify::notify("SwapTray", &format!("Cannot open dialog: {e}")); 1 }
    }
}
struct Dialog { gb: String, error: Option<String>, pending: Option<mpsc::Receiver<Result<String, ops::OpError>>> }
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
        let mut submit = false;
        egui::Frame::central_panel(&ui.style()).show(ui, |ui| {
            ui.heading("Create Swap File");
            ui.label(format!("Path: {}", state::swap_path().display()));
            ui.label("Creation formats the file but does not enable swap.");
            ui.horizontal(|ui| { ui.label("Size (GiB):"); ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.gb).desired_width(120.0)); });
            if let Some(error) = &self.error { ui.colored_label(ui.visuals().error_fg_color, error); }
            if busy { ui.spinner(); ui.label("Waiting for authentication / creating file..."); ui.ctx().request_repaint_after(Duration::from_millis(100)); }
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                submit = ui.add_enabled(!busy, egui::Button::new("Create")).clicked();
            });
        });
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
