//! `TerminalSession`: spawns the shell in a PTY, runs the I/O event loop, and handles writes.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, OnResize, WindowSize};
use alacritty_terminal::event_loop::{self, EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi;
use anyhow::Context as _;
use tracing::{info, warn};

use super::event_proxy::EventProxy;
use super::snapshot::{Palette, RenderSnapshot};
use crate::config::theme::Theme;
use crate::config::{Config, CursorStyle, ShellConfig};

/// Grid size handed to `alacritty_terminal`, which takes it through the `Dimensions` trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
    /// Cell size in physical pixels. Only used to report the pixel size to the PTY
    /// (`TIOCSWINSZ`), which some programs use to size images.
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

impl From<TermSize> for WindowSize {
    fn from(s: TermSize) -> Self {
        Self {
            num_lines: s.rows,
            num_cols: s.cols,
            cell_width: s.cell_width,
            cell_height: s.cell_height,
        }
    }
}

/// One shell running in a PTY, plus the terminal state its output is parsed into.
pub struct TerminalSession {
    /// Shared with the PTY thread. Lock only to copy state out; see CLAUDE.md.
    term: Arc<FairMutex<Term<EventProxy>>>,
    notifier: Notifier,
    events_rx: Receiver<Event>,
    proxy: EventProxy,
    /// The PTY I/O thread. It also stops without an `Exit` event when a PTY read or write
    /// fails, so a finished thread counts as the shell having exited.
    io_thread: JoinHandle<(EventLoop<tty::Pty, EventProxy>, event_loop::State)>,
    /// Current grid and cell size, as last given to the `Term` and the PTY.
    size: TermSize,
    pub exited: bool,
}

impl TerminalSession {
    /// Starts the shell and the PTY reader thread. Output is parsed into `term` on that
    /// thread; the UI is told about it through `ctx.request_repaint()` and `poll_events`.
    pub fn spawn(config: &Config, size: TermSize, ctx: egui::Context) -> anyhow::Result<Self> {
        let options = pty_options(&config.shell);
        match &options.shell {
            Some(shell) => info!(?shell, "spawning shell"),
            None => info!(shell = ?std::env::var("SHELL").ok(), "spawning default login shell"),
        }

        let (tx, events_rx) = mpsc::channel();
        let proxy = EventProxy::new(tx, ctx);

        let term_config = term::Config {
            scrolling_history: config.scrollback.lines,
            // Programs can change the cursor (DECSCUSR); a reset (CSI 0 q) returns to this.
            default_cursor_style: ansi::CursorStyle {
                shape: match config.cursor.style {
                    CursorStyle::Block => ansi::CursorShape::Block,
                    CursorStyle::Beam => ansi::CursorShape::Beam,
                    CursorStyle::Underline => ansi::CursorShape::Underline,
                },
                blinking: config.cursor.blink,
            },
            ..term::Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(term_config, &size, proxy.clone())));

        #[cfg(windows)]
        enable_ctrl_c_for_children();
        let pty =
            tty::new(&options, size.into(), 0).context("failed to start the shell in a PTY")?;
        // drain_on_exit: read the shell's last output before reporting `Exit`.
        let event_loop = EventLoop::new(Arc::clone(&term), proxy.clone(), pty, true, false)
            .context("failed to create the PTY event loop")?;
        // Connect before the loop starts parsing, so no reply can be emitted unconnected.
        proxy.connect(event_loop.channel());
        let notifier = Notifier(event_loop.channel());
        let io_thread = event_loop.spawn();
        info!(cols = size.cols, rows = size.rows, "shell spawned");

        Ok(Self {
            term,
            notifier,
            events_rx,
            proxy,
            io_thread,
            size,
            exited: false,
        })
    }

    /// Copies the visible screen for painting. Holds the `Term` lock only while copying.
    /// Must not be called inside an egui context closure (see `EventProxy`).
    pub fn snapshot(&self, theme: &Theme) -> RenderSnapshot {
        RenderSnapshot::capture(&self.term.lock(), theme)
    }

    /// The current RGB value of terminal color `index`, for answering OSC 4/10/11 queries.
    pub fn color(&self, index: usize, theme: &Theme) -> egui::Color32 {
        let term = self.term.lock();
        Palette::new(theme, term.colors()).get(index)
    }

    /// Terminal modes that change what keys send.
    pub fn size(&self) -> TermSize {
        self.size
    }

    /// Resizes the grid (reflowing wrapped lines) and tells the shell the new size, which
    /// it receives as SIGWINCH on Unix or a console resize under ConPTY. No-op when the
    /// size is unchanged, so callers can call it every frame.
    pub fn resize(&mut self, size: TermSize) {
        if size == self.size {
            return;
        }
        self.term.lock().resize(size);
        self.notifier.on_resize(size.into());
        self.size = size;
    }

    /// Starts a selection at the visible cell `(col, row)`. Selections live in `Term`, in
    /// grid coordinates, so they stay on the same text while output scrolls it.
    pub fn start_selection(&self, ty: SelectionType, (col, row): (usize, usize), side: Side) {
        let mut term = self.term.lock();
        let point = viewport_to_grid(&term, col, row);
        term.selection = Some(Selection::new(ty, point, side));
    }

    /// Extends the current selection to the visible cell `(col, row)`.
    pub fn update_selection(&self, (col, row): (usize, usize), side: Side) {
        let mut term = self.term.lock();
        let point = viewport_to_grid(&term, col, row);
        if let Some(selection) = &mut term.selection {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    /// The selected text, or `None` when nothing (or only an empty range) is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|s| !s.is_empty())
    }

    /// The terminal's current modes (mouse reporting, alternate screen, ...).
    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Moves the view through the scrollback. Output arriving while scrolled up keeps the
    /// view where it is (alacritty shifts `display_offset` with it); typing returns to the
    /// bottom (`send_input`).
    pub fn scroll(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    /// Sends user input to the shell. Like other terminals, typing jumps back to the
    /// bottom if the view was scrolled into history, and clears the selection.
    pub fn send_input(&self, bytes: Vec<u8>) {
        {
            let mut term = self.term.lock();
            if term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
            }
            // Typing replaces the selection highlight, as in other terminals.
            term.selection = None;
        }
        self.write(bytes);
    }

    /// Queues bytes for the PTY. Never blocks; the I/O thread does the write.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        if self.notifier.0.send(Msg::Input(bytes.into())).is_err() {
            warn!("PTY event loop is gone; dropping input");
        }
    }

    /// Drains terminal events queued since the last frame. Marks the session exited
    /// when the shell has gone, so the app can close the window.
    ///
    /// `Exit` is used rather than `ChildExit` because it comes after the final output has
    /// been drained (`drain_on_exit`), so nothing the shell printed last is lost.
    pub fn poll_events(&mut self) -> Vec<Event> {
        self.proxy.clear_wakeup();
        let events: Vec<Event> = self.events_rx.try_iter().collect();
        if events.iter().any(|e| matches!(e, Event::Exit)) || self.io_thread.is_finished() {
            self.exited = true;
        }
        events
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Stop the PTY thread; the error case means it has already stopped.
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

/// Visible row `row` is grid line `row - display_offset`: line 0 is the top of the live
/// screen, and history lines are negative.
fn viewport_to_grid<T>(term: &Term<T>, col: usize, row: usize) -> Point {
    let offset = term.grid().display_offset() as i32;
    let last_col = term.columns().saturating_sub(1);
    Point::new(Line(row as i32 - offset), Column(col.min(last_col)))
}

/// PTY options from config plus the environment SPEC §4.1 requires. The variables go
/// through the PTY options rather than `std::env::set_var` on our own process.
fn pty_options(shell: &ShellConfig) -> tty::Options {
    let mut env = HashMap::from([
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ("TERM_PROGRAM".to_owned(), "jterm".to_owned()),
    ]);
    // WSL passes only the Windows variables named in WSLENV into Linux, so without this,
    // programs in `wsl` see no COLORTERM and fall back from truecolor to 256 colors.
    if cfg!(windows) {
        env.insert(
            "WSLENV".to_owned(),
            wslenv_with(std::env::var("WSLENV").ok().as_deref()),
        );
    }
    tty::Options {
        shell: resolve_shell(shell, find_on_path).map(|(p, a)| tty::Shell::new(p, a)),
        working_directory: home_dir(),
        drain_on_exit: true,
        env,
        // Quote arguments the standard way so paths with spaces survive.
        #[cfg(windows)]
        escape_args: true,
    }
}

/// Adds COLORTERM and TERM_PROGRAM to an existing WSLENV list (colon-separated names, each
/// optionally with `/flags`), keeping whatever the user already forwards.
fn wslenv_with(existing: Option<&str>) -> String {
    let mut names: Vec<&str> = existing
        .unwrap_or_default()
        .split(':')
        .filter(|n| !n.is_empty())
        .collect();
    for ours in ["COLORTERM", "TERM_PROGRAM"] {
        if !names.iter().any(|n| n.split('/').next() == Some(ours)) {
            names.push(ours);
        }
    }
    names.join(":")
}

/// Picks the program to run. `None` lets `alacritty_terminal` choose, which on Unix
/// means `$SHELL`, falling back to the user's passwd entry, started as a login shell
/// (via `login` on macOS). Windows has no `$SHELL`, so prefer PowerShell 7 (`pwsh`)
/// and fall back to the built-in Windows PowerShell.
fn resolve_shell(
    config: &ShellConfig,
    is_on_path: impl Fn(&str) -> bool,
) -> Option<(String, Vec<String>)> {
    if let Some(program) = &config.program {
        return Some((program.clone(), config.args.clone()));
    }
    if cfg!(windows) {
        let program = if is_on_path("pwsh.exe") {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };
        return Some((program.to_owned(), config.args.clone()));
    }
    if !config.args.is_empty() {
        warn!("shell.args is ignored unless shell.program is set");
    }
    None
}

/// Windows children inherit their parent's "ignore Ctrl+C" flag. When jterm is started
/// from a process that set it (a shell, cargo, an IDE), the shell would inherit it too, and
/// the ^C that ConPTY turns into CTRL_C_EVENT would be ignored: Ctrl+C couldn't stop
/// anything. Clearing the flag on ourselves before spawning restores normal behaviour.
/// It's process-wide state, so it's done once, on the first spawn.
#[cfg(windows)]
fn enable_ctrl_c_for_children() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: a null handler with FALSE only clears this process's ignore-Ctrl+C flag;
        // no pointers are passed.
        let ok = unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0) };
        if ok == 0 {
            warn!(
                "SetConsoleCtrlHandler failed: {}; Ctrl+C may not interrupt programs",
                std::io::Error::last_os_error()
            );
        }
    });
}

fn find_on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn shell(program: Option<&str>, args: &[&str]) -> ShellConfig {
        ShellConfig {
            program: program.map(str::to_owned),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn configured_program_wins() {
        let got = resolve_shell(&shell(Some("/bin/zsh"), &["-l"]), |_| true);
        assert_eq!(got, Some(("/bin/zsh".to_owned(), vec!["-l".to_owned()])));
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefers_pwsh() {
        let got = resolve_shell(&shell(None, &[]), |p| p == "pwsh.exe");
        assert_eq!(got.map(|(p, _)| p).as_deref(), Some("pwsh.exe"));
        let got = resolve_shell(&shell(None, &[]), |_| false);
        assert_eq!(got.map(|(p, _)| p).as_deref(), Some("powershell.exe"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_defers_to_alacritty() {
        assert_eq!(resolve_shell(&shell(None, &[]), |_| true), None);
    }

    #[test]
    fn wslenv_keeps_existing_names() {
        let cases = [
            (None, "COLORTERM:TERM_PROGRAM"),
            (Some(""), "COLORTERM:TERM_PROGRAM"),
            (
                Some("GOPATH/l:USERPROFILE/p"),
                "GOPATH/l:USERPROFILE/p:COLORTERM:TERM_PROGRAM",
            ),
            (Some("COLORTERM/u"), "COLORTERM/u:TERM_PROGRAM"),
        ];
        for (existing, expected) in cases {
            assert_eq!(wslenv_with(existing), expected, "{existing:?}");
        }
    }

    #[test]
    fn env_matches_spec() {
        let env = pty_options(&ShellConfig::default()).env;
        assert_eq!(env["TERM"], "xterm-256color");
        assert_eq!(env["COLORTERM"], "truecolor");
        assert_eq!(env["TERM_PROGRAM"], "jterm");
    }

    /// The Step 4 acceptance path without a window: spawn a real shell, see output
    /// arrive, write `exit\r`, and see the session report that the shell exited.
    #[test]
    fn spawned_shell_produces_output_and_exits_on_command() {
        // A plain, fast shell rather than the user's (which may load a slow profile).
        let config = Config {
            shell: if cfg!(windows) {
                shell(Some("cmd.exe"), &[])
            } else {
                shell(Some("/bin/sh"), &[])
            },
            ..Config::default()
        };
        let size = TermSize {
            cols: 80,
            rows: 24,
            cell_width: 8,
            cell_height: 16,
        };
        let mut session =
            TerminalSession::spawn(&config, size, egui::Context::default()).expect("spawn shell");

        let deadline = Instant::now() + Duration::from_secs(20);
        let (mut woke, mut sent_exit) = (false, false);
        while !session.exited && Instant::now() < deadline {
            woke |= session
                .poll_events()
                .iter()
                .any(|e| matches!(e, Event::Wakeup));
            if woke && !sent_exit {
                session.write(&b"exit\r"[..]);
                sent_exit = true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(woke, "no Wakeup event: the shell produced no output");
        assert!(session.exited, "shell did not exit after `exit\\r`");
    }

    /// SPEC Step 6: Ctrl+C (0x03) must interrupt a running program. On Windows this
    /// depends on `enable_ctrl_c_for_children`; on Unix on the PTY line discipline.
    #[test]
    fn ctrl_c_interrupts_a_running_program() {
        let (program, args) = if cfg!(windows) {
            ("cmd.exe", ["/c", "ping -n 30 127.0.0.1"])
        } else {
            ("/bin/sh", ["-c", "sleep 30"])
        };
        let config = Config {
            shell: shell(Some(program), &args),
            ..Config::default()
        };
        let size = TermSize {
            cols: 80,
            rows: 24,
            cell_width: 8,
            cell_height: 16,
        };
        let mut session =
            TerminalSession::spawn(&config, size, egui::Context::default()).expect("spawn");
        // Let the program start; a ^C before it runs would hit the shell instead.
        std::thread::sleep(Duration::from_millis(1500));
        session.write(&b"\x03"[..]);

        let deadline = Instant::now() + Duration::from_secs(10);
        while !session.exited && Instant::now() < deadline {
            session.poll_events();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(session.exited, "^C did not stop the program");
    }
    /// SPEC Step 7: a resize reaches both the grid and the shell. The shell is asked for its
    /// own idea of the size, so this checks the PTY (ConPTY / TIOCSWINSZ), not just `Term`.
    #[test]
    fn resize_reaches_the_shell() {
        let program = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
        let config = Config {
            shell: shell(Some(program), &[]),
            ..Config::default()
        };
        let size = |cols, rows| TermSize {
            cols,
            rows,
            cell_width: 8,
            cell_height: 16,
        };
        let mut session =
            TerminalSession::spawn(&config, size(80, 24), egui::Context::default()).expect("spawn");
        session.resize(size(100, 30));
        assert_eq!(session.size(), size(100, 30));
        {
            let term = session.term.lock();
            assert_eq!((term.columns(), term.screen_lines()), (100, 30));
        }

        // `mode con` (cmd) and `stty size` (sh) print the size the shell's console has.
        let query: &[u8] = if cfg!(windows) {
            b"mode con\r"
        } else {
            b"stty size\r"
        };
        std::thread::sleep(Duration::from_millis(500));
        session.write(query);
        let expected: &[&str] = if cfg!(windows) {
            &["Lines:          30", "Columns:        100"]
        } else {
            &["30 100"]
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut screen = String::new();
        while Instant::now() < deadline {
            session.poll_events();
            screen = session
                .snapshot(&crate::config::theme::Theme::default())
                .cells
                .iter()
                .map(|c| c.c)
                .collect();
            if expected.iter().all(|e| screen.contains(e)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "shell didn't report the new size {expected:?}; screen:\n{}",
            screen.trim()
        );
    }
    #[test]
    fn viewport_rows_map_to_history_lines_when_scrolled() {
        use alacritty_terminal::event::VoidListener;
        use alacritty_terminal::grid::Scroll;
        use alacritty_terminal::vte::ansi::Processor;

        let mut term = Term::new(
            term::Config::default(),
            &TermSize {
                cols: 10,
                rows: 4,
                cell_width: 8,
                cell_height: 16,
            },
            VoidListener,
        );
        let lines: String = (1..=20).map(|i| format!("{i}\r\n")).collect();
        Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new()
            .advance(&mut term, lines.as_bytes());
        assert_eq!(
            viewport_to_grid(&term, 2, 1),
            Point::new(Line(1), Column(2))
        );
        term.scroll_display(Scroll::Delta(5));
        assert_eq!(
            viewport_to_grid(&term, 2, 1),
            Point::new(Line(-4), Column(2))
        );
        // Columns past the grid clamp to the last one.
        assert_eq!(viewport_to_grid(&term, 99, 0).column, Column(9));
    }
}
