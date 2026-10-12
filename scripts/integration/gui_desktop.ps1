<#
.SYNOPSIS
Checks GUI capture, keyboard movement and fail-safe release on an interactive desktop.
.DESCRIPTION
Starts an isolated GUI, invokes its capture button through UI Automation, and
posts key messages only to that GUI window. Minimizes the test window to check
focus loss. Does not send global keystrokes or connect to SteamVR.
#>
[CmdletBinding()]
param([string]$Cli = '', [string]$Gui = '')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if ($Cli -eq '') { $Cli = Join-Path $repo 'feeder/target/debug/vrdummy-cli.exe' }
if ($Gui -eq '') { $Gui = Join-Path $repo 'feeder/target/debug/vrdummy-gui.exe' }
$Cli = (Resolve-Path -LiteralPath $Cli).Path
$Gui = (Resolve-Path -LiteralPath $Gui).Path
$testDir = Join-Path $repo ".tmp/gui-desktop-$PID"
New-Item -ItemType Directory -Force -Path $testDir | Out-Null
$config = Join-Path $testDir 'config.toml'
Set-Content -Encoding ascii -LiteralPath $config -Value @(
    '[driver]', "pipe_name = 'vrdummy-gui-input-no-driver-$PID'",
    '[control]', "pipe_name = 'vrdummy-gui-input-control-$PID'",
    '[log]', "level = 'debug'"
)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DesktopTestWindow {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window, uint message, UIntPtr key, IntPtr data);
    [DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint type);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, IntPtr process);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint from, uint to, bool attach);
    public static bool Focus(IntPtr window) {
        uint current = GetCurrentThreadId();
        uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
        bool attached = current != foreground && AttachThreadInput(current, foreground, true);
        try { ShowWindow(window, 9); return SetForegroundWindow(window); }
        finally { if (attached) AttachThreadInput(current, foreground, false); }
    }
}
'@
$server = $null
$savedStateDir = $env:VRDUMMY_STATE_DIR
function State {
    # Starting a console client can change foreground focus; query in-process.
    $pipe = [System.IO.Pipes.NamedPipeClientStream]::new('.', "vrdummy-gui-input-control-$PID", [System.IO.Pipes.PipeDirection]::InOut)
    try {
        $pipe.Connect(5000)
        $reader = [System.IO.StreamReader]::new($pipe)
        $writer = [System.IO.StreamWriter]::new($pipe)
        $writer.AutoFlush = $true
        $writer.WriteLine('status --json')
        $task = $reader.ReadLineAsync()
        if (-not $task.Wait(3000)) { throw 'GUI status deadline exceeded' }
        $output = $task.Result
        if ($output -notlike 'ok *') { throw "GUI status failed: $output" }
        return $output.Substring(3) | ConvertFrom-Json
    } finally { $pipe.Dispose() }
}
function Button([string]$Name) {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($server.MainWindowHandle)
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $Name)
    $element = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
    if ($null -eq $element) { throw "GUI element unavailable: $Name" }
    $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Capture {
    [DesktopTestWindow]::ShowWindow($server.MainWindowHandle, 9) | Out-Null
    [System.Windows.Automation.AutomationElement]::FromHandle($server.MainWindowHandle).SetFocus()
    [DesktopTestWindow]::Focus($server.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 150
    if ([DesktopTestWindow]::GetForegroundWindow() -ne $server.MainWindowHandle) {
        throw "Test GUI did not acquire focus (foreground=$([DesktopTestWindow]::GetForegroundWindow()), target=$($server.MainWindowHandle), title=$($server.MainWindowTitle)); use an unlocked interactive desktop"
    }
    Button 'Capture keyboard / mouse'
    Start-Sleep -Milliseconds 150
    if ((State).interaction.owner -ne 'gui') { throw 'Capture did not acquire GUI ownership' }
}
function Key([uint32]$Code, [bool]$Down) {
    $data = [int64]1 + ([int64][DesktopTestWindow]::MapVirtualKeyW($Code, 0) -shl 16)
    if (-not $Down) { $data = $data -bor 0xC0000000L }
    $message = if ($Down) { 0x100 } else { 0x101 }
    if (-not [DesktopTestWindow]::PostMessageW($server.MainWindowHandle, $message, [UIntPtr]::new($Code), [IntPtr]::new($data))) {
        throw 'Posting key message failed'
    }
    Start-Sleep -Milliseconds 100
}
function Forward($Snapshot) {
    $left = $Snapshot.devices | Where-Object { $_.id -eq 1 }
    return ($left.inputs | Where-Object { $_.component_id -eq 12 }).value
}
try {
    $env:VRDUMMY_STATE_DIR = Join-Path $testDir 'preferences'
    $server = Start-Process -FilePath $Gui -ArgumentList @('--config', ('"{0}"' -f $config)) `
        -WorkingDirectory $repo -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $testDir 'gui.out') -RedirectStandardError (Join-Path $testDir 'gui.err')
    State | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $server.Refresh()
        if ($server.MainWindowHandle -ne [IntPtr]::Zero -and $server.MainWindowTitle -eq 'vrdummy feeder') { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    Start-Sleep -Milliseconds 500
    Capture
    Key 0x57 $true
    if ((Forward (State)) -le 0) { throw 'W did not produce forward motion' }
    Key 0x1B $true
    Key 0x57 $false
    Key 0x1B $false
    $state = State
    if ($null -ne $state.interaction.owner -or (Forward $state) -ne 0) { throw 'Esc left movement active' }
    Capture
    Key 0x57 $true
    if ((Forward (State)) -le 0) { throw 'Recapture did not permit movement' }
    [DesktopTestWindow]::ShowWindow($server.MainWindowHandle, 6) | Out-Null
    Start-Sleep -Milliseconds 200
    $state = State
    if ($null -ne $state.interaction.owner -or (Forward $state) -ne 0) { throw 'Focus loss left movement active' }
    Key 0x57 $false
    [DesktopTestWindow]::ShowWindow($server.MainWindowHandle, 9) | Out-Null
    Start-Sleep -Milliseconds 200
    if ($null -ne (State).interaction.owner) { throw 'Focus return unexpectedly resumed capture' }
    Button 'Headset Window placement'
    Button 'ctrl-l'
    & $Cli --config $config set input ctrl-l joystick/x 0.6 | Out-Null
    & $Cli --config $config set input ctrl-l joystick/y 0.4 | Out-Null
    & $Cli --config $config set fk ctrl-l 15 30 60 | Out-Null
    Start-Sleep -Milliseconds 300
    & (Join-Path $repo 'scripts/capture_window.ps1') -ProcessName 'vrdummy-gui' -Out (Join-Path $testDir 'gui.png')
    Write-Host "PASS: GUI capture, W, Esc, focus loss and no automatic resume ($testDir)"
} finally {
    if ($null -ne $server -and -not $server.HasExited) {
        & $Cli --config $config set control stop | Out-Null
        & $Cli --config $config set quit | Out-Null
        if (-not $server.WaitForExit(5000)) { Stop-Process -Id $server.Id -Force }
    }
    $env:VRDUMMY_STATE_DIR = $savedStateDir
}
$global:LASTEXITCODE = 0
