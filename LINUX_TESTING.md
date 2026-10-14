# Linux test runbook (MVP Step 11, Linux part)

**For a Claude Code session on a Linux machine.** Work through this file top to bottom. It
covers setup, what to run, what correct results look like, where to record results, and how
to fix what fails. Read `CLAUDE.md` first for project context; `SPEC.md` §5 Step 11 is the
requirement.

## Context

- jterm was built and tested on **Windows**: PowerShell and Git Bash through ConPTY, plus
  Linux programs inside WSL (still through ConPTY). It has **never run on a real Linux desktop**.
  Linux is the primary target.
- Code paths that are different on Linux and have never been exercised:
  - **Shell:** no `shell.program` configured → `alacritty_terminal` starts `$SHELL` as a login
    shell (`terminal/session.rs`, `resolve_shell`).
  - **PTY:** a real Unix PTY. Ctrl+C is handled by the line discipline, resize by TIOCSWINSZ.
  - **Windowing:** winit on X11 and on Wayland (client-side decorations, scale factors).
  - **Clipboard and keys:** egui-winit's clipboard and shortcut handling on X11/Wayland. It's
    unverified whether Ctrl+Shift+C arrives as `Event::Copy` with Shift set.
  - **Fonts and rendering:** egui's text rendering with Linux GPU drivers (wgpu: Vulkan or GL).
- Results so far: `KNOWN_ISSUES.md`. Its "Open issues" list is already known; don't re-report
  those items, but do note if they behave differently on Linux.

## Ground rules

- Commit directly on `main`; no feature branches. Message format: `linux: <summary>` for
  fixes, `test: <summary>` for test-script changes, ending with the Co-Authored-By trailer.
- A pre-commit hook runs `cargo fmt --check`, clippy `-D warnings` and `cargo test`, and
  blocks the commit if any fail. Fix the cause; never bypass it. Run `cargo fmt` as a separate
  command before committing (the hook checks before the rest of a combined command runs).
- **One commit per fix.** Keep fixes minimal and in the style of the surrounding code.
- Don't change the user's dotfiles. If a check needs a different jterm config
  (`~/.config/jterm/config.toml`), back it up, change it, and restore it byte-for-byte, then
  tell the user. The clipboard checks overwrite the clipboard; that's acceptable on a test machine.
- The automation moves the real mouse pointer and types into the focused window. The user
  shouldn't use the desktop while `linux-checks.sh` runs; tell them before starting.
- If something can't be done (missing tool, no Wayland session, no sudo), skip it, record
  why, and keep going. Ask the user only for things only they can do: sudo passwords,
  logging into a Wayland/X11 session, manual checks.

## 1. Setup

1. **Rust** ≥ 1.95 (`rust-version` in `Cargo.toml`): `rustup update stable`, or install via rustup.
2. **Build dependencies.** Try `cargo build` first. If it fails to link or the window fails
   to open, install (Debian/Ubuntu names):
   ```sh
   sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev \
       libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev mesa-vulkan-drivers
   ```
   Record anything that was required and isn't listed here; it belongs in the README (Step 12).
3. **Test tools:**
   ```sh
   sudo apt install xdotool bc xclip wl-clipboard imagemagick grim \
       vim tmux htop btop fzf tig man-db less openssh-client
   ```
   Optional: `lazygit`, `alacritty` (needed for the throughput comparison; `cargo install
   alacritty` works if there's no package).
4. **Session type:** `echo $XDG_SESSION_TYPE`. Ideally run the checks once in an **X11** session
   and once in a **Wayland** session. If only one is available, do that one and record the other
   as not tested.
5. **Gates:** `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`.
   All must pass on Linux too. Linux-only failures (for example the Unix branches in
   `session.rs` tests) are real findings: fix them first.

## 2. Run the automated checks

```sh
cargo build
bash .claude/skills/verify/scripts/linux-checks.sh            # all checks
bash .claude/skills/verify/scripts/linux-checks.sh vim tmux   # re-run selected checks
```

- It prints one line per check and writes `/tmp/jterm-checks-<timestamp>/report.md` plus
  screenshots and result files in the same directory.
- Check names: `launch-x11 launch-wayland env ctrl-c ctrl-d resize title bell idle-cpu colors
  scrollback copy paste vim tmux htop btop man less fzf tig lazygit big-cat manual`.
- For ad-hoc experiments, `bash .claude/skills/verify/scripts/smoke.sh -s 8 -o /tmp/s.png -k
  "<steps>"` runs one session. Step syntax is in `.claude/skills/verify/SKILL.md` → Linux.
- To measure release performance, pass `-e target/release/jterm` after `cargo build --release`.
  Run `big-cat` and `idle-cpu` in release; everything else is fine in debug.

**The scripts have never run on a real desktop.** They were written on Windows and only
parse-checked with a stub `xdotool`. If a script itself breaks (wrong window found, keys not
arriving, wrong click positions, screenshots fail), fix the script first:
`.claude/skills/verify/scripts/linux/lib.sh`, `smoke.sh`, `linux-checks.sh`. Commit those
fixes as `test: ...`. Typical suspects:
- Window lookup (`xdotool search --pid`).
- Focus (`windowactivate`; some window managers need `windowfocus`).
- Cell-to-pixel maths. Positions come from jterm's "measured cell metrics" log line; padding is
  assumed to be 8pt (`JT_PAD`).
- Typing speed (`--delay`).

## 3. Review every result

For **PASS/FAIL** checks, trust the result but read the details column. For **LOOK** checks,
open each screenshot with the Read tool and compare it with the expectations below. A LOOK
becomes PASS or FAIL in your final table.

| Check | Correct result |
|---|---|
| launch-x11 / launch-wayland | Window shows the user's shell prompt in correct colors with a block cursor; no garbage, no blank window. On Wayland the window has decorations (or note if it doesn't). |
| env | `xterm-256color truecolor jterm` |
| ctrl-c / ctrl-d | `sleep 100` interrupted; nested bash exits on Ctrl+D |
| resize | `tput cols lines` match the window size |
| title | Window title becomes `jterm-title-ok` |
| bell | A light, translucent overlay across the whole terminal (caught mid-fade) |
| idle-cpu | Under 150 ms CPU per 5 s idle (target ~0) |
| colors | `colors-256.png`: numbers 0–255 in their colors, then a 16×16 swatch grid (theme ANSI colors, smooth 6×6×6 cube, gray ramp). `truecolor-attributes.png`: four smooth gradients; bold, dim, italic, underline, inverse, hidden (blank), strikethrough, bold-italic each look like their label. `unicode.png`: box borders aligned; CJK and emoji take 2 cells (CJK may be boxes: known issue 1); colored `ls`. |
| scrollback | `scroll-wheel.png`: earlier numbers and a "↑ 15" badge top-right; `scroll-pgup.png`: one more page up; `scroll-typed.png`: back at the bottom, no badge |
| copy / paste | Clipboard holds exactly the selected text; pasted two lines sit at the prompt, not executed |
| vim | File contains `hlo`; click lands on line 10; wheel scrolls |
| tmux | 2 panes, click selects the left pane; `tmux.png` shows the split and the status bar in color |
| htop / btop | Colored meters and process list, correct layout, `-wheel.png` shows the list moved |
| man / less | Bold and underlined text; the wheel scrolls the page, not jterm's history (no ↑ badge) |
| fzf | List with prompt and colors; wheel moves the selection |
| tig / lazygit | Colored log/diff views, correct layout |
| big-cat | UI stays responsive (worst round trip well under 1 s); jterm time within 2× of Alacritty |

Then do the items the script can't:

1. **Native Wayland input** (if on Wayland): start `cargo run` normally, type a few commands,
   select and copy with Ctrl+Shift+C, paste with Ctrl+Shift+V, scroll, resize. xdotool can't
   drive native Wayland windows, so ask the user to do this and report back.
2. **ssh:** `ssh <host>` (or `ssh localhost` after `sudo apt install openssh-server`), then run
   vim there: colors, mouse (`:set mouse=a`), and a resize while connected (vim redraws).
3. **IME** (if the user uses one): composing text with ibus/fcitx commits correctly.
4. **Feel:** ask the user to type and scroll for a minute. Any lag, flicker or blurry text?
5. **Fractional scaling** (if available, e.g. 125% or 150%): text is crisp and the grid is
   aligned. Cell sizes snap to physical pixels (`ui/cell_metrics.rs`).

## 4. Investigate and fix failures

For each FAIL or bad LOOK:

1. Reproduce it with `smoke.sh` and a minimal step list, and look at the jterm log (debug
   builds log to stderr; `RUST_LOG=jterm=trace` shows every input event and is the fastest way
   to see what jterm received).
2. Find the cause. Relevant code: `src/app.rs` (frame loop, input routing, events),
   `src/ui/input_map.rs` (keys), `src/ui/mouse.rs` (wheel, clicks, selection),
   `src/ui/terminal_view.rs` (painting), `src/terminal/session.rs` (PTY, shell, resize),
   `src/terminal/snapshot.rs` (colors, selection), `src/ui/fonts.rs`.
3. If it's jterm: fix it, add a unit test where the logic is testable, re-run the check, and
   commit `linux: <what was fixed>`. If it's the environment or another program, record it
   as an open issue with the evidence instead.
4. CLAUDE.md "Gotchas" and the per-module notes describe decisions that may look like bugs
   but are deliberate (e.g. Ctrl+C copies with a selection only on Windows). Read them before
   changing behaviour.

## 5. Record results in KNOWN_ISSUES.md

- **Step 11 checklist table:** add a **Linux** column (or one each for **X11** and
  **Wayland** if both were tested) with ✅ / ⚠️ / ❌ / ⬜ and short notes, next to the Windows
  column. Update the "Test environments" section with the distro, kernel, desktop, GPU and
  session type(s).
- **Bugs found and fixed:** add each Linux fix with a one-line explanation, like the existing entries.
- **Open issues:** add new problems (numbered, with evidence and a suggested fix). Update
  existing ones with Linux behaviour (e.g. whether CJK glyphs show via system fonts, and
  whether Ctrl+Shift+C works as copy on X11 and Wayland).
- **Linux checklist section:** tick off what was done and mark what's still open.
- Also update `CLAUDE.md` "Project state" in one sentence (Linux pass done, link to the
  results), plus any new gotcha.
- Commit the docs: `docs: Linux compatibility results`.

## 6. Report back

Tell the user:
- Which sessions were tested (X11/Wayland) and on what hardware.
- The final results table (check → PASS/FAIL with one-line evidence).
- The jterm bugs fixed (commits) and the test-script fixes.
- New open issues, most severe first.
- What still needs them: manual checks, sudo installs, a Wayland session if only X11 was available.
- Whether anything blocks Step 12 (release hygiene: README, release profile, `v0.1.0` tag).
