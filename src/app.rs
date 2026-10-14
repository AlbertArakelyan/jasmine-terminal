//! Top-level eframe application. Owns the terminal session and routes events between
//! the PTY and the UI.

use std::time::{Duration, Instant};

use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::Rgb;
use egui::FontId;
use tracing::{debug, error, info};

use crate::config::Config;
use crate::terminal::session::{TermSize, TerminalSession};
use crate::ui::cell_metrics::{CellMetrics, GridSize};
use crate::ui::fonts;
use crate::ui::input_map::{Input, InputModes, KeyboardState};
use crate::ui::mouse::{self, PointerAction, PointerState, WheelAccumulator, WheelAction};
use crate::ui::terminal_view::{Layout, TerminalView};

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
    spawn_error: Option<String>,
    view: TerminalView,
    keyboard: KeyboardState,
    wheel: WheelAccumulator,
    pointer: PointerState,
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
        // Ctrl+Plus/Minus/0 belong to the shell (Ctrl+- is 0x1f), not to egui's UI zoom.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        Self {
            config,
            metrics: None,
            grid: None,
            session: None,
            spawn_error: None,
            view: TerminalView::default(),
            keyboard: KeyboardState::default(),
            wheel: WheelAccumulator::default(),
            pointer: PointerState::default(),
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

    /// Records the grid size for the current rect, for logging.
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

    /// Starts the shell on the first frame, and afterwards keeps the terminal and PTY sized
    /// to the window. The grid size is whole cells, so pixel changes smaller than a cell
    /// (and repeated frames at the same size) don't resize anything.
    fn sync_session(&mut self, ctx: &egui::Context, grid: GridSize, metrics: CellMetrics) {
        // The PTY wants the cell size in physical pixels; metrics are snapped, so this is exact.
        let to_px = |points: f32| (points * ctx.pixels_per_point()).round() as u16;
        let size = TermSize {
            cols: grid.cols,
            rows: grid.rows,
            cell_width: to_px(metrics.width),
            cell_height: to_px(metrics.height),
        };
        if let Some(session) = &mut self.session {
            session.resize(size);
            return;
        }
        if self.spawn_error.is_some() {
            return;
        }
        match TerminalSession::spawn(&self.config, size, ctx.clone()) {
            Ok(session) => {
                if let Some(exit) = &mut self.debug_exit {
                    exit.at = Some(Instant::now() + exit.delay);
                }
                self.session = Some(session);
            }
            Err(e) => {
                // Shown in the window; not retried every frame.
                error!("{e:#}");
                self.spawn_error = Some(format!("{e:#}"));
            }
        }
    }

    /// Forwards keyboard, wheel and pointer input. The terminal widget keeps keyboard focus and
    /// locks Tab, Escape and the arrows to itself, so egui doesn't use them to move focus.
    fn handle_input(&mut self, ctx: &egui::Context, response: &egui::Response, layout: &Layout) {
        if !response.has_focus() {
            response.request_focus();
        }
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
        let Some(session) = &self.session else {
            return;
        };
        // Copy the events out first: `Term` must never be locked inside an egui closure.
        let (events, pointer) = ctx.input(|i| (i.events.clone(), i.pointer.latest_pos()));
        if events.is_empty() {
            return;
        }
        let mode = session.mode();
        let modes = InputModes::from(mode);
        let size = session.size();
        let (cols, rows) = (usize::from(size.cols), usize::from(size.rows));
        for event in &events {
            // Keystrokes are only ever logged at trace level (SPEC §4.16).
            tracing::trace!(?event, "input event");
            match event {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    // macOS turns Shift+wheel into horizontal scrolling; treat it as vertical.
                    let dy = if delta.y == 0.0 && modifiers.shift {
                        delta.x
                    } else {
                        delta.y
                    };
                    let lines = self.wheel.lines(*unit, dy, layout.cell.height, rows);
                    let cell = pointer.map(|p| layout.cell_at(p, cols, rows));
                    let action = mouse::wheel_action(lines, mode, *modifiers, cell, rows);
                    tracing::trace!(lines, ?cell, ?mode, ?action, "wheel");
                    match action {
                        Some(WheelAction::Scroll(n)) => {
                            session.scroll(Scroll::Delta(n));
                            // Keep extending a drag selection as the view scrolls under it.
                            if let Some(p) = pointer.filter(|_| self.pointer.is_selecting()) {
                                let (cell, side) = layout.hit(p, cols, rows);
                                session.update_selection(cell, side);
                            }
                        }
                        Some(WheelAction::Write(bytes)) => session.write(bytes),
                        None => {}
                    }
                    continue;
                }
                // Shift+PgUp/PgDn page through the scrollback. Full-screen programs (less,
                // vim) get the keys instead, as they have no scrollback to page through.
                egui::Event::Key {
                    key: key @ (egui::Key::PageUp | egui::Key::PageDown),
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.shift
                    && !modifiers.ctrl
                    && !modifiers.alt
                    && !mode.contains(TermMode::ALT_SCREEN) =>
                {
                    session.scroll(if *key == egui::Key::PageUp {
                        Scroll::PageUp
                    } else {
                        Scroll::PageDown
                    });
                    continue;
                }
                _ => {}
            }
            // The pointer state also watches keys, modifiers and focus, so every event goes
            // through it; only pointer events stop here.
            let hit = |p| layout.hit(p, cols, rows);
            let action = self.pointer.handle(event, Instant::now(), mode, hit);
            if matches!(
                event,
                egui::Event::PointerButton { .. } | egui::Event::PointerMoved(_)
            ) {
                match action {
                    Some(PointerAction::Write(bytes)) => session.write(bytes),
                    Some(PointerAction::StartSelection(ty, cell, side)) => {
                        session.start_selection(ty, cell, side);
                    }
                    Some(PointerAction::UpdateSelection(cell, side)) => {
                        session.update_selection(cell, side);
                    }
                    None => {}
                }
                continue;
            }
            match self.keyboard.feed(event, modes) {
                Some(Input::Write(bytes)) => session.send_input(bytes),
                Some(Input::Copy) => {
                    if let Some(text) = session.selection_text() {
                        ctx.copy_text(text);
                    }
                }
                Some(Input::CopyOrInterrupt) => match session.selection_text() {
                    Some(text) => {
                        ctx.copy_text(text);
                        session.clear_selection();
                    }
                    None => session.send_input(vec![0x03]),
                },
                None => {}
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
                // OSC 4/10/11 queries: programs like vim, bat and delta ask for the
                // background color to pick light or dark styles.
                Event::ColorRequest(index, format) => {
                    let c = session.color(index, &self.config.theme);
                    session.write(
                        format(Rgb {
                            r: c.r(),
                            g: c.g(),
                            b: c.b(),
                        })
                        .into_bytes(),
                    );
                }
                // CSI 14 t / 18 t: programs asking for the text area size in pixels or cells.
                Event::TextAreaSizeRequest(format) => {
                    session.write(format(session.size().into()).into_bytes());
                }
                // Deliberately unanswered: letting programs read the clipboard is a data
                // leak, and SPEC §4.16 denies OSC 52 reads by default.
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
        // Events first: replies to color queries and exit handling shouldn't wait a frame.
        self.handle_terminal_events(&ctx);

        let metrics = self.cell_metrics(&ctx);
        // Read before snapshotting: never lock `Term` inside an egui context closure.
        let focused = ctx.input(|i| i.focused);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.config.theme.background.into()))
            .show(ui, |ui| {
                // The grid is sized from the exact rect the terminal is painted in.
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let padding = self.config.window.padding;
                let grid = metrics.grid_size(rect, padding);
                self.update_grid(grid);
                self.sync_session(&ctx, grid, metrics);
                let layout =
                    Layout::new(&ctx, rect, padding, metrics, self.config.font.size, focused);
                self.handle_input(&ctx, &response, &layout);

                let painter = ui.painter_at(rect);
                if let Some(session) = &self.session {
                    let snapshot = session.snapshot(&self.config.theme);
                    self.view.paint(&painter, rect, layout, &snapshot);
                    // egui-winit only enables the OS IME while this is set, and places the
                    // composition window at the cursor.
                    if response.has_focus() {
                        let cursor = snapshot
                            .cursor
                            .map_or(rect, |c| layout.cell_rect(c.row, c.col, 1));
                        ctx.output_mut(|o| {
                            o.ime = Some(egui::output::IMEOutput {
                                purpose: egui::IMEPurpose::Terminal,
                                rect,
                                cursor_rect: cursor,
                                should_interrupt_composition: false,
                            });
                        });
                    }
                } else if let Some(error) = &self.spawn_error {
                    painter.text(
                        rect.min + egui::vec2(padding, padding),
                        egui::Align2::LEFT_TOP,
                        format!("jterm could not start the shell:\n{error}"),
                        egui::FontId::monospace(self.config.font.size),
                        self.config.theme.normal.red.into(),
                    );
                }
            });
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
