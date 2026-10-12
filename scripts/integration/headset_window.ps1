<#
.SYNOPSIS
Moves an existing SteamVR Headset Window and verifies saved placement on feeder restart.
.DESCRIPTION
Requires SteamVR with a dummy HMD already running. Preferences and pipes are
isolated below .tmp; the original window position is restored in finally.
Does not restart SteamVR or change its registration/settings.
#>
[CmdletBinding()]
param([string]$Cli = '')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if ($Cli -eq '') { $Cli = Join-Path $repo 'feeder/target/debug/vrdummy-cli.exe' }
$Cli = (Resolve-Path -LiteralPath $Cli).Path
$testDir = Join-Path $repo ".tmp/headset-window-$PID"
New-Item -ItemType Directory -Force -Path $testDir | Out-Null
$config = Join-Path $testDir 'config.toml'
Set-Content -Encoding ascii -LiteralPath $config -Value @(
    '[driver]', "pipe_name = 'vrdummy-window-no-driver-$PID'",
    '[control]', "pipe_name = 'vrdummy-window-control-$PID'"
)
$savedStateDir = $env:VRDUMMY_STATE_DIR
$server = $null
$original = $null
function Start-Feeder {
    $script:server = Start-Process -FilePath $Cli -ArgumentList @('--config', ('"{0}"' -f $config), 'run', '--no-stdin') `
        -WindowStyle Hidden -PassThru -WorkingDirectory $repo `
        -RedirectStandardOutput (Join-Path $testDir 'server.out') -RedirectStandardError (Join-Path $testDir 'server.err')
}
function Stop-Feeder {
    if ($null -ne $server -and -not $server.HasExited) {
        & $Cli --config $config set quit | Out-Null
        if (-not $server.WaitForExit(5000)) { Stop-Process -Id $server.Id -Force }
    }
}
function Window([string[]]$Arguments) {
    $output = & $Cli --config $config set --json --timeout-ms 5000 window @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Window command failed: $output" }
    return ($output | ConvertFrom-Json).result
}
function At($State, [int[]]$Position) {
    if ($State.windows.Count -ne 1 -or $State.windows[0].bounds[0] -ne $Position[0] -or $State.windows[0].bounds[1] -ne $Position[1]) {
        throw "Window position mismatch; expected $Position"
    }
}
try {
    $env:VRDUMMY_STATE_DIR = Join-Path $testDir 'preferences'
    Start-Feeder
    $initial = Window @('status')
    if ($initial.windows.Count -ne 1) { throw 'Expected one existing SteamVR Headset Window' }
    $original = @($initial.windows[0].bounds[0], $initial.windows[0].bounds[1])
    $monitor = $initial.monitors | Where-Object { $_.primary } | Select-Object -First 1
    $target = @(($monitor.bounds[0] + 40), ($monitor.bounds[1] + 40))
    At (Window @('move', $target[0], $target[1])) $target
    At (Window @('save', $target[0], $target[1])) $target
    # Replacing an existing preference exercises Windows rename-over-file.
    $target = @(($target[0] + 20), ($target[1] + 20))
    At (Window @('save', $target[0], $target[1])) $target
    At (Window @('move', $original[0], $original[1])) $original
    Stop-Feeder
    Start-Feeder
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $state = Window @('status')
        if ($state.windows[0].bounds[0] -eq $target[0] -and $state.windows[0].bounds[1] -eq $target[1]) { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    At $state $target
    if (($state.saved_position -join ',') -ne ($target -join ',')) { throw 'Preference did not survive feeder restart' }
    Window @('forget') | Out-Null
    if (Test-Path -LiteralPath (Join-Path $env:VRDUMMY_STATE_DIR 'headset-window.json')) { throw 'Forget left a preference file' }
    $state | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 (Join-Path $testDir 'placement.json')
    Write-Host "PASS: live window movement, saved replacement, feeder restart and forget ($testDir)"
} finally {
    try {
        if ($null -ne $original -and $null -ne $server -and -not $server.HasExited) {
            Window @('forget') | Out-Null
            At (Window @('move', $original[0], $original[1])) $original
        }
    } finally {
        Stop-Feeder
        $env:VRDUMMY_STATE_DIR = $savedStateDir
    }
}
$global:LASTEXITCODE = 0
