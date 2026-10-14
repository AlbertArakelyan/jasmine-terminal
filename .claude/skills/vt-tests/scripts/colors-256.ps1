# Step 5: the 256-color palette as foreground numbers (the SPEC one-liner), then as
# background swatches, 16 per row. Rows 1-2 are the theme's ANSI colors, then the
# 6x6x6 cube, then the gray ramp.
$e = [char]27
foreach ($i in 0..255) { Write-Host -NoNewline ("$e[38;5;${i}m{0,3} " -f $i) }
Write-Host "$e[0m"
foreach ($i in 0..255) {
    Write-Host -NoNewline ("$e[48;5;${i}m {0,3} $e[0m" -f $i)
    if (($i + 1) % 16 -eq 0) { Write-Host }
}
