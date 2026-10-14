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

### Linux

`smoke.sh` is the Linux counterpart of `smoke.ps1`. It uses xdotool (X11; on a Wayland
session jterm runs under XWayland so input works) and ImageMagick `import` for screenshots:

```sh
cargo build && bash .claude/skills/verify/scripts/smoke.sh -s 8 -o /tmp/s.png \
    -k "sleep 100{Return}||{ctrl+c}||echo ok{Return}"
```

- Steps are separated by `||`. Text is typed literally; `{keysym}` sends a key with xdotool
  names: `{Return}`, `{ctrl+c}`, `{ctrl+shift+v}`, `{Escape}`, `{shift+Prior}`, `{F10}`.
  `{{` types a literal `{`.
- The same `<resize WxH>`, `<wheel N>` / `<wheel -N shift>`, `<click c,r>`, `<dclick c,r>`,
  `<drag c1,r1 c2,r2>` steps work, plus `<sleep ms>`. Cell positions come from jterm's own
  "measured cell metrics" log line, so they're right at any display scale.
- `-d` step delay, `-w` start delay (ms), `-e` exe (e.g. `target/release/jterm`), `--wayland`
  for a native Wayland launch check (grim screenshot, no input).
- xdotool moves the real pointer and types into the focused window, so use a test machine or VM.

**`linux-checks.sh`** runs the automatable part of the Linux checklist in `KNOWN_ISSUES.md`
and writes `<out>/report.md` with screenshots:

```sh
bash .claude/skills/verify/scripts/linux-checks.sh            # everything
bash .claude/skills/verify/scripts/linux-checks.sh vim tmux   # selected checks
```

- Self-verifying checks (PASS/FAIL): env vars, Ctrl+C, Ctrl+D, resize, title, idle CPU, copy
  (drag and double-click), bracketed paste, vim keys/click/wheel, tmux split/mouse, big-file
  throughput vs Alacritty.
- LOOK: inspect the screenshot (launch on X11 and Wayland, bell, colors, Unicode, scrollback,
  htop/btop/man/less/fzf/tig/lazygit). MANUAL: ssh, IME, feel.
- Needs xdotool, bc, xclip or wl-clipboard, and ImageMagick or xwd+convert. Optional tools
  are skipped when missing. On Debian/Ubuntu:
  `sudo apt install xdotool bc xclip imagemagick grim vim tmux htop btop fzf tig man-db`.
- Untested so far: written on Windows and only parse-checked with a stub xdotool in WSL. Fix
  the scripts as needed on the first real run.

The app reads the real user config (`%APPDATA%\jterm\config\config.toml` on Windows). If a test
needs different settings, back the file up, edit it, and restore it afterwards — and tell the
user you did.

## 3. Map results to acceptance criteria

For each criterion, report the evidence: test name, smoke output line, screenshot observation,
or "manual — needs the user" (interactive TUIs like vim/htop/tmux, IME, clipboard with another
app, Wayland/X11). Never mark a criterion passed without evidence.
