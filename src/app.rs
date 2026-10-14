//! Top-level eframe application. Owns the terminal session and routes events between
//! the PTY and the UI.

/// Root application state.
pub struct JtermApp;

impl JtermApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self
    }
}

impl eframe::App for JtermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |_ui| {});
    }
}
