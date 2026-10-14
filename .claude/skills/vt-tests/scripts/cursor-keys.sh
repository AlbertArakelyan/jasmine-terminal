#!/usr/bin/env bash
# Step 6: shows the exact bytes each key sends. Press keys, then Ctrl+D (twice if needed).
# Arrows send ESC [ A..D normally and ESC O A..D after `tput smkx` (application mode).
echo "Normal cursor mode. Press keys, Ctrl+D to stop:"
od -c
printf '\e[?1h'  # DECCKM on
echo "Application cursor mode. Press arrows, Ctrl+D to stop:"
od -c
printf '\e[?1l'
