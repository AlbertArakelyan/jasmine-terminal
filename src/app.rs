//! Top-level eframe application. Owns the terminal session and routes events between
//! the PTY and the UI.

use egui::FontId;
use tracing::{debug, info};

use crate::config::Config;
use crate::ui::cell_metrics::{CellMetrics, GridSize};
use crate::ui::fonts;

/// Root application state.
pub struct JtermApp {
    config: Config,
    /// Cell size and the `pixels_per_point` it was measured at. Measured lazily because
    /// egui has no fonts until the first frame, and re-measured when the scale changes
    /// (e.g. the window moves to a monitor with different DPI).
    metrics: Option<(f32, CellMetrics)>,
    grid: Option<GridSize>,
}

impl JtermApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        fonts::install(&cc.egui_ctx, &config.font);
        Self {
            config,
            metrics: None,
            grid: None,
        }
    }

    fn cell_metrics(&mut self, ctx: &egui::Context) -> CellMetrics {
        let ppp = ctx.pixels_per_point();
        match self.metrics {
            Some((measured_at, m)) if measured_at == ppp => m,
            _ => {
                let font = FontId::monospace(self.config.font.size);
                let m = CellMetrics::measure(ctx, &font);
                info!(
                    font_size = self.config.font.size,
                    pixels_per_point = ppp,
                    cell_width = m.width,
                    cell_height = m.height,
                    "measured cell metrics"
                );
                self.metrics = Some((ppp, m));
                m
            }
        }
    }

    /// Records the grid size for the current rect. Step 7 will resize the terminal here.
    fn update_grid(&mut self, grid: GridSize) {
        if self.grid == Some(grid) {
            return;
        }
        if self.grid.is_none() {
            info!(cols = grid.cols, rows = grid.rows, "initial grid size");
        } else {
            debug!(cols = grid.cols, rows = grid.rows, "grid size changed");
        }
        self.grid = Some(grid);
    }
}

impl eframe::App for JtermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let metrics = self.cell_metrics(ui.ctx());
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.config.theme.background.into()))
            .show(ui, |ui| {
                let grid = metrics.grid_size(ui.max_rect(), self.config.window.padding);
                self.update_grid(grid);
            });
    }
}
