# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

MVP Steps 1–2 are done: the SPEC §3.3 module layout exists, config loading works, and the bundled font is registered. `cargo run` opens an empty 1000×650 window filled with the theme background. Modules for later steps are still empty stubs. **`SPEC.md` is the source of truth**: it defines the product (Jasmine Terminal, binary `jterm`, a Warp-style terminal built on Rust + egui + `alacritty_terminal`), the target architecture, and an ordered MVP plan (§5, Steps 1–12) with acceptance criteria per step. Read the relevant SPEC section before starting any work.

Notes:
- The package is named `jasmine-terminal`, but a `[[bin]]` entry builds it as `jterm` (`target/debug/jterm.exe`).
- Dependencies are on their latest releases: `egui`/`eframe` 0.36 and `alacritty_terminal` 0.26. eframe 0.36 needs Rust 1.95 or newer, so `Cargo.toml` sets `rust-version = "1.95"`. eframe uses `App::ui(&mut self, ui, frame)`, not the older `update(ctx, frame)`.
- Development happens on Windows, but the spec targets Linux first, then macOS, and treats Windows (ConPTY) as a v1 non-goal. Keep code portable. Flag any PTY or platform behaviour that can't be checked on this machine instead of assuming it works.

## Workflow rules from SPEC.md

- Implement MVP steps **in order**. Don't start a step until the previous step's acceptance criteria pass. Don't build post-MVP features (blocks, input editor, tabs, palette, AI), but don't block them either: follow the module layout in SPEC §3.3.
- **Never write a VT/ANSI parser or terminal grid.** Use `alacritty_terminal` (its `tty` module for the PTY, `event_loop::EventLoop` for I/O). Study `egui_term` as the reference implementation.
- Before adding a dependency, look up its current version on crates.io and use the latest release. Don't rely on versions from memory. `egui_term` doesn't constrain our versions. It may target an older `egui`, so adapt any code borrowed from it to our egui API.
- If the spec turns out to be wrong or impossible with the chosen crates, stop, explain the conflict and propose an alternative. Don't deviate silently.
- After each step, run the checks below, then commit as `mvp(step-N): <summary>` (later phases use `phase1: ...` and so on).

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

## Architecture (per SPEC §3; terminal parts not built yet)

- **Two threads.** The eframe UI thread and alacritty's PTY I/O `EventLoop` thread share `Arc<FairMutex<Term<EventProxy>>>`.
- **`EventProxy`** implements alacritty's `EventListener`. It forwards events (Wakeup, Title, Bell, Exit, ClipboardStore, PtyWrite) over a channel to the app and calls `ctx.request_repaint()`.
- **Rendering.** The UI locks `Term` briefly to copy visible cells, cursor and selection into a `RenderSnapshot` (`terminal/snapshot.rs`, where colors are resolved against the theme), releases the lock, then paints. Never hold the `Term` lock while painting or laying out text.
- **Painting** batches runs of identically styled cells. Never create one galley per cell. Wide characters span 2 cells, and the spacer cell is skipped.
- **Repaint only on events** (PTY wakeup, input, cursor-blink timer via `request_repaint_after`). No continuous repaint loop; idle CPU should be about 0%.
- **Input** (`ui/input_map.rs`) maps egui events to bytes and must respect `TermMode` (DECCKM application cursor mode, bracketed paste, mouse reporting; Shift overrides mouse reporting for local selection).
- **Config** (`config/`) is TOML at the `directories` config path: `~/.config/jterm/config.toml` on Linux, `%APPDATA%\jterm\config\config.toml` on Windows. A missing file is created with commented defaults. Every section and key is optional, and missing ones take their defaults (`#[serde(default)]`). Unknown keys are only warnings (reported via `serde_ignored`). A malformed file or invalid value logs an error and the whole config falls back to defaults, without overwriting the file. `Config::load()` never fails. Colors are `"#rrggbb"` strings (`theme::Rgb`). The normal and bright ANSI sets are separate types, so each fills in its own defaults.
- **Fonts** (`ui/fonts.rs`): JetBrains Mono (OFL, `assets/fonts/`) is embedded with `include_bytes!` and installed as egui's `Monospace` family. Bold, italic and bold-italic are registered as named families (`fonts::BOLD_FAMILY` etc.), because egui has no font weights or styles. `font.path` replaces only the regular face. Fonts can't be looked up by family name.

## Conventions (SPEC §8)

- No `unwrap()` outside tests unless a comment documents the invariant. Use `?` with `anyhow::Context` in app code and `thiserror` in library-style modules.
- Doc comments on public items explain *why*.
- Never log keystrokes or terminal content at the default log level. No telemetry or network calls (AI is opt-in, Phase 5).
- Tests: table-driven key→bytes tests covering normal and application cursor modes, config parse and fallback tests, and headless integration tests that spawn a session and assert grid contents through `Term` APIs.
