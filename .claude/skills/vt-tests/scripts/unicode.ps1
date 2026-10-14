# Steps 5 and 11: wide characters and box drawing must stay aligned to the grid.
# The right-hand borders of all three boxes should line up in one column.
Write-Host '┌──────────┐'
Write-Host '│ABCDEFGHIJ│  ASCII (10 cells)'
Write-Host '│日本語中文│  CJK (5 chars, 10 cells)'
Write-Host '│😀🚀👍🎉✨│  emoji (5 chars, 10 cells)'
Write-Host '└──────────┘'
Write-Host '╔═╦═╗ ▀▄█░▒▓ ⣿⠿⠛'
Write-Host '╚═╩═╝ block + braille'
