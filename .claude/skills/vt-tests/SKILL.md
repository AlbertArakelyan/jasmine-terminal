---
name: vt-tests
description: Escape-sequence test scripts for checking jterm's terminal emulation and rendering (256 colors, truecolor, SGR attributes, wide chars/emoji, OSC title, bell, cursor key modes) plus the Step 11 compatibility checklist. Use when verifying MVP Steps 5–11 or debugging a rendering/input bug.
---

# VT test scripts

All scripts are plain bash + printf in `.claude/skills/vt-tests/scripts/`. Run them **inside
jterm** (by the user, or with `verify`'s `smoke.ps1 -Keys "bash <script>{ENTER}" -Screenshot …`),
then compare with the expectation below. Running them in another terminal first shows what
correct output looks like.

| Script | Step | Correct result |
|---|---|---|
| `colors-256.sh` | 5 | 16 rows of 16 swatches; rows 1–2 match the theme's ANSI colors, then a smooth 6×6×6 cube, then a gray ramp |
| `truecolor.sh [cols]` | 5 | four smooth gradients (red, green, blue, rainbow), no banding |
| `attributes.sh` | 5 | each line looks like its label; "hidden" is blank |
| `unicode.sh` | 5, 11 | right box borders aligned; CJK and emoji take 2 cells each |
| `title-bell.sh` | 10 | title becomes `jterm-osc2-test`; one visual bell flash |
| `cursor-keys.sh` | 6 | `od -c` shows `033 [ A` for Up normally, `033 O A` in application mode; Ctrl+C is `003` |

SPEC acceptance one-liners that need no script:
- Step 6: `sleep 100` then Ctrl+C interrupts; Ctrl+D exits the shell.
- Step 7: resize the window, then `tput cols; tput lines`.
- Step 8: `seq 1 20000`, scroll up; output while scrolled up must not jump the view.
- Step 9: paste multi-line text into zsh — it must not auto-execute (bracketed paste).

## Step 11 compatibility checklist

Manual, by the user (Claude can't drive interactive TUIs reliably). Record each failure in
`KNOWN_ISSUES.md` with the reason, as SPEC Step 11 requires:

vim/nvim (insert, visual, colorschemes, `:set mouse=a`) · htop/btop · tmux (splits, status
colors, mouse) · less/man · ssh + remote vim · fzf (Ctrl+R) · lazygit or tig · `cat` a 50 MB
file (UI stays responsive) · CJK/emoji/box drawing alignment · Wayland and X11 launch.
