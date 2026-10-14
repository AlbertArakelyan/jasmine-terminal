#!/usr/bin/env bash
# Linux smoke test, the counterpart of smoke.ps1: launches jterm, optionally runs steps
# (typed keys, mouse, resize), takes a screenshot, prints the final title and jterm's log.
#
#   bash .claude/skills/verify/scripts/smoke.sh [-s seconds] [-k "steps"] [-d step_delay_ms]
#        [-w start_delay_ms] [-o shot.png] [-e exe] [--wayland]
#
# Steps are separated by "||": typed text with {keysym} keys ("ls{Return}", "{ctrl+c}";
# "{{" types a literal "{"),
# or <resize WxH>, <wheel N>, <wheel -N shift>, <click c,r>, <dclick c,r>,
# <drag c1,r1 c2,r2>, <sleep ms>. See linux/lib.sh. Example:
#   bash .claude/skills/verify/scripts/smoke.sh -s 8 -k "sleep 100{Return}||{ctrl+c}||echo ok{Return}" -o /tmp/s.png
#
# --wayland starts jterm natively on Wayland: launch check and grim screenshot only, no input.
# Exit code 1 if jterm died or never showed a window.
set -u
cd "$(dirname "$0")/../../../.." || exit 1
source .claude/skills/verify/scripts/linux/lib.sh

secs=4; steps=""; delay=800; start_delay=500; shot=""; mode=x11
while [ $# -gt 0 ]; do
    case $1 in
        -s) secs=$2; shift 2 ;;
        -k) steps=$2; shift 2 ;;
        -d) delay=$2; shift 2 ;;
        -w) start_delay=$2; shift 2 ;;
        -o) shot=$2; shift 2 ;;
        -e) JT_EXE=$2; shift 2 ;;
        --wayland) mode=--wayland; shift ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

begin=$(date +%s)
jt_start "$mode"
if [ "$mode" = x11 ]; then
    geo=$(xdotool getwindowgeometry "$JT_WID" | sed -n 's/.*Geometry: //p')
    echo "window: id=$JT_WID title='$(jt_title)' size=$geo cell=${JT_CELL_W}x${JT_CELL_H} ppp=$JT_PPP"
else
    echo "window: native Wayland (no window id), cell=${JT_CELL_W}x${JT_CELL_H} ppp=$JT_PPP"
fi

if [ -n "$steps" ]; then
    [ "$mode" = x11 ] || jt_die "steps need X11 (drop --wayland)"
    sleep "$(awk -v ms="$start_delay" 'BEGIN { print ms / 1000 }')"
    jt_steps "$steps" "$delay"
fi

remaining=$(( secs - ($(date +%s) - begin) ))
[ $remaining -gt 0 ] && sleep $remaining

status=0
if jt_alive; then
    echo "alive: yes"
    [ "$mode" = x11 ] && echo "final title: '$(jt_title)'"
    [ -n "$shot" ] && jt_shot "$shot" && echo "screenshot: $shot"
else
    echo "FAIL: jterm exited during the run"
    status=1
fi
jt_stop
echo "--- jterm log (jterm targets, warnings, errors) ---"
sed 's/\x1b\[[0-9;]*m//g' "$JT_LOG" | grep -E 'jterm|WARN|ERROR|panicked'
rm -f "$JT_LOG"
exit $status
