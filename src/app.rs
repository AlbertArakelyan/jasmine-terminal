//! Top-level eframe application. Owns the terminal session and routes events between
//! the PTY and the UI.

use std::time::{Duration, Instant};

use alacritty_terminal::event::Event;
use egui::FontId;
use tracing::{debug, error, info};

use crate::config::Config;
use crate::terminal::session::{TermSize, TerminalSession};
use crate::ui::cell_metrics::{CellMetrics, GridSize};
use crate::ui::fonts;

/// Debug builds only: write `exit\r` to the shell this many milliseconds after it starts.
/// Exercises the shell-exit path end to end (SPEC Step 4) before keyboard input exists.
const DEBUG_EXIT_ENV: &str = "JTERM_DEBUG_EXIT_AFTER_MS";

/// Root application state.
pub struct JtermApp {
    config: Config,
    /// Cell size and the `(pixels_per_point, font size)` it was measured at. Measured
    /// lazily because egui has no fonts until the first frame, and re-measured when either
    /// changes (e.g. the window moves to a monitor with different DPI).
    metrics: Option<((f32, f32), CellMetrics)>,
    grid: Option<GridSize>,
    /// Started on the first frame, once the grid size is known.
    session: Option<TerminalSession>,
    spawn_failed: bool,
    wakeups: u64,
    debug_exit: Option<DebugExit>,
    /// The close command is already sent; the window can take another frame to go away.
    closing: bool,
}

struct DebugExit {
    delay: Duration,
    /// Set when the shell spawns, so the delay counts from shell start, not app start.
    at: Option<Instant>,
    sent: bool,
}

impl JtermApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        fonts::install(&cc.egui_ctx, &config.font);
        Self {
            config,
            metrics: None,
            grid: None,
            session: None,
            spawn_failed: false,
            wakeups: 0,
            debug_exit: debug_exit_from_env(),
            closing: false,
        }
    }

    fn cell_metrics(&mut self, ctx: &egui::Context) -> CellMetrics {
        let ppp = ctx.pixels_per_point();
        let key = (ppp, self.config.font.size);
        match self.metrics {
            Some((measured_at, m)) if measured_at == key => m,
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
                self.metrics = Some((key, m));
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

    fn ensure_session(&mut self, ctx: &egui::Context, grid: GridSize, metrics: CellMetrics) {
        if self.session.is_some() || self.spawn_failed {
            return;
        }
        // The PTY wants the cell size in physical pixels; metrics are snapped, so this is exact.
        let to_px = |points: f32| (points * ctx.pixels_per_point()).round() as u16;
        let size = TermSize {
            cols: grid.cols,
            rows: grid.rows,
            cell_width: to_px(metrics.width),
            cell_height: to_px(metrics.height),
        };
        match TerminalSession::spawn(&self.config, size, ctx.clone()) {
            Ok(session) => {
                if let Some(exit) = &mut self.debug_exit {
                    exit.at = Some(Instant::now() + exit.delay);
                }
                self.session = Some(session);
            }
            Err(e) => {
                // Don't retry every frame. Step 5 will show this in the window.
                error!("{e:#}");
                self.spawn_failed = true;
            }
        }
    }

    fn handle_terminal_events(&mut self, ctx: &egui::Context) {
        let Some(session) = &mut self.session else {
            return;
        };
        for event in session.poll_events() {
            match event {
                Event::Wakeup => {
                    self.wakeups += 1;
                    if self.wakeups == 1 {
                        info!("first output from the shell");
                    } else {
                        debug!(count = self.wakeups, "wakeup");
                    }
                }
                Event::ChildExit(status) => info!(%status, "shell exited"),
                Event::Exit => info!("terminal exited"),
                // Deliberately unanswered: letting programs read the clipboard is a data
                // leak, and SPEC §4.16 denies OSC 52 reads by default. TextAreaSizeRequest
                // (Step 7) and ColorRequest (Step 5) still need answers.
                Event::ClipboardLoad(..) => debug!("ignored clipboard read request"),
                // Titles are terminal content: debug level only (SPEC §4.16).
                other => debug!(event = ?other, "unhandled terminal event"),
            }
        }

        if let Some(exit) = &mut self.debug_exit
            && let Some(at) = exit.at
            && !exit.sent
        {
            let now = Instant::now();
            if now >= at {
                info!("{DEBUG_EXIT_ENV}: writing `exit\\r` to the shell");
                session.write(&b"exit\r"[..]);
                exit.sent = true;
            } else {
                ctx.request_repaint_after(at - now);
            }
        }

        if session.exited && !self.closing {
            info!("closing the window because the shell exited");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.closing = true;
        }
    }
}

impl eframe::App for JtermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let metrics = self.cell_metrics(&ctx);
        let mut grid = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.config.theme.background.into()))
            .show(ui, |ui| {
                grid = Some(metrics.grid_size(ui.max_rect(), self.config.window.padding));
            });
        if let Some(grid) = grid {
            self.update_grid(grid);
            self.ensure_session(&ctx, grid, metrics);
        }
        self.handle_terminal_events(&ctx);
    }
}

fn debug_exit_from_env() -> Option<DebugExit> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let ms: u64 = std::env::var(DEBUG_EXIT_ENV).ok()?.parse().ok()?;
    Some(DebugExit {
        delay: Duration::from_millis(ms),
        at: None,
        sent: false,
    })
}
