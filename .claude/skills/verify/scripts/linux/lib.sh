#!/usr/bin/env bash
# Shared helpers for driving jterm on Linux: launch it, find its window, type keys, click and
# scroll at grid cells, resize, screenshot, measure CPU. Sourced by smoke.sh and
# linux-checks.sh.
#
# Input injection uses xdotool, which needs X11. On a Wayland session jterm is started under
# XWayland (WAYLAND_DISPLAY unset) so xdotool can reach it; `jt_start --wayland` starts it
# natively for launch checks, where only screenshots (grim) work.
#
# Requirements: xdotool, and ImageMagick (`import`) or xwd+convert for screenshots;
# xclip (X11) or wl-clipboard (Wayland) for clipboard checks; grim for native Wayland shots.
#
# xdotool moves the real pointer and types into the focused window, so run this on a test
# machine or VM, not while working on the same desktop.

JT_EXE=${JT_EXE:-target/debug/jterm}
JT_PAD=${JT_PAD:-8}          # window.padding from config.toml, in points
JT_PID=""
JT_WID=""
JT_LOG=""
JT_CELL_W=8; JT_CELL_H=18; JT_PPP=1   # read from jterm's log once it starts

jt_die() { echo "FAIL: $*" >&2; exit 1; }

jt_need() {
    local tool
    for tool in "$@"; do
        command -v "$tool" >/dev/null || jt_die "missing tool: $tool"
    done
}

# Starts jterm and waits for its window. --wayland starts it natively under Wayland.
jt_start() {
    local mode=${1:-x11}
    [ -x "$JT_EXE" ] || jt_die "$JT_EXE not found (run cargo build first)"
    JT_LOG=$(mktemp -t jterm-log.XXXXXX)
    if [ "$mode" = "--wayland" ]; then
        [ -n "${WAYLAND_DISPLAY:-}" ] || jt_die "--wayland needs a Wayland session"
        RUST_LOG=${RUST_LOG:-info,wgpu_hal=error} "$JT_EXE" 2>"$JT_LOG" &
        JT_PID=$!
        sleep 3
        kill -0 "$JT_PID" 2>/dev/null || jt_die "jterm exited early (see $JT_LOG)"
        jt_read_metrics
        return
    fi
    jt_need xdotool
    # Unsetting WAYLAND_DISPLAY makes winit use X11 (XWayland on a Wayland session).
    env -u WAYLAND_DISPLAY RUST_LOG=${RUST_LOG:-info,wgpu_hal=error} "$JT_EXE" 2>"$JT_LOG" &
    JT_PID=$!
    local i
    for i in $(seq 1 100); do
        kill -0 "$JT_PID" 2>/dev/null || jt_die "jterm exited early (see $JT_LOG)"
        JT_WID=$(xdotool search --onlyvisible --pid "$JT_PID" 2>/dev/null | tail -1)
        [ -n "$JT_WID" ] && break
        sleep 0.1
    done
    [ -n "$JT_WID" ] || jt_die "no jterm window after 10 s"
    sleep 1   # first frame: fonts and cell metrics
    jt_read_metrics
}

# Cell size and scale come from jterm's own log line:
#   measured cell metrics font_size=14.0 pixels_per_point=1.25 cell_width=8.8 cell_height=18.4
jt_read_metrics() {
    local line
    line=$(sed 's/\x1b\[[0-9;]*m//g' "$JT_LOG" | grep -m1 'measured cell metrics') || return 0
    JT_PPP=$(sed -n 's/.*pixels_per_point=\([0-9.]*\).*/\1/p' <<<"$line")
    JT_CELL_W=$(sed -n 's/.*cell_width=\([0-9.]*\).*/\1/p' <<<"$line")
    JT_CELL_H=$(sed -n 's/.*cell_height=\([0-9.]*\).*/\1/p' <<<"$line")
}

jt_stop() {
    [ -n "$JT_PID" ] && kill "$JT_PID" 2>/dev/null
    wait "$JT_PID" 2>/dev/null
    JT_PID=""; JT_WID=""
}

jt_alive() { [ -n "$JT_PID" ] && kill -0 "$JT_PID" 2>/dev/null; }

jt_focus() { xdotool windowactivate --sync "$JT_WID" 2>/dev/null || xdotool windowfocus --sync "$JT_WID"; }

# Pixel position (relative to the window) of the centre of cell (col, row), or of its right
# edge with a third argument "right" (to include the cell when a drag ends there).
jt_cell_px() {
    local col=$1 row=$2 side=${3:-centre}
    awk -v c="$col" -v r="$row" -v w="$JT_CELL_W" -v h="$JT_CELL_H" -v p="$JT_PAD" \
        -v s="$JT_PPP" -v side="$side" 'BEGIN {
            fx = (side == "right") ? 0.85 : 0.5
            printf "%d %d\n", (p + (c + fx) * w) * s, (p + (r + 0.5) * h) * s
        }'
}

# Types one group. Text is typed literally; {keysym} sends a key (xdotool names), e.g.
# "ls -la{Return}", "{ctrl+c}", "{ctrl+shift+v}", "{Escape}:wq{Return}", "{shift+Prior}".
# "{{" types a literal "{" (a lone "}" is literal anyway): "tmux display -p '#{{pane_index}'".
jt_type() {
    local group=$1 rest token
    rest=$group
    while [ -n "$rest" ]; do
        if [[ $rest == '{{'* ]]; then
            xdotool type --clearmodifiers -- "{"
            rest=${rest:2}
        elif [[ $rest =~ ^\{([^}]+)\}(.*)$ ]]; then
            xdotool key --clearmodifiers "${BASH_REMATCH[1]}"
            rest=${BASH_REMATCH[2]}
        elif [[ $rest =~ ^([^{]+)(.*)$ ]]; then
            token=${BASH_REMATCH[1]}
            xdotool type --clearmodifiers --delay 15 -- "$token"
            rest=${BASH_REMATCH[2]}
        else
            # A lone "{" that doesn't start a key name: type it.
            xdotool type -- "${rest:0:1}"
            rest=${rest:1}
        fi
    done
}

# Runs one step: a key group, or <resize WxH>, <wheel N>, <wheel N shift>, <click c,r>,
# <dclick c,r>, <drag c1,r1 c2,r2> (0-based cells), <sleep ms>.
jt_step() {
    local step=$1 x y
    jt_focus
    if [[ $step =~ ^\<resize\ ([0-9]+)x([0-9]+)\>$ ]]; then
        xdotool windowsize --sync "$JT_WID" "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
    elif [[ $step =~ ^\<wheel\ ([+-]?[0-9]+)(\ shift)?\>$ ]]; then
        local n=${BASH_REMATCH[1]} button=4
        [ "$n" -lt 0 ] && { button=5; n=$(( -n )); }
        read -r x y < <(jt_cell_px 20 5)
        xdotool mousemove --window "$JT_WID" "$x" "$y"
        [ -n "${BASH_REMATCH[2]}" ] && xdotool keydown shift
        xdotool click --repeat "$n" --delay 30 "$button"
        [ -n "${BASH_REMATCH[2]}" ] && xdotool keyup shift
    elif [[ $step =~ ^\<(click|dclick)\ ([0-9]+),([0-9]+)\>$ ]]; then
        read -r x y < <(jt_cell_px "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}")
        xdotool mousemove --window "$JT_WID" "$x" "$y"
        if [ "${BASH_REMATCH[1]}" = dclick ]; then
            xdotool click --repeat 2 --delay 80 1
        else
            xdotool click 1
        fi
    elif [[ $step =~ ^\<drag\ ([0-9]+),([0-9]+)\ ([0-9]+),([0-9]+)\>$ ]]; then
        read -r x y < <(jt_cell_px "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}")
        local x2 y2
        read -r x2 y2 < <(jt_cell_px "${BASH_REMATCH[3]}" "${BASH_REMATCH[4]}" right)
        xdotool mousemove --window "$JT_WID" "$x" "$y" mousedown 1
        xdotool mousemove --window "$JT_WID" "$x2" "$y2"
        sleep 0.1
        xdotool mouseup 1
    elif [[ $step =~ ^\<sleep\ ([0-9]+)\>$ ]]; then
        sleep "$(awk -v ms="${BASH_REMATCH[1]}" 'BEGIN { print ms / 1000 }')"
    else
        jt_type "$step"
    fi
}

# Runs "step1||step2||..." with a delay (ms) between steps.
jt_steps() {
    local steps=$1 delay_ms=${2:-800} step first=1
    local IFS=$'\n'
    for step in $(sed 's/||/\n/g' <<<"$steps"); do
        [ -z "$step" ] && continue
        [ $first = 1 ] || sleep "$(awk -v ms="$delay_ms" 'BEGIN { print ms / 1000 }')"
        first=0
        jt_step "$step"
    done
}

# Screenshot of the jterm window (X11), or of the whole screen with grim (Wayland mode).
jt_shot() {
    local out=$1
    if [ -z "$JT_WID" ]; then
        jt_need grim
        grim "$out"
    elif command -v import >/dev/null; then
        import -window "$JT_WID" "$out"
    else
        jt_need xwd convert
        xwd -silent -id "$JT_WID" | convert xwd:- "$out"
    fi
}

jt_title() { xdotool getwindowname "$JT_WID"; }

# CPU time (user + system) of jterm so far, in milliseconds.
jt_cpu_ms() {
    local ticks
    ticks=$(awk '{ print $14 + $15 }' "/proc/$JT_PID/stat")
    echo $(( ticks * 1000 / $(getconf CLK_TCK) ))
}

# Waits up to $2 seconds for file $1 to exist.
jt_wait_file() {
    local file=$1 secs=${2:-10} i
    for i in $(seq 1 $(( secs * 10 ))); do
        [ -e "$file" ] && return 0
        sleep 0.1
    done
    return 1
}

jt_clipboard_get() {
    if [ -n "${WAYLAND_DISPLAY:-}" ] && command -v wl-paste >/dev/null; then
        wl-paste --no-newline
    else
        xclip -o -selection clipboard
    fi
}

jt_clipboard_set() {
    if [ -n "${WAYLAND_DISPLAY:-}" ] && command -v wl-copy >/dev/null; then
        printf '%s' "$1" | wl-copy
    else
        printf '%s' "$1" | xclip -selection clipboard
    fi
}
