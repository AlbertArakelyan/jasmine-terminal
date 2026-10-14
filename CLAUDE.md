# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

MVP Steps 1–11 are done (Step 11's Windows/WSL part; the Linux checklist in `KNOWN_ISSUES.md` is the user's to run): config, the bundled font, cell metrics, the shell in a PTY, grid rendering, keyboard input, resize, scrollback with the mouse wheel, selection with copy/paste and mouse reporting, and window title, visual bell and cursor blink. `cargo run` opens a 1000×650 window running the shell (PowerShell 7 on this machine). You can type, scroll, select (drag, or double/triple click), copy (Ctrl+Shift+C, or Ctrl+C with a selection on Windows) and paste (Ctrl+Shift+V). Programs that enable mouse reporting get clicks, OSC titles set the window title, BEL flashes, and the cursor blinks when configured. Next is Step 12 (release hygiene). `KNOWN_ISSUES.md` holds the compatibility results and the open issues. **`SPEC.md` is the source of truth**: it defines the product (Jasmine Terminal, binary `jterm`, a Warp-style terminal built on Rust + egui + `alacritty_terminal`), the target architecture, and an ordered MVP plan (§5, Steps 1–12) with acceptance criteria per step. Read the relevant SPEC section before starting any work.

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
- **Keyboard** (`ui/input_map.rs`, `JtermApp::handle_keyboard`):
  - xterm sequences. DECCKM switches arrows, Home and End to `ESC O x`. Modifiers use the CSI `1;m` form. Backspace is DEL and Ctrl+Backspace is BS. Alt adds an ESC prefix (not on macOS).
  - egui-winit quirks:
    - Ctrl/Cmd+C/X/V arrive only as `Event::Copy/Cut/Paste` (no Key event), so they're mapped back: Ctrl+C → ^C, Ctrl+Shift+C → `Input::Copy` (Step 9), Ctrl+X → ^X, Shift+Delete → `ESC[3;2~`, Ctrl+V → ^V (PSReadLine pastes on it itself), Ctrl+Shift+V and Shift+Insert → paste.
    - Text typed with Ctrl held is dropped, so plain Ctrl+letter comes from Key events. **AltGr characters on non-US Windows layouts are lost** (AltGr = Ctrl+Alt): a known gap. Ctrl+Alt key events send nothing on Windows; otherwise they'd inject ESC + control codes (AltGr+8 → ESC ESC).
    - IME input arrives only as `Ime(Commit)`. The OS IME is only enabled while the app sets `ctx.output_mut(|o| o.ime = Some(..))`, which it does every frame the terminal has focus, with the cursor cell as `cursor_rect`.
    - Paste is only emitted when the clipboard has text, so Ctrl+V with an empty clipboard sends no ^V. Ctrl+Insert arrives as Copy and sends ^C. Both can be fixed in Step 9.
  - `input_map::KeyboardState` tracks modifiers through the event stream (`ModifiersChanged` and the modifiers on key events) and across frames. `InputState::modifiers` is the end-of-frame state, after a fast Ctrl+C has already released Ctrl, so it would misread "x then Ctrl+V" in one frame. Cmd combinations never leak as keys on macOS.
  - **For Steps 8–9:** Shift+PgUp/PgDn currently go to the PTY. Check reserved shortcuts before `KeyboardState::feed`, and don't let them trigger scroll-to-bottom. The widget uses `Sense::click()` and needs `click_and_drag()`. Shift+Insert pastes only on Windows (egui-winit); decide PRIMARY-selection paste on Linux. `request_focus()` runs every frame, which will need changing once other focusable widgets exist (palette).
  - Paste: bracketed paste wraps the text and strips ESC and ^C, so the bracket can't be closed early. Without bracketed paste, newlines become CR.
  - The terminal widget keeps focus, and a focus lock filter keeps Tab, Escape and the arrows from moving egui focus. `zoom_with_keyboard` is off, so Ctrl+- reaches the shell.
  - Typing scrolls the view to the bottom (`send_input`).
- **Resize:**
  - Each frame, `JtermApp::sync_session` builds a `TermSize` (grid from the painted rect, plus the cell size in physical pixels) and calls `TerminalSession::resize`. That's a no-op unless the size changed, and since the grid is whole cells, sub-cell pixel changes never resize.
  - `resize` calls `Term::resize`, which reflows, then `Notifier::on_resize`, which sends `Msg::Resize` to the PTY (ConPTY resize, or TIOCSWINSZ/SIGWINCH on Unix).
  - `TextAreaSizeRequest` (CSI 14/18 t) is answered from `session.size()`.
  - `resize_reaches_the_shell` checks that the shell itself sees the new size (`mode con` / `stty size`).
  - **Known external issue:** Git Bash (MSYS2) under ConPTY drops the first key typed after a resize. PowerShell doesn't; jterm does send the key (seen in trace logs).
- **Scrollback and wheel** (`ui/mouse.rs`, `JtermApp::handle_input`):
  - `WheelAccumulator` turns wheel deltas into whole lines: 3 per notch, trackpad pixels divided by the cell height with the remainder carried, pages as the row count. Positive means up into history.
  - `wheel_action` picks what the wheel does:
    - With mouse reporting on and no Shift: wheel button reports (64/65) at the pointer cell.
    - On the alternate screen with ALTERNATE_SCROLL and no Shift: arrow keys, DECCKM-aware (less, man).
    - Otherwise: `Scroll::Delta` on the scrollback.
  - `mouse_report` encodes SGR, UTF-8 and legacy xterm reports, ready for Step 9's click reporting.
  - Shift+PgUp/PgDn page the scrollback (not on the alternate screen, where the program gets them). They're handled before `KeyboardState::feed`, so they don't jump to the bottom.
  - Output arriving while scrolled up doesn't move the view, because alacritty grows `display_offset` with it (`scrolled_view_stays_put_while_output_arrives`). Typing jumps back to the bottom.
  - While scrolled up, a "↑ N" badge sits in the top-right corner (`paint_scroll_badge`).
  - `Layout::cell_at(pos)` maps a pointer position to a cell, clamped to the grid.
  - `WheelAccumulator` drops its leftover fraction when the direction reverses, and ignores non-finite deltas.
  - Output to programs is capped at one page per event.
  - Reports add Shift +4, Alt +8 and Ctrl +16 to the button code.
  - Legacy and UTF-8 reports past their coordinate limit (223 / 2015) are dropped rather than clamped, and so is a report with no pointer position.
  - The badge uses inverted theme colors (`snap.foreground` / `snap.background`).
  - `InputModes::from(TermMode)` avoids a second lock.
- **Selection and clicks** (`mouse::PointerState`, `TerminalSession::*_selection`):
  - Without mouse reporting, or with Shift held, the primary button selects. One press starts a Simple selection (empty until dragged), two presses within 400 ms on the same cell select a word (Semantic), three a line (Lines). Dragging extends it.
  - Selections are alacritty's `Term::selection`, in grid coordinates (`viewport_to_grid`: line = row − display_offset), so they stay on their text as output scrolls.
  - The snapshot paints selected cells' bg with `theme.selection`.
  - Typing (`send_input`) clears the selection.
  - With mouse reporting and no Shift: press, release and drag (+32, only on cell change, with MOUSE_DRAG or MOUSE_MOTION) are reported. Plain motion is reported as button 3+32 only with MOUSE_MOTION. A release is always reported for a reported press.
  - Copy: Ctrl+Shift+C (Cmd+C on macOS) → `ctx.copy_text(selection_text)`. **On Windows, Ctrl+C copies and clears when there's a selection, else sends ^C** (`Input::CopyOrInterrupt`, like Windows Terminal).
  - Button presses being reported are tracked per button (bitmask), so each release matches its press. A release is dropped if the program turned mouse reporting off meanwhile, so it isn't typed into the shell. Drag reports carry the current modifiers. `PointerGone` or losing focus ends a local drag. Double-clicks allow a 1-cell wobble. Wheel scrolling during a drag re-extends the selection.
  - A wide character and its spacer are highlighted together if either half is selected.
  - The Windows Ctrl+C copy uses the selection even if it has scrolled out of view (as in Windows Terminal).
  - Not done: auto-scroll when dragging past the top or bottom edge; middle-click/PRIMARY paste on Linux. Whether X11/Wayland egui-winit delivers Ctrl+Shift+C as `Event::Copy` with Shift set is unverified (it does on Windows).
- **Title, bell, blink:**
  - `Event::Title` → `ViewportCommand::Title`. `ResetTitle` restores "jterm". On Windows the console layer itself sets the title to the running program while a command runs, and Git Bash sets `MINGW64:<cwd>` on every prompt.
  - With `bell.visual`, `Event::Bell` starts a 150 ms fading overlay in the theme foreground (`paint_bell`), repainting at about 60 fps only while it fades.
  - `cursor.style` and `cursor.blink` set alacritty's `default_cursor_style`, so programs can still change the cursor with DECSCUSR and a reset returns to the config.
  - Blinking (`snapshot.cursor.blinking`) uses `cursor_blink(elapsed)` with a 530 ms half-period. It repaints only at toggles (`request_repaint_after`), only while focused, and restarts on keys, text, paste and focus gain, so the cursor is solid while typing.
  - Measured idle CPU over 5 s in a release build: steady cursor 16 ms (debug build 0–31 ms), blinking 62 ms.
- **Focus reporting** (`JtermApp::sync_focus_reports`): while a program has DECSET 1004 on, jterm sends `ESC[I` / `ESC[O` on focus changes, and the current state when the mode turns on. **Windows ConPTY enables it and throttles output badly without it**: a 5 MB `cat` from an interactive WSL shell took over 38 s before the fix and 0.2 s after.
- **WSL:** `wsl -d Ubuntu` from PowerShell inside jterm runs Linux TUIs (vim, tmux, man, less) through ConPTY. jterm appends `COLORTERM:TERM_PROGRAM` to `WSLENV` (`wslenv_with`) so they reach Linux. Ubuntu has tmux, man, vim and ssh. htop, btop, fzf, tig and sshd need `sudo apt install` (the user's password). The default WSL distro here is docker-desktop, so always pass `-d Ubuntu`.
- **Block cursor text:** the glyph under a block cursor uses whichever of the cell's fg/bg contrasts more with the cursor color (`cursor_text_color`). In reverse video the bg is light, and the glyph used to vanish.
- **Benchmarks:** Alacritty 0.17 is installed at `C:\Program Files\Alacritty\alacritty.exe`, and `smoke.ps1 -Exe` can drive it like jterm. To time output, write a script into WSL (`start=$(date +%s.%N); cat file; ...`) that writes its timing to a file, and read the file afterwards. 50 MB from an interactive WSL prompt: jterm 2.56 s, Alacritty 3.81 s (the first Alacritty run measured 38 s, a cold-cache outlier). WSL `/tmp` doesn't persist between separate `wsl` invocations, so use `$HOME`. In SendKeys, `~` is Enter and `%` is Alt.
- **Linux testing:** `.claude/skills/verify/scripts/smoke.sh` (Linux counterpart of `smoke.ps1`, xdotool-based) and `linux-checks.sh` (the KNOWN_ISSUES Linux checklist, writes `report.md` + screenshots). Both use `scripts/linux/lib.sh`. They were written on Windows and only checked with a stub xdotool in WSL, so expect fixes on the first real run. Linux is the primary target and hasn't been run on a real desktop yet.
- **Mouse tests:** `smoke.ps1` takes `<click c,r>`, `<dclick c,r>` and `<drag c1,r1 c2,r2>` (0-based cells, assuming 8×18 cells and 8pt padding). It posts `WM_MOUSEMOVE` immediately before every button or wheel message. The real cursor is elsewhere, so Windows soon sends WM_MOUSELEAVE, and egui then drops button events and has no pointer position, which means no mouse reports. That race caused the flaky nvim results in Step 8. Tests that copy overwrite the user's clipboard, so back it up (`Get-Clipboard -Raw`) and restore it.
- **Testing the wheel:** `smoke.ps1` takes `<wheel N>` / `<wheel -N>` / `<wheel N shift>` groups. It posts `WM_MOUSEMOVE` (so egui knows the pointer cell) and then `WM_MOUSEWHEEL` to the window, so the user's real mouse isn't moved. Git's `less` is at `C:\Program Files\Git\usr\bin\less.exe` (not on the PowerShell PATH), and nvim enables mouse reporting by default.
  - **Known flakiness:** with nvim on Windows, wheel reports sometimes don't scroll, even when the trace logs show correct SGR wheel reports at the pointer cell. Part of it was the WM_MOUSELEAVE race (see "Mouse tests"), but it persists after that fix, while clicks through the same path work reliably. So it's in ConPTY or nvim; confirm wheel-in-nvim with a real mouse. History scrolling and `less` are reliable.
- **Windows Ctrl+C:** children inherit the parent's "ignore Ctrl+C" flag. When jterm was started from Git Bash or cargo, ^C (0x03) reached ConPTY but interrupted nothing. `enable_ctrl_c_for_children()` calls `SetConsoleCtrlHandler(NULL, FALSE)` before spawning. `ctrl_c_interrupts_a_running_program` covers it.
- **Typing into the app from tests:** `smoke.ps1 -Keys "group1||group2" -KeyDelayMs N -StartDelayMs N` uses SendKeys syntax. It first makes jterm the foreground window (via `AttachThreadInput`) and refuses to type otherwise, because SendKeys goes to whatever window is in front and stray keys landed in the user's windows before this check existed. The user's PowerShell profile takes about 1–1.6 s to load, so use `-StartDelayMs 3000`. Test commands end up in the user's PSReadLine history.
- **Known rendering gaps (Step 5 review, deferred):**
  - **Ligatures:** egui shapes text with harfrust and default features, so JetBrains Mono ligatures (`->`, `!=`) render even though SPEC puts ligatures in Phase 4. egui has no switch to turn them off. A ligature can also look different where a run splits or under the block cursor, and a user font with true multi-char ligatures or kerning could drift. Options: accept it, strip `liga`/`calt` from the bundled font, or a custom renderer in Phase 4.
  - **Combining marks:** `cell.zerowidth()` isn't rendered, so decomposed accents and ZWJ/VS16 emoji show only the base char.
  - **DIM:** always 2/3 brightness. alacritty instead uses the Dim* palette entries for named colors, so OSC changes to Dim* colors are ignored.
  - **Performance:** a screen of all-different colors is about 4,300 galleys per frame. `Palette::new` runs under the lock every frame, and `fits` uses a SipHash map. Profile in Step 11.
- **Fonts on this machine:** the user's oh-my-posh prompt uses Nerd Font icons, which JetBrains Mono lacks, so they show as boxes. CaskaydiaCove Nerd Font is installed, and `font.path` can point to it. CJK and Braille show as boxes because no system fallback font is loaded yet; planned for Step 11.
  - egui's `emoji-icon-font` is removed from the terminal's fallback chain (`fonts::terminal_fallbacks`). It puts egui UI icons on Private Use Area codepoints that clash with Nerd Fonts: oh-my-posh's Windows logo U+E62A rendered as a Chrome logo. egui's NotoEmoji stays.
  - The bundled font is plain JetBrains Mono, not the Nerd Font build. Options discussed with the user, not decided yet: bundle Symbols Nerd Font Mono as a fallback, add a `font.fallback_paths` config, or bundle JetBrains Mono Nerd Font.
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
- The commit-gate hook runs before the whole Bash command, so `cargo fmt && git commit` in one command still fails the fmt check. Run `cargo fmt` in a separate command first. Code appended to files with `cat >>` skips the format-on-edit hook.
- winit opens an untitled helper window before the real one, so window checks must wait for the titled window (`smoke.ps1` does).
