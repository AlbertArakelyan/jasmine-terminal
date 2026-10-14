# Step 5: 24-bit color. Should render as smooth gradients with no visible banding.
param([int]$Cols = 72)
$e = [char]27
foreach ($row in 'red', 'green', 'blue', 'rainbow') {
    $line = ''
    foreach ($i in 0..($Cols - 1)) {
        $v = [int]($i * 255 / ($Cols - 1))
        switch ($row) {
            'red'     { $r = $v; $g = 0; $b = 0 }
            'green'   { $r = 0; $g = $v; $b = 0 }
            'blue'    { $r = 0; $g = 0; $b = $v }
            'rainbow' { $r = 255 - $v; $g = if ($v -lt 128) { $v * 2 } else { 510 - $v * 2 }; $b = $v }
        }
        $line += "$e[48;2;${r};${g};${b}m "
    }
    Write-Host "$line$e[0m  $row"
}
