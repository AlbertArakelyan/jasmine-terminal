#!/usr/bin/env bash
# Step 5: the 256-color palette, 16 per row. Rows 0-1 are the theme's ANSI colors,
# 16-231 the 6x6x6 cube, 232-255 the grayscale ramp.
for i in $(seq 0 255); do
  printf '\e[48;5;%dm %3d \e[0m' "$i" "$i"
  [ $(( (i + 1) % 16 )) -eq 0 ] && printf '\n'
done
