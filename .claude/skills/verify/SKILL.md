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
- `-Keys "<SendKeys>"` — brings jterm to the front and types, e.g. `-Keys "ls{ENTER}"`,
  `-Keys "^c"` (Ctrl+C), `-Keys "%x"` (Alt+X), `{UP}`, `{TAB}`, `{ESC}`. Separate groups with
  `||` to type them `-KeyDelayMs` apart (default 800), e.g.
  `-Keys "Start-Sleep 100{ENTER}||^c||echo done{ENTER}"`. `-StartDelayMs` waits before the
  first group; use 3000 with the user's PowerShell profile, which takes 1–1.6 s to load.
  - The script refuses to type (`FAIL: jterm isn't the foreground window`) unless jterm is in
    front. SendKeys types into whatever window is focused, which could be one of the user's.
  - SendKeys specials need braces: `{+}`, `{^}`, `{%}`, `{~}`, `{(}`, `{)}`, `{{}`, `{}}`.
  - Typed commands go into the user's PSReadLine history.
  - Example: an automated vim check is
    `-Keys "nvim -u NONE '<scratch>/t.txt'{ENTER}||ihello{ESC}||hhx||{LEFT}x||:wq{ENTER}"`,
    after which the file should contain `hlo`.
- `<wheel N>` / `<wheel -N>` / `<wheel N shift>` groups scroll N notches up/down over the window via
  `WM_MOUSEWHEEL` (the real mouse isn't moved).
- A `<resize WxH>` group resizes the window (outer size in pixels) instead of typing, e.g.
  `-Keys "echo a{ENTER}||<resize 700x450>||$Host.UI.RawUI.WindowSize{ENTER}"`.
- `-Seconds N` — total run time, including key delays; output settles before the screenshot is taken.

Linux/macOS: `bash .claude/skills/verify/scripts/smoke.sh 4` (alive check and logs only).

The app reads the real user config (`%APPDATA%\jterm\config\config.toml` on Windows). If a test
needs different settings, back the file up, edit it, and restore it afterwards — and tell the
user you did.

## 3. Map results to acceptance criteria

For each criterion, report the evidence: test name, smoke output line, screenshot observation,
or "manual — needs the user" (interactive TUIs like vim/htop/tmux, IME, clipboard with another
app, Wayland/X11). Never mark a criterion passed without evidence.
