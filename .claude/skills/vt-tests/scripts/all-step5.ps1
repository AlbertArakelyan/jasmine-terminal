# Runs every Step 5 rendering check in one screen, plus a colored directory listing
# (PowerShell 7.2+ colors Get-ChildItem output, the equivalent of `ls --color`).
$here = $PSScriptRoot
& "$here/colors-256.ps1"
& "$here/truecolor.ps1"
& "$here/attributes.ps1"
& "$here/unicode.ps1"
Get-ChildItem "$here/../.."   # colored directory names
