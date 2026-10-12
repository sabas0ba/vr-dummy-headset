<#
.SYNOPSIS
Verifies JSON control of a CLI or GUI feeder without SteamVR.
.DESCRIPTION
Uses isolated named pipes and writes all test artifacts below .tmp.
Pass -Gui to run the same contract against a GUI on an interactive desktop.
#>
[CmdletBinding()]
param([string]$Cli = "", [string]$Gui = "")

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
if ($Cli -eq "") { $Cli = Join-Path $repo "feeder/target/debug/vrdummy-cli.exe" }
$Cli = (Resolve-Path -LiteralPath $Cli).Path
if ($Gui -ne "") { $Gui = (Resolve-Path -LiteralPath $Gui).Path }
$testDir = Join-Path $repo ".tmp/agent-cli-$PID"
New-Item -ItemType Directory -Force -Path $testDir | Out-Null
$config = Join-Path $testDir "config.toml"
Set-Content -Encoding ascii -LiteralPath $config -Value @(
    '[driver]', "pipe_name = 'vrdummy-agent-no-driver-$PID'",
    '[control]', "pipe_name = 'vrdummy-agent-control-$PID'"
)
$server = $null

function Invoke-Json([string[]]$Command, [int]$ExpectedExit = 0) {
    $output = & $Cli --config $config --json @Command
    $actualExit = $LASTEXITCODE
    if ($actualExit -ne $ExpectedExit) { throw "Unexpected exit $actualExit (expected $ExpectedExit): $output" }
    if (@($output).Count -ne 1) { throw "Expected exactly one stdout JSON line: $output" }
    $reply = $output | ConvertFrom-Json
    if ($reply.schema_version -ne 1) { throw "Unsupported schema: $output" }
    if ($reply.ok -ne ($ExpectedExit -eq 0)) { throw "Status/exit disagreement: $output" }
    return $reply
}

Push-Location $repo
try {
    $executable = if ($Gui -ne "") { $Gui } else { $Cli }
    $arguments = @('--config', ('"{0}"' -f $config))
    if ($Gui -eq "") { $arguments += @('run', '--no-stdin') }
    $server = Start-Process -FilePath $executable -ArgumentList $arguments -WorkingDirectory $repo `
        -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $testDir "server.out") `
        -RedirectStandardError (Join-Path $testDir "server.err")
    $status = Invoke-Json @('status', '--timeout-ms', '5000')
    if ($status.result.connected -or $status.result.devices.Count -ne 6) { throw 'Unexpected initial state' }
    Invoke-Json @('set', 'pos', 'hmd', '1', '1.8', '-1') | Out-Null
    Invoke-Json @('set', 'input', 'ctrl-l', 'trigger/value', '0.75') | Out-Null
    Invoke-Json @('set', 'swap', '1') | Out-Null
    $status = Invoke-Json @('status')
    $hmd = $status.result.devices | Where-Object { $_.id -eq 0 }
    $left = $status.result.devices | Where-Object { $_.id -eq 1 }
    $trigger = $left.inputs | Where-Object { $_.component_id -eq 5 }
    if (($hmd.position_m -join ',') -ne '1,1.8,-1') { throw 'Position was not retained' }
    if ($trigger.value -ne 0.75 -or -not $status.result.display.swap_eyes) { throw 'Input/display state was not retained' }
    $status | ConvertTo-Json -Depth 12 | Set-Content -Encoding UTF8 (Join-Path $testDir 'snapshot.json')
    $errorReply = Invoke-Json @('set', 'unknown-command') 1
    if ($errorReply.error.kind -ne 'command_error') { throw 'Command error not classified' }
    $errorReply = Invoke-Json @('status', '--timeout-ms', '0') 2
    if ($errorReply.error.kind -ne 'argument_error') { throw 'Argument error not classified' }
    $legacy = & $Cli --config $config status
    if ($LASTEXITCODE -ne 0 -or $legacy -notlike 'ok connected=false*') { throw 'Text compatibility failed' }
    Invoke-Json @('set', 'quit') | Out-Null
    if (-not $server.WaitForExit(5000)) { throw 'Feeder did not shut down' }
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $errorReply = Invoke-Json @('status', '--timeout-ms', '100') 4
    if ($timer.Elapsed.TotalSeconds -gt 3 -or $errorReply.error.kind -ne 'timeout') { throw 'Deadline contract failed' }
    Write-Host "PASS: JSON snapshot, mutation, errors, text compatibility, shutdown and deadline ($testDir)"
} finally {
    if ($null -ne $server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
    Pop-Location
}

# The final negative test leaves a native failure code in PowerShell. GitHub
# Actions propagates it even when every assertion above succeeded.
$global:LASTEXITCODE = 0
