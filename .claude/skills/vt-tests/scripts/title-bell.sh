#!/usr/bin/env bash
# Step 10: sets the window title (OSC 0 and OSC 2), then rings the bell (visual flash).
printf '\e]0;jterm-osc0-test\a'; sleep 1
printf '\e]2;jterm-osc2-test\a'; sleep 1
printf '\a'
echo "title should now be 'jterm-osc2-test'; the window should have flashed once"
