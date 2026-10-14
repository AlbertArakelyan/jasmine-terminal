---
name: vt-tests
description: Escape-sequence test scripts for checking jterm's terminal emulation and rendering (256 colors, truecolor, SGR attributes, wide chars/emoji, OSC title, bell, cursor key modes) plus the Step 11 compatibility checklist. Use when verifying MVP Steps 5–11 or debugging a rendering/input bug.
---

# VT test scripts

Scripts live in `.claude/skills/vt-tests/scripts/`, in two flavours: `.ps1` for PowerShell (the
default shell on the Windows dev machine; the user wants to keep it) and `.sh` for bash/zsh.
Run them **inside jterm**, then compare with the expectations below. Running them in another
terminal first shows what correct output looks like.

How to run them:
- **With keyboard input (Step 6+):** use `verify`'s `smoke.ps1 -Keys "& '<abs path>\<script>.ps1'{ENTER}" -Screenshot …`.
- **Without keyboard input:** make the script the shell. Back up the config, set
  `[shell] program = 'pwsh.exe'` and `args = ['-NoLogo', '-NoProfile', '-NoExit', '-File', '<abs path>/<script>.ps1']`,
  take a screenshot, then restore the config byte-for-byte.

The window shows 35 rows, so run the scripts one at a time, or use `all-step5.ps1` and accept
that the top scrolls away.

| Script | Step | Correct result |
|---|---|---|
| `colors-256.ps1` / `.sh` | 5 | The `.ps1` first prints 0–255 as colored numbers (the SPEC one-liner). Both print 16 rows of 16 swatches: the theme's ANSI colors, then a smooth 6×6×6 cube, then a gray ramp |
| `truecolor.ps1` / `.sh [cols]` | 5 | four smooth gradients (red, green, blue, rainbow), no banding |
| `attributes.ps1` / `.sh` | 5 | each line looks like its label; "hidden" is blank. The `.ps1` also checks tab stops (8/16/24/32) and an unbroken multi-color underline |
| `unicode.ps1` / `.sh` | 5, 11 | right box borders aligned; CJK and emoji take 2 cells each. CJK and Braille show as boxes until a system fallback font is loaded (Step 11) |
| `all-step5.ps1` | 5 | all of the above plus a colored `Get-ChildItem` (the PowerShell `ls --color`) |
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
