#!/usr/bin/env bash
# SessionStart: tell Claude where the MVP stands, based on `mvp(step-N)` commit subjects.
cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0
last=$(git log --format=%s 2>/dev/null | grep -oE '^mvp\(step-[0-9]+\)' | grep -oE '[0-9]+' | sort -n | tail -1)
if [ -n "$last" ]; then
  echo "jterm MVP progress: steps 1-$last are committed. Next is Step $((last + 1)) in SPEC.md §5 (use /mvp-step $((last + 1)))."
else
  echo "jterm MVP progress: no mvp(step-N) commits yet. Start with Step 1 in SPEC.md §5."
fi
dirty=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
[ "$dirty" != "0" ] && echo "Working tree has $dirty uncommitted change(s); check \`git status\` before starting new work."
exit 0
