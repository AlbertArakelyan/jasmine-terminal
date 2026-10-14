# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

MVP Steps 1–5 are done: config, the bundled font, cell metrics, the shell in a PTY, and grid rendering. `cargo run` opens a 1000×650 window showing the shell (PowerShell 7 on this machine) with full colors and text attributes. There's no keyboard input yet (Step 6), and the terminal isn't resized with the window yet (Step 7). The window closes when the shell exits. Modules for later steps are still empty stubs. **`SPEC.md` is the source of truth**: it defines the product (Jasmine Terminal, binary `jterm`, a Warp-style terminal built on Rust + egui + `alacritty_terminal`), the target architecture, and an ordered MVP plan (§5, Steps 1–12) with acceptance criteria per step. Read the relevant SPEC section before starting any work.

Notes:
- The package is named `jasmine-terminal`, but a `[[bin]]` entry builds it as `jterm` (`target/debug/jterm.exe`).
- Dependencies are on their latest releases: `egui`/`eframe` 0.36 and `alacritty_terminal` 0.26. eframe 0.36 needs Rust 1.95 or newer, so `Cargo.toml` sets `rust-version = "1.95"`. eframe uses `App::ui(&mut self, ui, frame)`, not the older `update(ctx, frame)`.
- Development happens on Windows, but the spec targets Linux first, then macOS, and treats Windows (ConPTY) as a v1 non-goal. Keep code portable. Flag any PTY or platform behaviour that can't be checked on this machine instead of assuming it works.
- **Shell choice (decided with the user, deviates from SPEC §4.1):**
  - `shell.program` always wins when set.
  - Windows default: `pwsh.exe` if it's on `PATH`, otherwise `powershell.exe`. Set `shell.program` to Git Bash to run the bash VT test scripts.
  - Unix with no `shell.program`: we pass `shell: None`, and `alacritty_terminal` uses `$SHELL`, falling back to the passwd entry, started as a login shell (via `login` on macOS). That's instead of the SPEC's `/bin/bash` fallback. `shell.args` is ignored in this case.

## Workflow rules from SPEC.md

- Implement MVP steps **in order**. Don't start a step until the previous step's acceptance criteria pass. Don't build post-MVP features (blocks, input editor, tabs, palette, AI), but don't block them either: follow the module layout in SPEC §3.3.
- **Never write a VT/ANSI parser or terminal grid.** Use `alacritty_terminal` (its `tty` module for the PTY, `event_loop::EventLoop` for I/O). Study `egui_term` as the reference implementation.
- Before adding a dependency, look up its current version on crates.io and use the latest release. Don't rely on versions from memory. `egui_term` doesn't constrain our versions. It may target an older `egui`, so adapt any code borrowed from it to our egui API.
- If the spec turns out to be wrong or impossible with the chosen crates, stop, explain the conflict and propose an alternative. Don't deviate silently.
- After each step, run the checks below, then commit as `mvp(step-N): <summary>` (later phases use `phase1: ...` and so on). Commit directly on `main`; don't create feature branches unless asked.

## Commands

```sh
cargo build
cargo run
cargo clippy -- -D warnings     # must pass
cargo fmt --check               # must pass
cargo test
cargo test <name_substring>     # run a single test / filter by name
cargo build --release
```

A `justfile` wraps these: `just` (lint + test), `just fmt`, `just lint` (fmt check + clippy with `--all-targets`), `just test`, `just run`. `just` is not installed on this machine yet (`scoop install just`), so run the cargo commands directly until it is. Debug builds log to stderr; set `RUST_LOG` to change the level (default `info,wgpu_hal=error`).

## Architecture (per SPEC §3; rendering and input not built yet)

- **Two threads.** The eframe UI thread and alacritty's PTY I/O `EventLoop` thread share `Arc<FairMutex<Term<EventProxy>>>`.
- **`EventProxy`** implements alacritty's `EventListener`. It forwards events (Wakeup, Title, Bell, Exit, ClipboardStore, PtyWrite) over a channel to the app and calls `ctx.request_repaint()`.
- **Rendering.** The UI locks `Term` briefly to copy visible cells, cursor and selection into a `RenderSnapshot` (`terminal/snapshot.rs`, where colors are resolved against the theme), releases the lock, then paints. Never hold the `Term` lock while painting or laying out text.
- **Painting** batches runs of identically styled cells. Never create one galley per cell. Wide characters span 2 cells, and the spacer cell is skipped.
- **Repaint only on events** (PTY wakeup, input, cursor-blink timer via `request_repaint_after`). No continuous repaint loop; idle CPU should be about 0%.
- **Input** (`ui/input_map.rs`) maps egui events to bytes and must respect `TermMode` (DECCKM application cursor mode, bracketed paste, mouse reporting; Shift overrides mouse reporting for local selection).
- **Config** (`config/`) is TOML at the `directories` config path: `~/.config/jterm/config.toml` on Linux, `%APPDATA%\jterm\config\config.toml` on Windows. A missing file is created with commented defaults. Every section and key is optional, and missing ones take their defaults (`#[serde(default)]`). Unknown keys are only warnings (reported via `serde_ignored`). A malformed file or invalid value logs an error and the whole config falls back to defaults, without overwriting the file. `Config::load()` never fails. Colors are `"#rrggbb"` strings (`theme::Rgb`). The normal and bright ANSI sets are separate types, so each fills in its own defaults.
- **Fonts** (`ui/fonts.rs`): JetBrains Mono (OFL, `assets/fonts/`) is embedded with `include_bytes!` and installed as egui's `Monospace` family. Bold, italic and bold-italic are registered as named families (`fonts::BOLD_FAMILY` etc.), because egui has no font weights or styles. `font.path` replaces only the regular face. Fonts can't be looked up by family name.
- **Cell metrics** (`ui/cell_metrics.rs`): cell width is the advance of `M`, height is egui's `row_height`, both snapped to whole physical pixels so glyphs stay on the pixel grid. egui has no fonts before the first frame, so `JtermApp` measures lazily in `ui()` and re-measures when `pixels_per_point` changes. `grid_size(rect, padding)` floors with a small epsilon and is clamped to at least 1×1. Headless tests can measure with a real `egui::Context` (see `measure_headless`); they must call `textures_delta.clear()` on the output or egui's debug assert panics.
- **Session** (`terminal/session.rs`):
  - `TerminalSession::spawn` runs on the first frame, once the grid size is known.
  - `TERM`, `COLORTERM` and `TERM_PROGRAM` go through `tty::Options.env`, never `std::env::set_var`.
  - The working directory is the home dir. `drain_on_exit` is on.
  - When the shell exits, `alacritty_terminal` sends `ChildExit(status)`, drains the last output, then sends `Exit`. `session.exited` is set on `Exit`, so the final output isn't lost, or when the I/O thread has finished, because a PTY read or write error stops it without sending `Exit`. The app then sends `ViewportCommand::Close` once.
  - `EventProxy` writes `PtyWrite` replies (DA, DSR/cursor position, DECRQM) straight to the PTY from the I/O thread; they never reach the UI. fish, vim and crossterm apps hang without them. It also coalesces `Wakeup`s to one queued at a time; `poll_events` clears the flag. Not answered yet: `ColorRequest` (Step 5, from the theme) and `TextAreaSizeRequest` (Step 7). `ClipboardLoad` is ignored on purpose (SPEC §4.16).
  - **Lock order:** `EventProxy` takes egui's context lock while the PTY thread may hold the `Term` lock. Never lock `Term` inside an egui context closure (`ctx.input(|i| ..)`, `ui.input(..)`, `ctx.fonts(..)`, `ctx.memory(..)`), or the threads can deadlock.
  - Dropping the session sends `Msg::Shutdown`. Closing the window also ends the shell and its conhost processes, as verified on Windows.
  - A spawn failure is logged and not retried.
- **`JTERM_DEBUG_EXIT_AFTER_MS=N`** (debug builds only) writes `exit\r` to the shell N ms after it spawns, to test the exit path without keyboard input: `JTERM_DEBUG_EXIT_AFTER_MS=1500 pwsh -NoProfile -File .claude/skills/verify/scripts/smoke.ps1 -Seconds 6`. The smoke script then reports `FAIL: process exited`, which is expected here.
- Logging: the first `Wakeup` is logged at info and later (coalesced) ones at debug (`RUST_LOG=jterm=debug`). Window titles and other terminal content are only logged at debug.
- **Snapshot** (`terminal/snapshot.rs`): `RenderSnapshot::capture` runs under the `Term` lock and copies the visible cells with colors already resolved to `Color32`. `Palette` holds all 269 alacritty color slots: a color the program set (OSC 4/10/11) wins, then the theme, then the xterm 256-color values. DIM is 2/3 brightness, INVERSE swaps fg/bg after dimming, and HIDDEN sets fg to bg. Bold uses the bold face; it doesn't brighten the color.
- **Renderer** (`ui/terminal_view.rs`):
  - Paints the default background, then background runs, then text runs, then the cursor.
  - A text run is one galley of adjacent cells with the same fg and style. Undecorated spaces join any run, so they don't split it.
  - Runs use `extra_letter_spacing = cell.width - advance` (advance measured per face) to stay on the cell grid; `runs_stay_on_the_cell_grid` tests this.
  - Wide chars, and any non-ASCII char whose advance differs from the face's (CJK, emoji, fallback-font glyphs, cached in `fits`), are painted alone, centered in their 1 or 2 cells.
  - The origin is `rect.min + padding` snapped to pixels. The grid size comes from the rect the app allocates and paints into.
  - Cursor: block (glyph redrawn in the cell's bg), beam and underline are 2px. It's a hollow block when the window is unfocused or the shape is HollowBlock.
  - Control characters (alacritty stores a literal `\t` in tab cells) are drawn as spaces. egui would lay out `\t` 4 cells wide. Underline and strikethrough are drawn with `painter.hline` over whole cells, not egui's text decorations, which stop short of the last cell. U+2009 and U+202F never join runs, because egui narrows them.
- `ColorRequest` (OSC 4/10/11 queries) is answered from `session.color(index, theme)`. A shell spawn error is shown in the window.
- **Known rendering gaps (Step 5 review, deferred):**
  - **Ligatures:** egui shapes text with harfrust and default features, so JetBrains Mono ligatures (`->`, `!=`) render even though SPEC puts ligatures in Phase 4. egui has no switch to turn them off. A ligature can also look different where a run splits or under the block cursor, and a user font with true multi-char ligatures or kerning could drift. Options: accept it, strip `liga`/`calt` from the bundled font, or a custom renderer in Phase 4.
  - **Combining marks:** `cell.zerowidth()` isn't rendered, so decomposed accents and ZWJ/VS16 emoji show only the base char.
  - **DIM:** always 2/3 brightness. alacritty instead uses the Dim* palette entries for named colors, so OSC changes to Dim* colors are ignored.
  - **Performance:** a screen of all-different colors is about 4,300 galleys per frame. `Palette::new` runs under the lock every frame, and `fits` uses a SipHash map. Profile in Step 11.
- **Fonts on this machine:** the user's oh-my-posh prompt uses Nerd Font icons, which JetBrains Mono lacks, so they show as boxes. CaskaydiaCove Nerd Font is installed, and `font.path` can point to it. CJK and Braille show as boxes because no system fallback font is loaded yet; planned for Step 11.
- **Rendering checks without keyboard input:** run a PowerShell test script as the shell, by backing up the config and setting `[shell] program = 'pwsh.exe'`, `args = ['-NoLogo', '-NoProfile', '-NoExit', '-File', '<abs path>/.claude/skills/vt-tests/scripts/<test>.ps1']`. Take a `smoke.ps1 -Screenshot`, then restore the config.
- Config validation: `font.size` must be 4–200 and `window.padding` finite and ≥ 0. Otherwise the whole config falls back to defaults, like any other invalid value.

## Conventions (SPEC §8)

- No `unwrap()` outside tests unless a comment documents the invariant. Use `?` with `anyhow::Context` in app code and `thiserror` in library-style modules.
- Doc comments on public items explain *why*.
- Never log keystrokes or terminal content at the default log level. No telemetry or network calls (AI is opt-in, Phase 5).
- Tests: table-driven key→bytes tests covering normal and application cursor modes, config parse and fallback tests, and headless integration tests that spawn a session and assert grid contents through `Term` APIs.

## Claude Code setup (`.claude/`)

- **`/mvp-step N`**: the end-to-end workflow for an MVP step: research, implement, verify every acceptance criterion, update docs, commit. Use it for every step.
- **`verify` skill**: the gates plus `.claude/skills/verify/scripts/smoke.ps1`, which launches `jterm.exe` and reports the window title and size, `alive`, and log lines. `-Screenshot <png>` captures the window (Read the PNG to check rendering), and `-Keys "<SendKeys>"` types into it. `smoke.sh` is the Linux/macOS fallback (alive check and logs only).
- **`vt-tests` skill**: escape-sequence scripts for Steps 5–11 (256 colors, truecolor, SGR attributes, wide chars, OSC title and bell, cursor key modes) with their expected results, plus the Step 11 manual checklist.
- **Agents:**
  - `crate-researcher` looks up the real API of a crate at our `Cargo.lock` version, using registry source, context7 and egui_term.
  - `rust-reviewer` reviews a diff against the SPEC rules.
  - `acceptance-checker` gives an independent PASS/FAIL/MANUAL verdict per acceptance criterion.
- **Hooks** (`.claude/settings.json`, scripts in `.claude/hooks/`, require `jq`):
  - Edited `.rs` files are run through `rustfmt` automatically.
  - Every `git commit` that touches Rust code, Cargo files or assets runs `cargo fmt --check`, clippy `-D warnings` and `cargo test` first, and is blocked if any fail. Fix the cause; never bypass it.
  - Session start reports the last `mvp(step-N)` commit and the next step.
- **MCP:** `.mcp.json` is gitignored. Copy `.mcp.example.json` to `.mcp.json`, then either put your Context7 key in the `Authorization` header or delete the `headers` block to use Context7 without a key (rate-limited). Use context7 for egui/eframe/alacritty_terminal docs, but check anything it says against the local source.

## Gotchas

- Crate APIs here are newer than model training data. Read the real source in `~/.cargo/registry/src/*/<crate>-<version>/` (version from `Cargo.lock`) before writing code against egui/eframe/alacritty_terminal.
- Don't edit text containing backslashes (Windows paths) with `sed`/`perl` from Git Bash; escapes get silently corrupted. Use the Edit tool.
- winit opens an untitled helper window before the real one, so window checks must wait for the titled window (`smoke.ps1` does).
