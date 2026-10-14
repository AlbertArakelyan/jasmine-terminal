# Launches target/debug/jterm.exe, checks the window, optionally types keys and takes
# a screenshot, then closes it and prints jterm's log lines.
#
#   pwsh -NoProfile -File .claude/skills/verify/scripts/smoke.ps1 [-Seconds 4] [-Keys "ls{ENTER}"] [-Screenshot out.png]
#
# -Keys uses WScript.Shell SendKeys syntax ({ENTER}, ^c for Ctrl+C, %x for Alt+X, {UP}, {TAB}).
# Separate groups with "||" to type them -KeyDelayMs apart (pwsh -File can't pass arrays), e.g.
#   -Keys "Start-Sleep 100{ENTER}||^c" -KeyDelayMs 1500
# -StartDelayMs waits before the first group, e.g. for a slow shell profile to load.
# A group of the form "<resize 1300x800>" resizes the window (outer size, in pixels) instead of typing.
# "<wheel 5>" / "<wheel -5>" scrolls 5 notches up / down over the window centre ("<wheel +5 shift>" with Shift),
# posted as WM_MOUSEWHEEL so the user's real mouse cursor isn't moved.
# Exit code 1 if the process exited early or no window appeared.
param(
    [int]$Seconds = 4,
    [string]$Keys = "",
    [int]$KeyDelayMs = 800,
    [int]$StartDelayMs = 300,
    [string]$Screenshot = "",
    [string]$Exe = "target/debug/jterm.exe"
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Exe)) { Write-Output "FAIL: $Exe not found (run cargo build first)"; exit 1 }

Add-Type -Namespace Smoke -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool GetClientRect(System.IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(System.IntPtr h);
[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, System.IntPtr pid);
[DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
[DllImport("user32.dll")] public static extern bool BringWindowToTop(System.IntPtr h);
[DllImport("user32.dll")] public static extern bool SetWindowPos(System.IntPtr h, System.IntPtr after, int x, int y, int w, int ht, uint flags);
[DllImport("user32.dll")] public static extern bool PostMessage(System.IntPtr h, uint msg, System.IntPtr w, System.IntPtr l);
[DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
// Windows only lets the foreground thread change the foreground window. Attaching to its
// input queue for the call is the standard way for a test driver to bring a window forward.
public static bool Focus(System.IntPtr h) {
    var fg = GetForegroundWindow();
    if (fg == h) return true;
    uint me = GetCurrentThreadId(), them = GetWindowThreadProcessId(fg, System.IntPtr.Zero);
    AttachThreadInput(me, them, true);
    BringWindowToTop(h); SetForegroundWindow(h);
    AttachThreadInput(me, them, false);
    return GetForegroundWindow() == h;
}
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

        $groups = @($Keys -split [regex]::Escape("||") | Where-Object { $_ })
        if ($groups.Count -gt 0) {
            [void][Smoke.Win]::SetForegroundWindow($h); Start-Sleep -Milliseconds $StartDelayMs
            $shell = New-Object -ComObject WScript.Shell
            for ($i = 0; $i -lt $groups.Count; $i++) {
                if ($i -gt 0) { Start-Sleep -Milliseconds $KeyDelayMs }
                if ($groups[$i] -match '^<resize (\d+)x(\d+)>$') {
                    # SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
                    [void][Smoke.Win]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, [int]$Matches[1], [int]$Matches[2], 0x16)
                    $r = New-Object Smoke.Win+RECT; [void][Smoke.Win]::GetClientRect($h, [ref]$r)
                    Write-Output "resized: client=$($r.R)x$($r.B)"
                    continue
                }
                # Mouse in grid cells (0-based col,row), posted as window messages so the user's
                # real mouse isn't moved: "<click c,r>", "<dclick c,r>", "<drag c1,r1 c2,r2>".
                if ($groups[$i] -match '^<(click|dclick|drag) (\d+),(\d+)(?: (\d+),(\d+))?>$') {
                    # Default 14pt JetBrains Mono at 100%: 8x18 cells, 8pt padding.
                    $px = { param($c, $r) [IntPtr]((([int](8 + $r * 18 + 9)) -shl 16) -bor ([int](8 + $c * 8 + 2))) }
                    $from = & $px ([int]$Matches[2]) ([int]$Matches[3])
                    # Every button message is preceded by a move to the same spot, posted back to back:
                    # the real cursor is elsewhere, so Windows soon reports the pointer leaving, and egui
                    # ignores buttons while it thinks the pointer is outside the window.
                    $post = { param($msg, $w, $l) [void][Smoke.Win]::PostMessage($h, 0x0200, [IntPtr]($w -band 1), $l); [void][Smoke.Win]::PostMessage($h, $msg, [IntPtr]$w, $l); Start-Sleep -Milliseconds 40 }
                    & $post 0x0200 0 $from   # WM_MOUSEMOVE
                    switch ($Matches[1]) {
                        'click'  { & $post 0x0201 1 $from; & $post 0x0202 0 $from }
                        'dclick' { & $post 0x0201 1 $from; & $post 0x0202 0 $from; & $post 0x0201 1 $from; & $post 0x0202 0 $from }
                        'drag'   {
                            # End on the right half of the last cell so it's included.
                            $to = [IntPtr]((([int](8 + [int]$Matches[5] * 18 + 9)) -shl 16) -bor ([int](8 + [int]$Matches[4] * 8 + 6)))
                            & $post 0x0201 1 $from; & $post 0x0200 1 $to; & $post 0x0202 0 $to
                        }
                    }
                    Write-Output "mouse: $($groups[$i])"
                    continue
                }
                if ($groups[$i] -match '^<wheel ([+-]?\d+)( shift)?>$') {
                    $notches = [int]$Matches[1]
                    $keys = if ($Matches[2]) { 0x4 } else { 0 }   # MK_SHIFT
                    $w = New-Object Smoke.Win+RECT; [void][Smoke.Win]::GetWindowRect($h, [ref]$w)
                    $x = [int](($w.L + $w.R) / 2); $y = [int](($w.T + $w.B) / 2)
                    $lparam = [IntPtr](($y -shl 16) -bor ($x -band 0xFFFF))
                    # A real wheel happens with the pointer over the window, so egui knows where
                    # it is (mouse reports need the cell). WM_MOUSEMOVE takes client coordinates.
                    $c = New-Object Smoke.Win+RECT; [void][Smoke.Win]::GetClientRect($h, [ref]$c)
                    $move = [IntPtr]((([int]($c.B / 2)) -shl 16) -bor ([int]($c.R / 2)))
                    [void][Smoke.Win]::PostMessage($h, 0x0200, [IntPtr]::Zero, $move)   # WM_MOUSEMOVE
                    Start-Sleep -Milliseconds 50
                    $step = if ($notches -gt 0) { 120 } else { -120 }   # WHEEL_DELTA per notch
                    for ($n = 0; $n -lt [Math]::Abs($notches); $n++) {
                        $wparam = [IntPtr]((($step -band 0xFFFF) -shl 16) -bor $keys)
                        # Re-post the move right before each notch: see the note on mouse buttons below.
                        [void][Smoke.Win]::PostMessage($h, 0x0200, [IntPtr]::Zero, $move)
                        [void][Smoke.Win]::PostMessage($h, 0x020A, $wparam, $lparam)   # WM_MOUSEWHEEL
                        Start-Sleep -Milliseconds 30
                    }
                    Write-Output "wheel: $notches notches$($Matches[2])"
                    continue
                }
                # SendKeys types into whatever window is in front. Never type unless that's
                # jterm, or the keys could land in one of the user's own windows.
                $front = $false
                for ($try = 0; $try -lt 20 -and -not $front; $try++) {
                    [void][Smoke.Win]::Focus($h)
                    Start-Sleep -Milliseconds 50
                    $front = [Smoke.Win]::GetForegroundWindow() -eq $h
                }
                if (-not $front) {
                    Write-Output "FAIL: jterm isn't the foreground window; not sending: $($groups[$i])"
                    $status = 1
                    break
                }
                $shell.SendKeys($groups[$i])
                Write-Output "sent keys: $($groups[$i])"
            }
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
