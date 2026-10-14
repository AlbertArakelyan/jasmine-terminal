#!/usr/bin/env bash
# Linux/macOS smoke test: runs target/debug/jterm for N seconds and reports whether it
# stayed alive, plus its log lines. Window checks and screenshots are Windows-only
# (smoke.ps1); on X11, `xdotool search --name jterm` can confirm the window by hand.
#
#   bash .claude/skills/verify/scripts/smoke.sh [seconds]
secs=${1:-4}
exe=target/debug/jterm
[ -x "$exe" ] || { echo "FAIL: $exe not found (run cargo build first)"; exit 1; }
log=$(mktemp)
timeout "$secs" "$exe" 2>"$log"
code=$?
if [ "$code" -eq 124 ]; then echo "alive: yes (still running after ${secs}s)"; status=0
else echo "FAIL: exited early with code $code"; status=1; fi
echo "--- jterm log (jterm targets, warnings, errors) ---"
grep -E 'jterm|WARN|ERROR|panicked' "$log"
rm -f "$log"
exit $status
