//! `TerminalSession`: spawns the shell in a PTY, runs the I/O event loop, and handles writes.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::event_loop::{self, EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term, TermMode};
use alacritty_terminal::tty;
use anyhow::Context as _;
use tracing::{info, warn};

use super::event_proxy::EventProxy;
use super::snapshot::{Palette, RenderSnapshot};
use crate::config::theme::Theme;
use crate::config::{Config, ShellConfig};
use crate::ui::input_map::InputModes;

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
    pub fn input_modes(&self) -> InputModes {
        let mode = *self.term.lock().mode();
        InputModes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        }
    }

    /// Sends user input to the shell. Like other terminals, typing jumps back to the
    /// bottom if the view was scrolled into history.
    pub fn send_input(&self, bytes: Vec<u8>) {
        {
            let mut term = self.term.lock();
            if term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
            }
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

/// PTY options from config plus the environment SPEC §4.1 requires. The variables go
/// through the PTY options rather than `std::env::set_var` on our own process.
fn pty_options(shell: &ShellConfig) -> tty::Options {
    let env = HashMap::from([
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ("TERM_PROGRAM".to_owned(), "jterm".to_owned()),
    ]);
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
}
