#!/usr/bin/env bash
# PostToolUse (Edit|Write): format the edited Rust file so `cargo fmt --check` stays green.
# Never blocks: a file that doesn't parse yet is left for the compiler to report.
command -v jq >/dev/null || exit 0
f=$(jq -r '.tool_input.file_path // empty')
case "$f" in
  *.rs) [ -f "$f" ] && rustfmt --edition 2024 "$f" >/dev/null 2>&1 ;;
esac
exit 0
