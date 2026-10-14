#!/usr/bin/env bash
# PreToolUse (Bash): before any `git commit`, run the gates from SPEC.md
# (fmt, clippy -D warnings, tests). Exit 2 blocks the commit and shows Claude why.
# Commits that touch no Rust code or Cargo files skip the gates.
command -v jq >/dev/null || exit 0
cmd=$(jq -r '.tool_input.command // empty')
printf '%s' "$cmd" | grep -Eq '(^|[;&|[:space:]])git[[:space:]]+commit' || exit 0

cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0
changed=$( { git diff HEAD --name-only; git ls-files --others --exclude-standard; } 2>/dev/null )
printf '%s\n' "$changed" | grep -Eq '\.rs$|(^|/)Cargo\.(toml|lock)$|^assets/' || exit 0

fail() { printf 'Commit blocked: %s failed.\n%s\n' "$1" "$2" >&2; exit 2; }
out=$(cargo fmt --all --check 2>&1) || fail "cargo fmt --check" "$(printf '%s' "$out" | tail -30)"
out=$(cargo clippy --all-targets --quiet -- -D warnings 2>&1) || fail "cargo clippy -D warnings" "$(printf '%s' "$out" | tail -40)"
out=$(cargo test --quiet 2>&1) || fail "cargo test" "$(printf '%s' "$out" | tail -40)"
exit 0
