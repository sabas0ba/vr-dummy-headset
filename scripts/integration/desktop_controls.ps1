<#
.SYNOPSIS
Tests desktop input leases, rig editing and window queries against a CLI or GUI feeder.
.DESCRIPTION
Uses isolated driver/control pipes and preferences below .tmp. Does not launch
SteamVR or VRChat or move any real window. Keeps a control pipe connected to
exercise frame updates without depending on CLI process startup latency.
#>
[CmdletBinding()]
param([string]$Cli = "", [string]$Gui = "")
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if ($Cli -eq '') { $Cli = Join-Path $repo 'feeder/target/debug/vrdummy-cli.exe' }
$Cli = (Resolve-Path -LiteralPath $Cli).Path
if ($Gui -ne '') { $Gui = (Resolve-Path -LiteralPath $Gui).Path }
$testDir = Join-Path $repo ".tmp/desktop-controls-$PID"
New-Item -ItemType Directory -Force -Path $testDir | Out-Null
$config = Join-Path $testDir 'config.toml'
$pipeName = "vrdummy-desktop-control-$PID"
Set-Content -Encoding ascii -LiteralPath $config -Value @(
    '[driver]', "pipe_name = 'vrdummy-desktop-no-driver-$PID'",
    '[control]', "pipe_name = '$pipeName'"
)
$savedStateDir = $env:VRDUMMY_STATE_DIR
$server = $null
$pipe = $null
$reader = $null
$writer = $null

function Request([string]$Line, [switch]$Rejected) {
    $writer.WriteLine($Line)
    $task = $reader.ReadLineAsync()
    if (-not $task.Wait(3000)) { throw "Control response timed out: $Line" }
    $reply = $task.Result
    if ($Rejected) {
        if ($reply -notlike 'err *') { throw "Expected rejection: $Line => $reply" }
    } elseif ($reply -ne 'ok' -and $reply -notlike 'ok *') {
        throw "Command failed: $Line => $reply"
    }
    return $reply
}
function Snapshot {
    $reply = Request 'status --json'
    return $reply.Substring(3) | ConvertFrom-Json
}
function InputValue($State, [int]$Device, [int]$Component) {
    $deviceState = $State.devices | Where-Object { $_.id -eq $Device }
    return ($deviceState.inputs | Where-Object { $_.component_id -eq $Component }).value
}
function Check([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
try {
    $env:VRDUMMY_STATE_DIR = Join-Path $testDir 'preferences'
    $executable = if ($Gui -ne '') { $Gui } else { $Cli }
    $arguments = @('--config', ('"{0}"' -f $config))
    if ($Gui -eq '') { $arguments += @('run', '--no-stdin') }
    $server = Start-Process -FilePath $executable -ArgumentList $arguments -WorkingDirectory $repo `
        -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $testDir 'server.out') `
        -RedirectStandardError (Join-Path $testDir 'server.err')

    $json = & $Cli --config $config set --json --timeout-ms 5000 window status
    Check ($LASTEXITCODE -eq 0) "Window JSON command failed: $json"
    $window = $json | ConvertFrom-Json
    Check ($window.ok -and $window.result.schema_version -eq 1) 'Window status is not a structured result'
    Check ($null -eq $window.result.saved_position) 'Test did not isolate saved placement'

    $pipe = [System.IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(5000)
    $reader = [System.IO.StreamReader]::new($pipe)
    $writer = [System.IO.StreamWriter]::new($pipe)
    $writer.AutoFlush = $true
    Request 'input ctrl-l trigger/value 0.75' | Out-Null
    Request 'control begin agent walk' | Out-Null
    Request 'control frame agent 1 1 0 15 10 1 1 1 1 0.05' | Out-Null
    $state = Snapshot
    Check ($state.interaction.owner -eq 'agent') 'Lease owner missing'
    $x = InputValue $state 1 11
    $y = InputValue $state 1 12
    Check ([Math]::Abs($x * $x + $y * $y - 1) -lt 0.00001) 'Diagonal stick magnitude is not normalized'
    Request 'pos hmd 0 2 0' -Rejected | Out-Null
    Request 'control begin gui walk' -Rejected | Out-Null
    Request 'control frame gui 0 0 0 0 0 0 0 0 0 0.05' -Rejected | Out-Null
    Request 'control stop' | Out-Null
    $state = Snapshot
    Check ($null -eq $state.interaction.owner) 'Stop retained ownership'
    foreach ($component in @(1, 5, 6, 8, 9, 11, 12)) {
        Check ((InputValue $state 2 $component) -eq 0) "Stop retained right input $component"
    }
    Check ((InputValue $state 1 5) -eq 0.75) 'Stop changed unrelated manual input'
    Request 'control begin agent walk' | Out-Null
    Request 'control frame agent 0 1 0 0 0 0 0 0 0 0.05' | Out-Null
    Start-Sleep -Milliseconds 750
    $state = Snapshot
    Check ($null -eq $state.interaction.owner -and (InputValue $state 1 12) -eq 0) 'Lease expiry retained motion'
    Request 'control frame agent 0 1 0 0 0 0 0 0 0 0.05' -Rejected | Out-Null

    Request 'preset stand' | Out-Null
    $before = Snapshot
    Request 'edit checkpoint' | Out-Null
    Request 'rig translate 1 0 -2' | Out-Null
    $after = Snapshot
    foreach ($device in $before.devices) {
        $moved = $after.devices | Where-Object { $_.id -eq $device.id }
        Check ([Math]::Abs($moved.position_m[0] - $device.position_m[0] - 1) -lt 0.00001) 'Rig translation X mismatch'
        Check ([Math]::Abs($moved.position_m[2] - $device.position_m[2] + 2) -lt 0.00001) 'Rig translation Z mismatch'
    }
    Request 'fk ctrl-l 0 30 90' | Out-Null
    $state = Snapshot
    Check ($state.interaction.rig.chains.'1'.Count -eq 3) 'FK chain missing'
    Request 'ik ctrl-r 10 10 10' | Out-Null
    $state = Snapshot
    $chain = $state.interaction.rig.chains.'2'
    $distanceSquared = 0.0
    for ($i = 0; $i -lt 3; $i++) { $distanceSquared += [Math]::Pow($chain[2][$i] - $chain[0][$i], 2) }
    Check ([Math]::Sqrt($distanceSquared) -le 0.560001) 'IK exceeded arm reach'
    Request 'edit undo' | Out-Null
    $after = Snapshot
    Check (($before.devices | ConvertTo-Json -Depth 12 -Compress) -eq ($after.devices | ConvertTo-Json -Depth 12 -Compress)) 'Undo did not restore poses'
    Request 'edit undo' -Rejected | Out-Null
    Request 'rig lengths arm NaN 1' -Rejected | Out-Null
    $after | ConvertTo-Json -Depth 12 | Set-Content -Encoding UTF8 (Join-Path $testDir 'snapshot.json')
    Write-Host "PASS: desktop lease, conflict, release, expiry, rig, FK/IK, undo, window JSON ($testDir)"
} finally {
    if ($null -ne $writer) { $writer.Dispose() }
    if ($null -ne $reader) { $reader.Dispose() }
    if ($null -ne $pipe) { $pipe.Dispose() }
    if ($null -ne $server -and -not $server.HasExited) {
        & $Cli --config $config set quit | Out-Null
        if (-not $server.WaitForExit(5000)) { Stop-Process -Id $server.Id -Force }
    }
    $env:VRDUMMY_STATE_DIR = $savedStateDir
}
$global:LASTEXITCODE = 0
