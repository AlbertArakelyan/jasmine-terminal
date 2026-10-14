# Known issues and compatibility status

This file is the SPEC §5 Step 11 compatibility pass: what was tested, where, the results, and
what's still open. Re-run the checklist before every release.

## Test environments

- **Windows 11** (dev machine): jterm runs PowerShell 7 through ConPTY. Linux programs were
  tested inside jterm through WSL 2 (Ubuntu 26.04), started with `wsl -d Ubuntu`, so they run on
  real Linux but still go through ConPTY. Tests were driven by `.claude/skills/verify` (typed keys,
  posted mouse events, screenshots).
- **Linux (X11 / Wayland):** not tested yet. See [Linux checklist](#linux-checklist).

## Step 11 checklist

| Item | Windows result | Notes |
|---|---|---|
| vim / nvim: insert, visual, colorschemes, mouse | ✅ | Ubuntu vim: `desert` colorscheme and syntax colors, click with `mouse=a` moves the cursor, `V` visual line mode, redraw after resize. nvim (Windows): insert, `hjkl`, arrows, `:wq`, click. |
| htop / btop | ⬜ not tested | Not installed in WSL (`sudo apt install htop btop`). Linux checklist. |
| tmux: splits, status bar colors, mouse | ✅ | Vertical and horizontal splits (`tput cols` = 61 per half), custom and default status bar colors, mouse click switches panes. |
| less / man | ✅ | `man ls`: bold and underline render, wheel scrolls (12 lines per 4 notches), inverse status line. Git's `less` scrolls with the wheel. |
| ssh to a remote host, vim there | ⬜ not tested | No SSH server available (`sudo apt install openssh-server` in WSL, then `ssh localhost`). Linux checklist. |
| fzf (Ctrl+R) | ⚠️ partial | fzf isn't installed. bash's built-in Ctrl+R reverse search works (and exposed a cursor bug, fixed). Linux checklist. |
| lazygit or tig | ✅ (tig) | tig in Git Bash: log/diff split view, colors, `j`/`k`, Enter. lazygit not installed. |
| `cat` a large file (50 MB) | ✅ | UI stays responsive (231 samples: 2 ms average and 37 ms worst message round trip, never hung). Throughput, 50 MB from an interactive WSL prompt: **jterm 2.56 s vs Alacritty 0.17 3.81 s**. |
| Unicode: CJK, emoji, box drawing aligned | ⚠️ partial | Alignment is correct: wide chars take 2 cells and box borders line up. Emoji and box drawing render. **CJK and Braille glyphs show as boxes** (see issues). |
| Wayland and X11 launch | ⬜ not tested | Linux only. Linux checklist. |

## Bugs found and fixed in this pass

- **Focus reporting was missing** (DECSET 1004, required by SPEC §4.1). Windows ConPTY enables
  it to track whether the terminal is focused. Without `ESC[I`, output from an interactive WSL
  shell was throttled to a crawl (a 5 MB `cat` took over 38 s; 0.2 s with the fix). jterm now
  reports focus changes to programs that enable it, and the current state when they enable it.
- **`COLORTERM`/`TERM_PROGRAM` didn't reach WSL.** WSL forwards only variables listed in
  `WSLENV`, so Linux programs couldn't detect truecolor. jterm adds both to `WSLENV`.
- **Character under the block cursor vanished in reverse video** (readline's Ctrl+R match,
  TUI selections). The cursor now draws it in whichever cell color contrasts more.

## Open issues

1. **CJK and Braille glyphs are missing.** The bundled JetBrains Mono and egui's fallback fonts
   don't cover them. Layout is correct; only the glyphs are absent. Fix: load a system fallback
   font, e.g. Microsoft YaHei / Yu Gothic on Windows, Noto Sans CJK on Linux, PingFang on macOS,
   and Segoe UI Symbol / DejaVu for Braille. The open question is the memory cost (CJK fonts
   are 15–20 MB).
2. **Nerd Font icons are missing** (oh-my-posh and starship prompts show boxes). Undecided between
   bundling Symbols Nerd Font Mono as a fallback, adding a `font.fallback_paths` config, or
   bundling JetBrains Mono Nerd Font.
3. **AltGr characters on non-US Windows layouts can't be typed.** egui-winit drops text typed
   with Ctrl held, and AltGr is Ctrl+Alt on Windows. jterm sends nothing rather than wrong escapes.
4. **Ligatures render** (`->`, `!=`) because egui shapes text with the font's default features,
   although SPEC plans ligatures for Phase 4. egui has no switch to turn them off.
5. **Combining marks are dropped**: decomposed accents and ZWJ/VS16 emoji show only the base character.
6. **No auto-scroll when dragging a selection past the window edge**; scroll with the wheel
   while dragging instead. No middle-click (PRIMARY) paste on Linux.
7. **Wheel in nvim on Windows is unreliable** in automated tests even though jterm sends correct
   SGR wheel reports (clicks through the same path work). Likely ConPTY/nvim. Confirm with a real
   mouse.
8. **Git Bash (MSYS2) drops the first key typed after a resize.** jterm sends it; PowerShell and
   WSL don't lose it. External.
9. **Windows: Ctrl+Insert interrupts instead of copying**, and Ctrl+V with an empty clipboard
   sends nothing (egui-winit turns both into clipboard events).

## Linux checklist

Run on a Linux machine (X11 and Wayland sessions if possible). Record failures in
[Open issues](#open-issues).

Most of this is automated: `bash .claude/skills/verify/scripts/linux-checks.sh` runs
items 1–8 and 10–16 where the tools are installed, and writes a report with PASS/FAIL and
screenshots (see `.claude/skills/verify/SKILL.md` → Linux). ssh (9), IME and general feel
remain manual.

1. **Launch** on Wayland and on X11 (`WAYLAND_DISPLAY= cargo run` forces X11 under XWayland).
   The window opens, the shell prompt renders, and the log shows `spawning default login shell`
   with your `$SHELL`.
2. **Environment:** `echo $TERM $COLORTERM $TERM_PROGRAM` → `xterm-256color truecolor jterm`.
3. **Ctrl+C / Ctrl+D:** `sleep 100`, Ctrl+C interrupts. Ctrl+D exits a nested `bash`.
4. **Resize:** resize the window, then `tput cols; tput lines` matches. htop and vim redraw correctly.
5. **htop / btop:** colors, the meters update, mouse clicks on columns or processes, F-keys
   (F6 sort, F10 quit), resize.
6. **tmux:** splits (`Ctrl+b %`, `Ctrl+b "`), status bar colors, `set -g mouse on` then click
   panes and drag borders, scroll a pane with the wheel.
7. **vim / nvim:** colorscheme, visual mode, `:set mouse=a` click and wheel, focus events (vim
   refreshes on focus).
8. **less / man:** `man ls` bold and underline, wheel scrolling.
9. **ssh:** `ssh` to a host and run vim there; colors, mouse and resize carry over.
10. **fzf:** Ctrl+R history search, colors, mouse selection in the list.
11. **lazygit or tig:** navigation and colors.
12. **Big output:** `time cat` of a 50 MB file stays responsive; compare with Alacritty on the
    same machine (target: within 2×).
13. **Unicode:** `bash .claude/skills/vt-tests/scripts/unicode.sh`. Borders aligned; CJK/Braille
    glyphs (see issue 1).
14. **Copy/paste:** select with drag, double-click and triple-click; Ctrl+Shift+C, then paste
    in another app; Ctrl+Shift+V of multi-line text into bash/zsh doesn't run it (bracketed paste).
    Check that Ctrl+Shift+C arrives as copy on X11 and Wayland.
15. **Title/bell:** `printf '\e]0;hello\a'` sets the title; `printf '\a'` flashes.
16. **Idle CPU:** near 0% in `top` with the default steady cursor.
