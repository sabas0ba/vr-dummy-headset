<#
.SYNOPSIS
Saves a screenshot of one top-level window (only that window) as PNG.

.DESCRIPTION
Finds the window by process name and, optionally, a title fragment, and
renders it with PrintWindow (PW_RENDERFULLCONTENT), so the capture contains
only that window even when other windows overlap it.

.EXAMPLE
.\scripts\capture_window.ps1 -ProcessName vrdummy-gui -Out docs\images\gui.png
.\scripts\capture_window.ps1 -ProcessName vrcompositor -Title "Headset Window" -Out docs\images\headset.png
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ProcessName,
    [string]$Title = "",
    [Parameter(Mandatory = $true)][string]$Out,
    # Bring the window to the front and copy it from the screen instead of
    # PrintWindow. Needed for GPU-composited windows (Qt, Chromium) that
    # PrintWindow renders blank.
    [switch]$ScreenCopy
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win32Capture {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmd);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hWnd, int attr, out RECT rect, int size);
    // Visible frame without the invisible resize border (DWMWA_EXTENDED_FRAME_BOUNDS = 9).
    public static bool FrameBounds(IntPtr hWnd, out RECT rect) {
        if (DwmGetWindowAttribute(hWnd, 9, out rect, Marshal.SizeOf(typeof(RECT))) == 0) return true;
        return GetWindowRect(hWnd, out rect);
    }
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder text, int count);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    public static System.Collections.Generic.List<IntPtr> WindowsOf(uint pid) {
        var list = new System.Collections.Generic.List<IntPtr>();
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) list.Add(h); return true; }, IntPtr.Zero);
        return list;
    }
    public static string TitleOf(IntPtr h) {
        int n = GetWindowTextLength(h);
        var sb = new System.Text.StringBuilder(n + 1);
        GetWindowText(h, sb, sb.Capacity);
        return sb.ToString();
    }
}
"@

# Physical pixel coordinates, so that window rectangles and screen copies agree
# on high-DPI displays and the capture is not downscaled.
[Win32Capture]::SetProcessDPIAware() | Out-Null

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if ($null -eq $procs) { throw "process not found: $ProcessName" }

$hwnd = [IntPtr]::Zero
foreach ($p in $procs) {
    foreach ($h in [Win32Capture]::WindowsOf([uint32]$p.Id)) {
        $t = [Win32Capture]::TitleOf($h)
        if ($Title -eq "" -or $t -like "*$Title*") { $hwnd = $h; break }
    }
    if ($hwnd -ne [IntPtr]::Zero) { break }
}
if ($hwnd -eq [IntPtr]::Zero) { throw "window not found for $ProcessName (title *$Title*)" }

if ([Win32Capture]::IsIconic($hwnd)) { [Win32Capture]::ShowWindow($hwnd, 9) | Out-Null; Start-Sleep -Milliseconds 300 }
$rect = New-Object Win32Capture+RECT
[Win32Capture]::FrameBounds($hwnd, [ref]$rect) | Out-Null
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
if ($w -le 0 -or $h -le 0) { throw "window has no size" }

$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$ok = $false
if (-not $ScreenCopy) {
    $hdc = $g.GetHdc()
    $ok = [Win32Capture]::PrintWindow($hwnd, $hdc, 2)  # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc)
}
if (-not $ok) {
    # Copy the screen region with the window temporarily made topmost so that
    # nothing overlaps it (SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW = 0x43).
    $topmost = [IntPtr](-1)
    $notopmost = [IntPtr](-2)
    [Win32Capture]::SetWindowPos($hwnd, $topmost, 0, 0, 0, 0, 0x43) | Out-Null
    [Win32Capture]::SetForegroundWindow($hwnd) | Out-Null
    # WScript.Shell.AppActivate is allowed to take the foreground where
    # SetForegroundWindow from a background process is not.
    try { (New-Object -ComObject WScript.Shell).AppActivate([Win32Capture]::TitleOf($hwnd)) | Out-Null } catch {}
    Start-Sleep -Milliseconds 600
    $g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bmp.Size)
    [Win32Capture]::SetWindowPos($hwnd, $notopmost, 0, 0, 0, 0, 0x43) | Out-Null
}
$g.Dispose()
$dir = Split-Path -Parent $Out
if ($dir -ne "" -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host ("saved {0} ({1}x{2}, '{3}')" -f $Out, $w, $h, [Win32Capture]::TitleOf($hwnd))
