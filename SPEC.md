# Jasmine Terminal — Specification

> Working name: **Jasmine Terminal** (binary: `jterm`). Rename freely; keep the binary name short.
> A fast, local-first, no-login terminal with Warp-style blocks and a modern input editor, built in Rust with egui.

---

## How to use this document (for Claude Code)

1. **Build the MVP first.** The [MVP](#mvp) section is an ordered list of steps. Do them in order. Each step has acceptance criteria. Do not start a step until the previous step's criteria pass.
2. **Do not implement post-MVP features during the MVP**, but keep the architecture compatible with them. The [Full Feature Spec](#full-feature-spec) says where the project is heading, so module boundaries and data structures should not block those features later.
3. **Never write a VT/ANSI parser or terminal grid from scratch.** Use `alacritty_terminal`. Terminal emulation correctness is a solved problem; the product value is the UX layer.
4. **Check current crate versions on crates.io** before adding dependencies. Do not assume versions from memory. Prefer the versions `egui_term` currently uses for `alacritty_terminal`/`egui` compatibility.
5. After each step: `cargo build`, `cargo clippy -- -D warnings`, `cargo fmt --check`, run relevant tests, then commit with a message like `mvp(step-3): render terminal grid`.
6. If something in this spec turns out to be wrong or impossible with the chosen crates, stop, explain the conflict, and propose an alternative instead of silently deviating.

---

## 1. Overview

### 1.1 Problem
Warp popularized a better terminal UX (command blocks, editor-style input, palette, AI), but it requires an account, leans on cloud features, and its performance/stability has degraded. Traditional fast terminals (Alacritty, Ghostty, WezTerm) are excellent emulators but keep the 1980s interaction model.

### 1.2 Product
A native terminal that:
- Is as correct as Alacritty (because it uses Alacritty's core).
- Groups each command and its output into a **block**.
- Offers an **editor-style input** (multi-line, mouse, selections, highlighting).
- Works fully **offline, with no account and no telemetry**.
- Treats AI as **optional and bring-your-own-key / local model**.

### 1.3 Goals
- Correct: vim, nvim, tmux, htop, less, ssh, fzf, git TUIs all work.
- Fast: startup < 150 ms, input latency comparable to Alacritty, smooth scrolling through 100k lines of scrollback.
- Local-first: all config, history and data stored locally in plain files.
- Pleasant: polished look, sane defaults, keyboard-driven.

### 1.4 Non-goals
- Writing our own terminal emulator core.
- Accounts, cloud sync, collaboration, telemetry.
- Being a full IDE or multiplexer replacement (tmux keeps working inside it).
- Windows support in v1 (keep code portable, but don't test or ship for Windows initially).

### 1.5 Target platforms
- **Primary:** Linux (X11 + Wayland).
- **Secondary:** macOS.
- **Later:** Windows (ConPTY).

---

## 2. Tech Stack

| Concern | Choice | Notes |
|---|---|---|
| Language | Rust (stable, edition 2021+) | |
| Windowing / UI | `eframe` + `egui` | Immediate-mode; same stack as other Jasmine apps |
| Terminal core | `alacritty_terminal` | Grid, VT parsing, scrollback, selection, alt screen, PTY (`tty` module), I/O `EventLoop` |
| Reference implementation | `egui_term` | Study or fork its rendering/input approach; keep versions compatible |
| PTY fallback | `portable-pty` | Only if `alacritty_terminal::tty` proves insufficient |
| Config | `serde` + `toml` | |
| Paths | `directories` | XDG on Linux, standard dirs on macOS |
| Logging | `tracing` + `tracing-subscriber` | Log file in data dir, level via env/config |
| Errors | `anyhow` (app), `thiserror` (library modules) | |
| Clipboard | `arboard` (or egui's built-in clipboard) | |
| Fonts | Bundle JetBrains Mono (OFL) as default | Allow user font path in config |
| Later: GPU renderer | `wgpu` glyph atlas | Only if egui text rendering becomes a bottleneck |
| Later: history store | `rusqlite` | For block/command history search |
| Later: fuzzy matching | `nucleo` or `fuzzy-matcher` | Command palette, history search |
| Later: syntax highlight | `tree-sitter-bash` or a lightweight tokenizer | Input editor |

---

## 3. Architecture

### 3.1 High-level data flow

```
 keyboard/mouse ──▶ egui UI ──▶ InputRouter ──┬──▶ PTY writer ──▶ shell process
                                             │
                                             └──▶ InputEditor (post-MVP)

 shell process ──▶ PTY reader (alacritty EventLoop thread)
                     │
                     ▼
            Term<EventProxy> (Arc<FairMutex<..>>) ──▶ grid snapshot ──▶ TerminalView renders
                     │
                     └──▶ events (Title, Bell, Exit, Wakeup, ClipboardStore, PtyWrite)
                              │
                              ▼
                       EventProxy ──▶ channel ──▶ App (request_repaint, update tab title, etc.)
```

### 3.2 Threading
- **UI thread:** eframe event loop. Locks the `Term` briefly to snapshot visible cells, then renders. Never blocks on I/O.
- **PTY I/O thread:** `alacritty_terminal::event_loop::EventLoop` reads the PTY and feeds the parser. It calls our `EventListener` on events; our listener sends them through a channel and calls `ctx.request_repaint()`.
- Keep lock hold times short. Do not hold the `Term` lock while shaping text or doing layout work if avoidable; copy what's needed into a lightweight render snapshot first.

### 3.3 Crate / module layout

Single binary crate for MVP, organized so modules can later be split into a workspace:

```
jterm/
├─ Cargo.toml
├─ assets/
│  └─ fonts/JetBrainsMono-Regular.ttf (+ Bold, Italic, BoldItalic)
├─ shell-integration/        # post-MVP (Phase 1)
│  ├─ jterm.zsh
│  ├─ jterm.bash
│  └─ jterm.fish
└─ src/
   ├─ main.rs                # arg parsing, logging init, eframe launch
   ├─ app.rs                 # JtermApp: implements eframe::App, owns tabs, routes events
   ├─ config/
   │  ├─ mod.rs              # Config struct, load/save, defaults
   │  └─ theme.rs            # Color palette (16 ANSI + fg/bg/cursor/selection)
   ├─ terminal/
   │  ├─ mod.rs
   │  ├─ session.rs          # TerminalSession: spawns PTY + EventLoop, owns Term, resize, write
   │  ├─ event_proxy.rs      # EventListener impl -> channel + repaint
   │  └─ snapshot.rs         # Copy of visible cells/cursor/selection for rendering
   ├─ ui/
   │  ├─ mod.rs
   │  ├─ terminal_view.rs    # Renders snapshot, handles input/mouse/selection/scroll
   │  ├─ cell_metrics.rs     # Font metrics: cell width/height, baseline
   │  └─ input_map.rs        # egui key events -> bytes/escape sequences
   ├─ blocks/                # post-MVP (Phase 1)
   ├─ editor/                # post-MVP (Phase 2)
   └─ palette/               # post-MVP (Phase 3)
```

### 3.4 Key types (MVP)

```rust
pub struct TerminalSession {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    notifier: Notifier,            // alacritty_terminal::event_loop::Notifier, used to write to PTY
    size: TermSize,                // cols/rows
    events_rx: Receiver<Event>,    // from EventProxy
    pub title: String,
    pub exited: bool,
}

impl TerminalSession {
    pub fn spawn(config: &Config, size: TermSize, ctx: egui::Context) -> anyhow::Result<Self>;
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>);
    pub fn resize(&mut self, cols: u16, rows: u16, cell_w: f32, cell_h: f32);
    pub fn poll_events(&mut self) -> Vec<Event>;
    pub fn snapshot(&self) -> RenderSnapshot;
}
```

Exact signatures may change to fit the `alacritty_terminal` API; keep the responsibilities the same.

---

## 4. Full Feature Spec

Each feature lists its phase. **MVP features are expanded step-by-step in the [MVP](#mvp) section.**

### 4.1 Terminal core — *MVP*
- Spawn the user's login shell (`$SHELL`, fallback `/bin/bash`) in a PTY, with `TERM=xterm-256color`, `COLORTERM=truecolor`, and `TERM_PROGRAM=jterm`.
- Full VT emulation via `alacritty_terminal`: colors (16/256/truecolor), bold/italic/underline/inverse/dim/strikethrough, alternate screen, cursor shapes, bracketed paste, mouse reporting, focus events.
- Scrollback (default 10,000 lines, configurable).
- Resize on window resize (PTY gets `TIOCSWINSZ` via alacritty's resize).
- Window title from OSC 0/2.
- Bell: visual flash (configurable off).
- Close window when the shell exits (configurable: close / keep with "[process exited]").

### 4.2 Rendering — *MVP (basic), Phase 4 (advanced)*
- Monospace grid rendering with egui painter: per-cell background rects, text runs batched by style.
- Cursor: block / beam / underline, blinking optional; hollow block when unfocused.
- Selection highlight.
- Wide characters (CJK) occupy 2 cells; emoji fallback font.
- **Phase 4:** font ligatures, custom `wgpu` glyph-atlas renderer if profiling shows egui is a bottleneck, background image/opacity, padding, smooth scroll.

### 4.3 Input — *MVP*
- Map egui key events to correct byte sequences: printable text, Enter, Backspace (DEL `0x7f`), Tab, Esc, arrows, Home/End, PgUp/PgDn, Insert/Delete, F1–F12, Ctrl+letter (C0 control codes), Alt as ESC-prefix.
- Respect application cursor mode (DECCKM) for arrow keys (`ESC O A` vs `ESC [ A`).
- IME / composed text input via egui `Event::Text`.
- Paste: bracketed paste when the terminal mode is enabled.

### 4.4 Mouse & selection — *MVP*
- Click-drag selection, double-click word, triple-click line.
- Copy: `Ctrl+Shift+C` (Linux), `Cmd+C` (macOS). Paste: `Ctrl+Shift+V` / `Cmd+V`.
- Mouse wheel scrolls scrollback; when the app enables mouse reporting (e.g., vim with `mouse=a`), forward mouse events instead. Shift overrides to force local selection.
- In alternate screen without mouse reporting, wheel sends arrow keys (like other terminals).

### 4.5 Configuration — *MVP (basic), expanded later*
- File: `~/.config/jterm/config.toml` (via `directories`). Created with defaults on first run.
- MVP keys: `font.family`/`font.path`, `font.size`, `shell.program`, `shell.args`, `scrollback.lines`, `theme` (inline palette), `cursor.style`, `cursor.blink`, `window.padding`, `bell.visual`.
- Later: hot reload on file change, named theme files in `~/.config/jterm/themes/`, keybinding overrides.

### 4.6 Tabs & splits — *Phase 3*
- Tabs with titles from OSC title or current command; `Ctrl+Shift+T` new tab, `Ctrl+Shift+W` close, `Ctrl+Tab` switch.
- Splits: horizontal/vertical, focus movement by keyboard, drag to resize.
- New tab/split inherits the current working directory (from OSC 7, emitted by shell integration).

### 4.7 Shell integration & Blocks — *Phase 1 (the headline feature)*
- Ship shell integration scripts for **zsh, bash, fish** that emit:
  - **OSC 133** semantic prompt marks: `A` (prompt start), `B` (command start / prompt end), `C` (command executed / output start), `D;<exit_code>` (command finished).
  - **OSC 7** current working directory: `file://host/path`.
- Auto-inject integration: e.g., for zsh set `ZDOTDIR` to a temp dir whose `.zshrc` sources the user's real config then ours; for bash use `--rcfile`; for fish use `--init-command`. Must be disable-able in config. Do not modify user dotfiles.
- `alacritty_terminal` may not expose OSC 133 directly. Implement a lightweight byte-stream tap before the parser (or a custom handler) that detects these sequences and records their grid line positions. Document the chosen approach in code.
- **Block model:** each block stores: command text, cwd, start/end time, exit code, start line and end line in scrollback, collapsed flag.
- **Block UI:** subtle separators between blocks, header with command, duration, exit status (green/red marker), hover actions: copy command, copy output, rerun, collapse.
- Click to select a block; keyboard navigation between blocks (`Ctrl+Up/Down`).
- When a full-screen app uses the alternate screen (vim, htop), blocks are hidden; normal grid rendering applies.
- Graceful degradation: without integration (e.g., inside ssh or an unsupported shell) the terminal works like a normal terminal.

### 4.8 Editor-style input — *Phase 2*
- When the shell is at a prompt (between OSC 133 `B` and `C`), the bottom input area becomes a custom text editor: multi-line, mouse cursor placement, selection, undo/redo, standard shortcuts.
- Syntax highlighting of the command (commands, flags, strings, paths, pipes).
- On Enter: send the command text + `\r` to the PTY. Shift+Enter inserts a newline.
- Tab completion: initially pass through to the shell (send current buffer + Tab, read back). This is hard; spec it as an investigation task, with a fallback of disabling the custom editor for completion.
- Automatically hand control back to raw terminal mode whenever a command is running, a full-screen app is active, or no prompt mark is present. Raw mode must always be available via a toggle shortcut.
- History: Up/Down in the editor cycles local command history.

### 4.9 Command palette & history search — *Phase 3*
- `Ctrl+Shift+P`: fuzzy palette of actions (new tab, split, theme switch, open config, toggle editor mode, etc.).
- `Ctrl+R` override (optional): fuzzy search over block history stored in SQLite (`~/.local/share/jterm/history.db`) with command, cwd, exit code, timestamp, duration.
- Search in scrollback: `Ctrl+Shift+F`, highlight matches, next/prev.

### 4.10 Workflows / snippets — *Phase 3*
- User-defined parameterized commands in `~/.config/jterm/workflows.toml`, e.g. `docker logs -f {{container}}`. Insert via palette, tab through parameters.

### 4.11 AI (optional) — *Phase 5*
- Disabled by default. No network calls unless the user configures a provider.
- Providers: OpenAI-compatible HTTP endpoint (covers Ollama, LM Studio, and hosted APIs) and Anthropic API, with the key in config or an env var.
- Features: natural language → command (shown in the editor for review, never auto-executed), "explain this error" on a failed block, summarize block output.
- Context sent is shown to the user and limited to the selected block(s); nothing is sent implicitly.

### 4.12 Links & smart selection — *Phase 4*
- Detect URLs and file paths (`path:line:col`); Ctrl+click opens them. OSC 8 hyperlinks supported.

### 4.13 Notifications — *Phase 4*
- Desktop notification when a long-running command (> configurable threshold) finishes while the window is unfocused.

### 4.14 Packaging & distribution — *Phase 6*
- Linux: AppImage, `.deb`, AUR PKGBUILD, Flatpak (note Flatpak sandbox constraints for spawning host shells; use `flatpak-spawn --host`). Desktop file + icon.
- macOS: `.app` bundle via `cargo-bundle`, notarization later.
- Install `jterm` terminfo optional; default to `xterm-256color` to avoid ssh issues.

### 4.15 Performance targets
| Metric | Target |
|---|---|
| Cold start to first prompt | < 150 ms |
| `cat` a 100 MB file | Does not freeze UI; throughput within 2× of Alacritty |
| Idle CPU | ~0% (repaint only on events, cursor blink timer) |
| Memory with 10k scrollback | < 100 MB |
| Frame time while scrolling | < 8 ms on integrated GPU |

Rule: **repaint only when needed** (PTY wakeup events, input, cursor blink). Never run a continuous repaint loop.

### 4.16 Privacy & security
- No telemetry, no account, no auto-update pings in v1.
- Clipboard writes requested by programs (OSC 52) are allowed only if enabled in config (default: allow write, deny read).
- Never log keystrokes or terminal content at default log level.

---

<a id="mvp"></a>
## 5. MVP

**Definition of done for the MVP:** a single-window terminal that runs the user's shell, renders correctly with colors, handles keyboard, mouse selection, copy/paste, scrollback and resizing, and in which **vim, htop, tmux, less and ssh work correctly**. No blocks, no custom editor, no tabs, no AI.

### Step 1 — Project scaffold
- `cargo new jterm`, set up the module layout from §3.3 (empty modules are fine).
- Add dependencies: `eframe`, `egui`, `alacritty_terminal`, `serde`, `toml`, `directories`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`. Check versions against `egui_term`'s `Cargo.toml` for compatibility.
- `main.rs` initializes logging (to stderr in debug builds) and opens an empty eframe window titled "jterm", 1000×650.
- Add `rustfmt.toml` (defaults are fine) and a CI-ready `Makefile` or `justfile` with `fmt`, `lint`, `test`, `run`.

**Accept:** `cargo run` opens an empty window; `cargo clippy -- -D warnings` passes.

### Step 2 — Config loading
- `Config` struct with defaults for all MVP keys from §4.5. Default theme: a tasteful dark palette (define all 16 ANSI colors + foreground, background, cursor, selection).
- Load from `~/.config/jterm/config.toml`; if missing, write defaults. Unknown keys warn instead of failing. A malformed file logs an error and falls back to defaults.
- Bundle JetBrains Mono under `assets/fonts/` with `include_bytes!`, and register it with egui as the monospace family.

**Accept:** unit tests for default config, parse of a sample TOML, and fallback on a malformed file. The window still opens.

### Step 3 — Cell metrics
- `cell_metrics.rs`: compute cell width and height from the configured font and size using egui's font layout (measure an `M` glyph or the font's advance width; line height from row height). Account for `pixels_per_point`.
- Function to convert an available rect into `(cols, rows)` minus padding.

**Accept:** unit test for the rect → cols/rows conversion; logging shows sensible values at startup.

### Step 4 — Spawn the shell
- `TerminalSession::spawn`: create the PTY with `alacritty_terminal::tty::new` using shell/args from config, env vars from §4.1, working dir = `$HOME`.
- Create `Term` with the initial size and scrollback from config, wrap it in `Arc<FairMutex<..>>`, start the `EventLoop` and keep its `Notifier` for writing.
- `EventProxy` implements `EventListener`: sends events through a `std::sync::mpsc` (or `crossbeam`) channel and calls `ctx.request_repaint()`.
- Handle `Event::Exit`/child exit: mark the session exited and close the window (default behaviour).

**Accept:** logging shows the shell spawned and `Wakeup` events arrive. Exiting the shell (via a debug write of `exit\r`) closes the app.

### Step 5 — Render the grid
- `snapshot.rs`: under the lock, copy visible cells (char, fg, bg, flags), cursor position/shape and display offset into a `RenderSnapshot`. Resolve colors (named, indexed 256, RGB) against the theme here.
- `terminal_view.rs`: allocate the available rect, paint the background, then per line: paint background rects for runs of cells with non-default bg, and paint text for runs of identical style (batch runs; do not paint one galley per cell). Handle bold (bold font or brighter color), italic, underline, strikethrough, inverse, dim, hidden.
- Wide chars: render once, spanning 2 cells; skip the spacer cell.
- Draw the cursor per shape; hollow block when the window is unfocused.

**Accept:** the shell prompt renders with correct colors. `ls --color` shows colors. Running `for i in {0..255}; do printf "\e[38;5;${i}m%3d " $i; done` shows a correct 256-color palette. A truecolor test script renders a smooth gradient.

### Step 6 — Keyboard input
- `input_map.rs`: translate egui `Event::Text` and `Event::Key` into bytes per §4.3, including Ctrl combinations (Ctrl+C → `0x03`, etc.), Alt-prefix, and application cursor mode (read `TermMode` from the term).
- Reserve app shortcuts (copy/paste) before forwarding to the PTY.
- When the user types, reset scroll to the bottom.

**Accept:** typing commands works; Ctrl+C interrupts `sleep 100`; Ctrl+D exits; arrows navigate shell history; Tab completes; vim opens, moves with arrows and hjkl, and `:wq` works.

### Step 7 — Resize
- Every frame, compute cols/rows from the available rect; on change call `TerminalSession::resize`, which resizes both the `Term` and the PTY via the notifier.
- Debounce isn't required, but avoid resizing on sub-cell pixel changes.

**Accept:** resizing the window reflows the prompt; `tput cols; tput lines` reports the new size; htop and vim redraw correctly after resize.

### Step 8 — Scrollback & mouse wheel
- Mouse wheel scrolls the scrollback (`term.scroll_display`), with sensible lines per notch.
- If mouse reporting mode is active, send wheel events to the PTY instead. In alternate screen without mouse mode, send arrow keys.
- `Shift+PgUp/PgDn` scroll by page.

**Accept:** `seq 1 20000` then scrolling up shows earlier output; scrolling in `less` and vim works; new output while scrolled up does not jump the view until the user types.

### Step 9 — Selection, copy & paste
- Mouse drag creates a selection via `alacritty_terminal`'s `Selection` (simple, semantic for double-click, lines for triple-click). Convert pixel position to grid point, accounting for display offset.
- Render selection highlight using the theme's selection color.
- Copy shortcut copies `term.selection_to_string()` to the clipboard. Paste reads the clipboard and writes it to the PTY, wrapped in bracketed paste sequences when that mode is on.
- Mouse events are forwarded to the app when mouse reporting is active, unless Shift is held.

**Accept:** select text and paste it into another app; paste multi-line text into zsh (with bracketed paste) without auto-executing; in vim with `:set mouse=a`, clicking moves the cursor.

### Step 10 — Title, bell, polish
- Apply OSC title changes to the window title (`ctx.send_viewport_cmd(ViewportCommand::Title(..))`).
- Visual bell: brief overlay flash, if enabled.
- Cursor blink with a timer-based repaint (`request_repaint_after`), stopped when not focused.
- Window padding from config.
- Verify idle CPU is ~0%.

**Accept:** `echo -e "\e]0;hello\a"` changes the title; `echo -e "\a"` flashes; idle CPU near 0% in `top`.

### Step 11 — Compatibility pass
Manually test and fix issues in each:
- [ ] vim / nvim (insert mode, visual mode, colorschemes, mouse)
- [ ] htop / btop
- [ ] tmux (splits, status bar colors, mouse mode)
- [ ] less / man
- [ ] ssh to a remote host, run vim there
- [ ] fzf (Ctrl+R in shell)
- [ ] lazygit or tig
- [ ] `cat` a large file (e.g., 50 MB): UI stays responsive
- [ ] Unicode: CJK text, emoji, box-drawing characters align on the grid
- [ ] Wayland and X11 both launch

**Accept:** every box is checked or has a filed issue in `KNOWN_ISSUES.md` with a reason.

### Step 12 — MVP release hygiene
- `README.md`: what it is, build instructions, config reference, known issues.
- `cargo build --release` with `lto = "thin"`, `codegen-units = 1`, `strip = true`.
- Tag `v0.1.0`.

**MVP complete.**

---

## 6. Post-MVP Roadmap

| Phase | Theme | Contents (see §4) |
|---|---|---|
| 1 | **Blocks** | Shell integration scripts + auto-injection, OSC 133/7 parsing, block model, block UI and actions (§4.7) |
| 2 | **Input editor** | Editor-style prompt input, highlighting, raw-mode fallback, local history (§4.8) |
| 3 | **Productivity** | Tabs & splits, command palette, history DB + fuzzy search, scrollback search, workflows (§4.6, §4.9, §4.10) |
| 4 | **Polish** | Ligatures, links/OSC 8, notifications, hot-reload config, theme files, renderer optimization or `wgpu` (§4.2, §4.12, §4.13) |
| 5 | **AI (optional)** | BYO-key / local model integration (§4.11) |
| 6 | **Distribution** | Packaging for Linux and macOS (§4.14) |

Each phase should get its own step-by-step breakdown, written in the same style as §5, before implementation begins.

---

## 7. Testing Strategy

- **Unit tests:** config parsing, cell metrics math, key → bytes mapping (table-driven tests covering every key in §4.3 in normal and application cursor modes), color resolution, OSC 133 detection (Phase 1).
- **Integration tests:** spawn a session running `printf` scripts headlessly (no window) and assert grid contents via `Term` APIs, e.g. colors, cursor position after escape sequences, resize behavior.
- **Manual compatibility checklist:** §5 Step 11, re-run before every release.
- **Performance checks:** a `scripts/bench.sh` that times `cat` of a large file and `seq 1 1000000`; compare against Alacritty on the same machine.
- **CI:** GitHub Actions on Linux (and macOS later): fmt, clippy, test, release build.

---

## 8. Conventions

- No `unwrap()` in non-test code except where an invariant is documented in a comment; prefer `?` and `anyhow::Context`.
- Keep the `Term` lock scope minimal; never call into egui painting while holding it.
- Public functions and modules get a short doc comment explaining *why*, not just *what*.
- Feature flags for experimental subsystems (`blocks`, `editor`, `ai`) once they exist, so the core terminal can always be built alone.
- Commit style: `mvp(step-N): ...`, then `phase1: ...`, etc.

---

## 9. Open Questions

1. Final product name and icon (Jasmine Studios branding).
2. Does egui text rendering hold the performance targets, or is a `wgpu` renderer needed earlier than Phase 4? Decide after Step 11 profiling.
3. Best hook point for OSC 133 detection given `alacritty_terminal`'s API: byte-stream tap before the parser vs. a fork/patch. Prototype in early Phase 1.
4. Tab completion strategy for the custom input editor (Phase 2) is the biggest UX risk; prototype before committing to the design.
5. Licensing: MIT/Apache-2.0 dual license (compatible with `alacritty_terminal`, which is Apache-2.0).
