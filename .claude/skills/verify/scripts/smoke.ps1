# Launches target/debug/jterm.exe, checks the window, optionally types keys and takes
# a screenshot, then closes it and prints jterm's log lines.
#
#   pwsh -NoProfile -File .claude/skills/verify/scripts/smoke.ps1 [-Seconds 4] [-Keys "ls{ENTER}"] [-Screenshot out.png]
#
# -Keys uses WScript.Shell SendKeys syntax ({ENTER}, ^c for Ctrl+C, %x for Alt+X).
# Exit code 1 if the process exited early or no window appeared.
param(
    [int]$Seconds = 4,
    [string]$Keys = "",
    [string]$Screenshot = "",
    [string]$Exe = "target/debug/jterm.exe"
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Exe)) { Write-Output "FAIL: $Exe not found (run cargo build first)"; exit 1 }

Add-Type -Namespace Smoke -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool GetClientRect(System.IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(System.IntPtr h);
[DllImport("user32.dll")] public static extern bool PrintWindow(System.IntPtr h, System.IntPtr hdc, uint flags);
public struct RECT { public int L, T, R, B; }
'@

$log = Join-Path ([IO.Path]::GetTempPath()) "jterm-smoke-$PID.log"
$p = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $log
$status = 0
try {
    $deadline = (Get-Date).AddSeconds($Seconds)
    # winit first creates an untitled helper window; wait for the real, titled, sized one.
    $c = New-Object Smoke.Win+RECT
    while ((Get-Date) -lt $deadline -and -not $p.HasExited) {
        $p.Refresh()
        if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -and
            [Smoke.Win]::GetClientRect($p.MainWindowHandle, [ref]$c) -and $c.R -gt 0) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($p.HasExited) { Write-Output "FAIL: process exited early with code $($p.ExitCode)"; $status = 1 }
    elseif ($p.MainWindowHandle -eq 0 -or -not $p.MainWindowTitle) { Write-Output "FAIL: no titled window after ${Seconds}s"; $status = 1 }
    else {
        $h = $p.MainWindowHandle
        Write-Output "window: title='$($p.MainWindowTitle)' client=$($c.R)x$($c.B)"

        if ($Keys) {
            [void][Smoke.Win]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300
            (New-Object -ComObject WScript.Shell).SendKeys($Keys)
            Write-Output "sent keys: $Keys"
        }
        # Let the remaining time pass so output and repaints settle before capturing.
        $remaining = ($deadline - (Get-Date)).TotalMilliseconds
        if ($remaining -gt 0) { Start-Sleep -Milliseconds $remaining }

        if ($Screenshot) {
            Add-Type -AssemblyName System.Drawing
            $w = New-Object Smoke.Win+RECT; [void][Smoke.Win]::GetWindowRect($h, [ref]$w)
            $bmp = New-Object System.Drawing.Bitmap ($w.R - $w.L), ($w.B - $w.T)
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $hdc = $g.GetHdc()
            # PW_RENDERFULLCONTENT (2) captures GPU-rendered (wgpu) content even if covered.
            [void][Smoke.Win]::PrintWindow($h, $hdc, 2)
            $g.ReleaseHdc($hdc); $g.Dispose()
            $bmp.Save((Resolve-Path -LiteralPath (Split-Path -Parent ([IO.Path]::GetFullPath($Screenshot)))).Path + "\" + (Split-Path -Leaf $Screenshot))
            $bmp.Dispose()
            Write-Output "screenshot: $([IO.Path]::GetFullPath($Screenshot))"
        }
        $p.Refresh()
        if ($p.HasExited) { Write-Output "FAIL: process exited during the run (code $($p.ExitCode))"; $status = 1 }
        else { Write-Output "alive: yes" }
    }
}
finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    $p.WaitForExit()
    Write-Output "--- jterm log (jterm targets, warnings, errors) ---"
    if (Test-Path $log) {
        Get-Content $log | ForEach-Object { $_ -replace "\x1b\[[0-9;]*m", "" } |
            Select-String -Pattern "jterm|WARN|ERROR|panicked" | ForEach-Object { $_.Line }
        Remove-Item $log -ErrorAction SilentlyContinue
    }
}
exit $status
