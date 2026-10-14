---
name: verify
description: Verify jterm actually works — run the fmt/clippy/test gates, launch the app for a smoke test, type keys into it, and screenshot the window to check rendering. Use after any change to the app, before committing, or when asked to run, launch, or screenshot jterm.
---

# Verify jterm

## 1. Gates (must pass)

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## 2. Smoke test (does the app start and stay up?)

Windows (this dev machine):

```sh
cargo build && pwsh -NoProfile -File .claude/skills/verify/scripts/smoke.ps1 -Seconds 4
```

It prints the window title and client size, `alive: yes`, and jterm's log lines (`jterm::*`
targets plus any WARN/ERROR/panic). Exit code 1 means the app died or never showed a window.

Options:
- `-Screenshot <path.png>` — captures the window (works with wgpu rendering). Save it in the
  scratchpad directory, then **Read the PNG** to check rendering yourself.
- `-Keys "<SendKeys>"` — focuses the window and types, e.g. `-Keys "ls{ENTER}"`,
  `-Keys "^c"` (Ctrl+C), `-Keys "%x"` (Alt+X). Run a VT test script with
  `-Keys "bash .claude/skills/vt-tests/scripts/colors-256.sh{ENTER}"` once a shell runs (Step 4+).
- `-Seconds N` — total run time; output settles before the screenshot is taken.

Linux/macOS: `bash .claude/skills/verify/scripts/smoke.sh 4` (alive check and logs only).

The app reads the real user config (`%APPDATA%\jterm\config\config.toml` on Windows). If a test
needs different settings, back the file up, edit it, and restore it afterwards — and tell the
user you did.

## 3. Map results to acceptance criteria

For each criterion, report the evidence: test name, smoke output line, screenshot observation,
or "manual — needs the user" (interactive TUIs like vim/htop/tmux, IME, clipboard with another
app, Wayland/X11). Never mark a criterion passed without evidence.
