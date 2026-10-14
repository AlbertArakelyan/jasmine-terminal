#!/usr/bin/env bash
# Steps 5 and 11: wide characters and box drawing must stay aligned to the grid.
# The right-hand borders of all three boxes should line up in one column.
printf '┌──────────┐\n'
printf '│ABCDEFGHIJ│  ASCII (10 cells)\n'
printf '│日本語中文│  CJK (5 chars, 10 cells)\n'
printf '│😀🚀👍🎉✨│  emoji (5 chars, 10 cells)\n'
printf '└──────────┘\n'
printf '╔═╦═╗ ▀▄█░▒▓ ⣿⠿⠛\n╚═╩═╝ block + braille\n'
