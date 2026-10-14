#!/usr/bin/env bash
# Runs the automatable part of the Linux checklist in KNOWN_ISSUES.md against a real jterm
# window and writes a report.
#
#   bash .claude/skills/verify/scripts/linux-checks.sh [-o out_dir] [-e exe] [only_check ...]
#
# Each check starts a fresh jterm, types into the user's shell ($SHELL; bash or zsh
# expected), and where possible verifies itself: the shell writes a result file and the
# script compares it. Checks it can't verify are LOOK: a screenshot to inspect (Read the PNG).
# Results: <out_dir>/report.md, plus screenshots and result files in <out_dir>.
#
# Runs under X11 (on a Wayland session jterm runs under XWayland for input; the native
# Wayland launch is checked separately). xdotool moves the real pointer and types into the
# focused window: run on a test machine or VM and don't touch the desktop while it runs.
#
# Needs: xdotool, xclip or wl-clipboard, ImageMagick (import) or xwd+convert, bc.
# Optional (checked when installed): vim, tmux, htop, btop, man, less, fzf, tig, lazygit,
# alacritty, grim (Wayland launch screenshot).
set -u
cd "$(dirname "$0")/../../../.." || exit 1
source .claude/skills/verify/scripts/linux/lib.sh

OUT=/tmp/jterm-checks-$(date +%Y%m%d-%H%M%S)
only=()
while [ $# -gt 0 ]; do
    case $1 in
        -o) OUT=$2; shift 2 ;;
        -e) JT_EXE=$2; shift 2 ;;
        *) only+=("$1"); shift ;;
    esac
done
mkdir -p "$OUT"
REPORT=$OUT/report.md
jt_need xdotool bc
command -v xclip >/dev/null || command -v wl-paste >/dev/null || jt_die "missing xclip or wl-clipboard"
[ -x "$JT_EXE" ] || cargo build || jt_die "build failed"

{
    echo "# jterm Linux checks — $(date '+%Y-%m-%d %H:%M')"
    echo
    echo "- Host: $(uname -sr), session: ${XDG_SESSION_TYPE:-unknown}, shell: ${SHELL:-?}"
    echo "- Exe: $JT_EXE"
    echo
    echo "| Check | Result | Details |"
    echo "|---|---|---|"
} >"$REPORT"

record() { # check result details
    printf '| %s | %s | %s |\n' "$1" "$2" "$3" >>"$REPORT"
    printf '%-14s %-5s %s\n' "$1" "$2" "$3"
}

# Starts jterm and waits for the shell prompt.
open() { jt_start; sleep "${PROMPT_WAIT:-1.5}"; }
close() { jt_stop; }
has() { command -v "$1" >/dev/null; }
want() { [ ${#only[@]} -eq 0 ] || [[ " ${only[*]} " == *" $1 "* ]]; }

check_launch_x11() {
    open
    jt_shot "$OUT/launch-x11.png"
    if jt_alive; then record launch-x11 LOOK "window $(xdotool getwindowgeometry "$JT_WID" | sed -n 's/.*Geometry: //p'), cell ${JT_CELL_W}x${JT_CELL_H} at scale $JT_PPP; prompt in launch-x11.png"
    else record launch-x11 FAIL "jterm exited"; fi
    close
}

check_launch_wayland() {
    if [ -z "${WAYLAND_DISPLAY:-}" ]; then record launch-wayland SKIP "not a Wayland session"; return; fi
    if ! has grim; then
        jt_start --wayland; local alive=no; jt_alive && alive=yes; jt_stop
        record launch-wayland "$([ $alive = yes ] && echo PASS || echo FAIL)" "native Wayland launch, alive=$alive (no grim for a screenshot)"
        return
    fi
    jt_start --wayland
    sleep 1.5
    grim "$OUT/launch-wayland.png"
    local alive=no; jt_alive && alive=yes
    jt_stop
    record launch-wayland LOOK "alive=$alive; whole-screen shot launch-wayland.png (jterm window should show the prompt)"
}

check_env() {
    open
    jt_steps "printf '%s %s %s\\n' \"\$TERM\" \"\$COLORTERM\" \"\$TERM_PROGRAM\" > $OUT/env.txt{Return}"
    jt_wait_file "$OUT/env.txt" 5
    local got; got=$(cat "$OUT/env.txt" 2>/dev/null)
    [ "$got" = "xterm-256color truecolor jterm" ] && record env PASS "$got" || record env FAIL "got '$got'"
    close
}

check_ctrl_c() {
    open
    jt_steps "sleep 100{Return}||{ctrl+c}||echo ok > $OUT/ctrlc.txt{Return}" 800
    jt_wait_file "$OUT/ctrlc.txt" 5 && record ctrl-c PASS "sleep 100 interrupted" || record ctrl-c FAIL "shell still blocked after Ctrl+C"
    close
}

check_ctrl_d() {
    open
    jt_steps "echo \$SHLVL > $OUT/shlvl0.txt{Return}||bash{Return}||{ctrl+d}||echo \$SHLVL > $OUT/shlvl1.txt{Return}" 800
    jt_wait_file "$OUT/shlvl1.txt" 5
    local a b; a=$(cat "$OUT/shlvl0.txt" 2>/dev/null); b=$(cat "$OUT/shlvl1.txt" 2>/dev/null)
    [ -n "$a" ] && [ "$a" = "$b" ] && record ctrl-d PASS "nested bash exited (SHLVL back to $a)" || record ctrl-d FAIL "SHLVL before=$a after=$b"
    close
}

check_resize() {
    open
    jt_steps "<resize 700x450>||echo \$(tput cols) \$(tput lines) > $OUT/size.txt{Return}" 1000
    jt_wait_file "$OUT/size.txt" 5
    local got w h expect
    got=$(cat "$OUT/size.txt" 2>/dev/null)
    read -r w h < <(xdotool getwindowgeometry "$JT_WID" | sed -n 's/.*Geometry: \([0-9]*\)x\([0-9]*\).*/\1 \2/p')
    expect=$(awk -v w="$w" -v h="$h" -v s="$JT_PPP" -v p="$JT_PAD" -v cw="$JT_CELL_W" -v ch="$JT_CELL_H" \
        'BEGIN { printf "%d %d", (w / s - 2 * p) / cw + 0.001, (h / s - 2 * p) / ch + 0.001 }')
    [ "$got" = "$expect" ] && record resize PASS "window ${w}x${h} → tput $got" || record resize FAIL "tput '$got', expected '$expect' for ${w}x${h}"
    close
}

check_title() {
    open
    # Shell prompts often set the title themselves, so read it while the command still runs.
    jt_steps "printf '\\033]0;jterm-title-ok\\007'; sleep 3{Return}"
    sleep 1
    local title; title=$(jt_title)
    [ "$title" = "jterm-title-ok" ] && record title PASS "OSC 0 set the window title" || record title FAIL "title is '$title'"
    close
}

check_bell() {
    open
    jt_steps "for i in \$(seq 60); do printf '\\a'; sleep 0.04; done{Return}"
    sleep 1
    jt_shot "$OUT/bell.png"
    record bell LOOK "bell.png should show the light flash overlay (BEL every 40 ms)"
    close
}

check_idle_cpu() {
    open
    sleep 2
    local a b; a=$(jt_cpu_ms); sleep 5; b=$(jt_cpu_ms)
    local used=$(( b - a ))
    [ $used -lt 150 ] && record idle-cpu PASS "${used} ms CPU in 5 s idle" || record idle-cpu FAIL "${used} ms CPU in 5 s idle (expected ~0)"
    close
}

check_colors() {
    local dir=.claude/skills/vt-tests/scripts
    open
    jt_steps "clear; bash $dir/colors-256.sh{Return}"; sleep 1; jt_shot "$OUT/colors-256.png"
    jt_steps "clear; bash $dir/truecolor.sh; bash $dir/attributes.sh{Return}"; sleep 1; jt_shot "$OUT/truecolor-attributes.png"
    jt_steps "clear; bash $dir/unicode.sh; ls --color=always /{Return}"; sleep 1; jt_shot "$OUT/unicode.png"
    record colors LOOK "colors-256.png, truecolor-attributes.png, unicode.png (expected results: .claude/skills/vt-tests/SKILL.md)"
    close
}

check_scrollback() {
    open
    jt_steps "clear; seq 1 3000{Return}||<wheel 5>" 1500
    sleep 0.5; jt_shot "$OUT/scroll-wheel.png"
    jt_steps "{shift+Prior}"; sleep 0.5; jt_shot "$OUT/scroll-pgup.png"
    jt_steps "x"; sleep 0.5; jt_shot "$OUT/scroll-typed.png"
    record scrollback LOOK "scroll-wheel.png: earlier numbers and a ↑15 badge; scroll-pgup.png: one page further; scroll-typed.png: back at the prompt"
    jt_steps "{BackSpace}"
    close
}

check_copy() {
    open
    jt_clipboard_set "before"
    jt_steps "clear; echo copy-me-12345 and-more{Return}||<drag 0,1 12,1>||{ctrl+shift+c}" 800
    sleep 0.5
    local got; got=$(jt_clipboard_get)
    [ "$got" = "copy-me-12345" ] && record copy-drag PASS "clipboard '$got'" || record copy-drag FAIL "clipboard '$got' (expected copy-me-12345)"
    jt_steps "<dclick 20,1>||{ctrl+shift+c}" 800
    sleep 0.5; got=$(jt_clipboard_get)
    [ "$got" = "and-more" ] && record copy-word PASS "double-click copied '$got'" || record copy-word FAIL "clipboard '$got' (expected and-more)"
    close
}

check_paste() {
    open
    rm -f "$OUT/paste-ran"
    jt_clipboard_set "touch $OUT/paste-ran
echo second-line"
    jt_steps "{ctrl+shift+v}"
    sleep 2
    jt_shot "$OUT/paste.png"
    if [ -e "$OUT/paste-ran" ]; then record paste FAIL "pasted text ran (no bracketed paste?)"
    else record paste PASS "multi-line paste did not run; paste.png shows both lines at the prompt"; fi
    jt_steps "{ctrl+c}"
    close
}

check_vim() {
    if ! has vim; then record vim SKIP "vim not installed"; return; fi
    open
    rm -f "$OUT/vim.txt"
    jt_steps "vim -u NONE -N $OUT/vim.txt{Return}||ihello{Escape}||hhx||{Left}x||:wq{Return}" 700
    sleep 1
    local got; got=$(cat "$OUT/vim.txt" 2>/dev/null)
    [ "$got" = "hlo" ] && record vim-keys PASS "insert, hjkl, arrows, :wq" || record vim-keys FAIL "file '$got' (expected hlo)"

    seq 1 200 >"$OUT/nums.txt"
    jt_steps "vim -u NONE -N -c 'set mouse=a' $OUT/nums.txt{Return}||<click 1,9>||:call writefile([line('.')], '$OUT/vim-click.txt'){Return}||<wheel -3>||:call writefile([line('w0')], '$OUT/vim-wheel.txt'){Return}" 900
    sleep 0.5
    jt_shot "$OUT/vim-mouse.png"
    jt_steps ":q!{Return}"
    got=$(cat "$OUT/vim-click.txt" 2>/dev/null)
    [ "$got" = "10" ] && record vim-click PASS "click on row 9 → line 10" || record vim-click FAIL "cursor line '$got' (expected 10)"
    got=$(cat "$OUT/vim-wheel.txt" 2>/dev/null)
    [ -n "$got" ] && [ "$got" -gt 1 ] && record vim-wheel PASS "wheel scrolled to top line $got" || record vim-wheel FAIL "top line '$got' after wheel down"
    close
}

check_tmux() {
    if ! has tmux; then record tmux SKIP "tmux not installed"; return; fi
    open
    jt_steps "tmux -f /dev/null new -s jtcheck{Return}||tmux set -g mouse on{Return}||{ctrl+b}%||tmux display -p '#{{window_panes}' > $OUT/tmux-panes.txt{Return}||<click 2,3>||tmux display -p '#{{pane_index}' > $OUT/tmux-pane.txt{Return}" 900
    sleep 0.5
    jt_shot "$OUT/tmux.png"
    jt_steps "tmux kill-session -t jtcheck{Return}"
    local panes pane; panes=$(cat "$OUT/tmux-panes.txt" 2>/dev/null); pane=$(cat "$OUT/tmux-pane.txt" 2>/dev/null)
    [ "$panes" = "2" ] && record tmux-split PASS "2 panes" || record tmux-split FAIL "panes '$panes'"
    [ "$pane" = "0" ] && record tmux-mouse PASS "click selected the left pane" || record tmux-mouse FAIL "active pane '$pane' after clicking the left pane"
    record tmux-look LOOK "tmux.png: split, status bar colors"
    close
}

# Opens a full-screen program, screenshots it, quits with $3.
look_tui() { # name command quit_keys
    if ! has "$1"; then record "$1" SKIP "not installed"; return; fi
    open
    jt_steps "$2{Return}" ; sleep 2.5
    jt_shot "$OUT/$1.png"
    jt_steps "<wheel -3>"; sleep 0.5; jt_shot "$OUT/$1-wheel.png"
    jt_steps "$3"
    record "$1" LOOK "$1.png (colors, layout), $1-wheel.png (after wheel down)"
    close
}

check_bigcat() {
    local big=$OUT/big.txt bench=$OUT/bench.sh
    head -c 37500000 /dev/urandom | base64 -w 120 >"$big"
    cat >"$bench" <<EOF
#!/bin/bash
start=\$(date +%s.%N); cat $big; end=\$(date +%s.%N)
echo "\$end - \$start" | bc > $OUT/time-\$1.txt
EOF
    chmod +x "$bench"
    open
    local cpu0; cpu0=$(jt_cpu_ms)
    jt_steps "$bench jterm{Return}"
    sleep 2; jt_shot "$OUT/bigcat-running.png"
    local resp_worst=0 t0 t1 ms
    while ! [ -e "$OUT/time-jterm.txt" ]; do
        t0=$(date +%s%N); xdotool getwindowname "$JT_WID" >/dev/null; t1=$(date +%s%N)
        ms=$(( (t1 - t0) / 1000000 )); [ $ms -gt $resp_worst ] && resp_worst=$ms
        sleep 0.25
        jt_alive || break
    done
    local tj; tj=$(cat "$OUT/time-jterm.txt" 2>/dev/null)
    local cpu=$(( $(jt_cpu_ms) - cpu0 ))
    close
    local detail="jterm ${tj}s, CPU ${cpu} ms, worst X round trip ${resp_worst} ms"
    if has alacritty; then
        rm -f "$OUT/time-alacritty.txt"
        alacritty -e "$bench" alacritty >/dev/null 2>&1
        local ta; ta=$(cat "$OUT/time-alacritty.txt" 2>/dev/null)
        detail="$detail; alacritty ${ta}s"
        if [ -n "$tj" ] && [ -n "$ta" ] && [ "$(echo "$tj <= 2 * $ta" | bc)" = 1 ]; then record big-cat PASS "$detail (target: within 2×)"
        else record big-cat FAIL "$detail (target: within 2×)"; fi
    else
        record big-cat LOOK "$detail; alacritty not installed for comparison; bigcat-running.png mid-run"
    fi
    rm -f "$big"
}

record_manual() {
    record ssh MANUAL "ssh to a host, run vim there: colors, mouse, resize"
    record ime MANUAL "IME input (e.g. ibus/fcitx) composes and commits text"
    record feel MANUAL "typing latency and scrolling feel smooth"
}

want launch-x11 && check_launch_x11
want launch-wayland && check_launch_wayland
want env && check_env
want ctrl-c && check_ctrl_c
want ctrl-d && check_ctrl_d
want resize && check_resize
want title && check_title
want bell && check_bell
want idle-cpu && check_idle_cpu
want colors && check_colors
want scrollback && check_scrollback
want copy && check_copy
want paste && check_paste
want vim && check_vim
want tmux && check_tmux
want htop && look_tui htop htop "{F10}"
want btop && look_tui btop btop "q"
want man && look_tui man "man ls" "q"
want less && look_tui less "less /etc/services" "q"
want fzf && look_tui fzf "seq 1 500 | fzf" "{Escape}"
want tig && look_tui tig "tig" "q"
want lazygit && look_tui lazygit "lazygit" "q"
want big-cat && check_bigcat
want manual && record_manual

echo
echo "report: $REPORT"
