//! Top-level eframe application. Owns the terminal session and routes events between
//! the PTY and the UI.

use crate::config::Config;
use crate::ui::fonts;

/// Root application state.
pub struct JtermApp {
    config: Config,
}

impl JtermApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        fonts::install(&cc.egui_ctx, &config.font);
        Self { config }
    }
}

impl eframe::App for JtermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.config.theme.background.into()))
            .show(ui, |_ui| {});
    }
}
