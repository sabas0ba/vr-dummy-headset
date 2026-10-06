<#
.SYNOPSIS
End-to-end check against a running SteamVR: register the driver, start
SteamVR, drive the feeder and read the resulting poses back through OpenVR.

.DESCRIPTION
Steps:
  1. vrpathreg adddriver <driver folder>
  2. start SteamVR (vrstartup.exe) unless it is already running
  3. wait until the virtual HMD is visible to an OpenVR client (vrdummy_probe)
  4. start `vrdummy-cli run --no-stdin` in the background
  5. send commands through the control pipe and verify positions with
     `vrdummy_probe expect`
  6. stop the feeder, optionally stop SteamVR and unregister the driver

Exit code 0 means every expectation matched.

.PARAMETER DriverDir
Driver folder (contains driver.vrdrivermanifest).

.PARAMETER Probe
Path of vrdummy_probe.exe.

.PARAMETER Feeder
Path of vrdummy-cli.exe.

.PARAMETER SteamVRDir
SteamVR installation folder.

.PARAMETER KeepRunning
Leave SteamVR running and the driver registered after the check.
#>
[CmdletBinding()]
param(
    [string]$DriverDir = (Join-Path $PSScriptRoot "..\..\driver\build\msvc-x64\vrdummy"),
    [string]$Probe = (Join-Path $PSScriptRoot "..\..\driver\build\msvc-x64\Release\vrdummy_probe.exe"),
    [string]$Feeder = (Join-Path $PSScriptRoot "..\..\feeder\target\release\vrdummy-cli.exe"),
    [string]$SteamVRDir = "C:\Program Files (x86)\Steam\steamapps\common\SteamVR",
    [switch]$KeepRunning
)

$ErrorActionPreference = "Stop"
$failures = 0

function Step($name, [scriptblock]$body) {
    Write-Host "== $name"
    & $body
}

function Expect($label, $ok) {
    if ($ok) { Write-Host "  PASS $label" } else { Write-Host "  FAIL $label"; $script:failures++ }
}

$DriverDir = (Resolve-Path $DriverDir).Path
$Probe = (Resolve-Path $Probe).Path
$Feeder = (Resolve-Path $Feeder).Path
$vrpathreg = Join-Path $SteamVRDir "bin\win64\vrpathreg.exe"
$vrstartup = Join-Path $SteamVRDir "bin\win64\vrstartup.exe"
foreach ($p in @($vrpathreg, $vrstartup)) { if (-not (Test-Path $p)) { throw "not found: $p" } }

$steamvrWasRunning = $null -ne (Get-Process -Name vrserver -ErrorAction SilentlyContinue)

Step "register driver" {
    & $vrpathreg adddriver $DriverDir | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "vrpathreg adddriver failed" }
}

Step "start SteamVR" {
    if (-not $steamvrWasRunning) {
        Start-Process -FilePath $vrstartup | Out-Null
    }
    & $Probe wait VRDUMMY-HMD-0 90
    Expect "virtual HMD visible to OpenVR clients" ($LASTEXITCODE -eq 0)
}

Step "device list" {
    & $Probe list
    $list = (& $Probe list) -join "`n"
    Expect "left controller present" ($list -match "serial=VRDUMMY-CTRL-L")
    Expect "right controller present" ($list -match "serial=VRDUMMY-CTRL-R")
    Expect "tracker 3 present" ($list -match "serial=VRDUMMY-TRK-3")
}

$feederProc = $null
Step "start feeder" {
    $script:feederProc = Start-Process -FilePath $Feeder -ArgumentList @("--level", "debug", "run", "--no-stdin") -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 3
    $status = & $Feeder set status
    Write-Host "  $status"
    Expect "feeder connected to driver" ($status -match "connected=true")
}

Step "pose round trip" {
    & $Feeder set pos hmd 0.25 1.7 -0.5 | Out-Null
    & $Feeder set pos trk0 0.1 0.95 -0.2 | Out-Null
    & $Feeder set pos ctrl-r 0.3 1.2 -0.4 | Out-Null
    Start-Sleep -Milliseconds 500
    & $Probe expect VRDUMMY-HMD-0 0.25 1.7 -0.5 0.02
    Expect "hmd position" ($LASTEXITCODE -eq 0)
    & $Probe expect VRDUMMY-TRK-3 0.1 0.95 -0.2 0.02
    Expect "tracker position" ($LASTEXITCODE -eq 0)
    & $Probe expect VRDUMMY-CTRL-R 0.3 1.2 -0.4 0.02
    Expect "right controller position" ($LASTEXITCODE -eq 0)
}

Step "preset and display" {
    & $Feeder set preset tpose 1.8 | Out-Null
    & $Feeder set ipd 0.07 | Out-Null
    & $Feeder set swap 1 | Out-Null
    Start-Sleep -Milliseconds 500
    & $Probe pose VRDUMMY-CTRL-L
    & $Probe expect VRDUMMY-CTRL-L -0.738 1.476 0 0.02
    Expect "tpose left hand" ($LASTEXITCODE -eq 0)
}

Step "stop feeder" {
    & $Feeder set quit | Out-Null
    Start-Sleep -Seconds 1
    if ($null -ne $script:feederProc -and -not $script:feederProc.HasExited) { Stop-Process -Id $script:feederProc.Id -Force }
    Start-Sleep -Seconds 1
    $line = (& $Probe pose VRDUMMY-TRK-3) -join ""
    Write-Host "  $line"
    Expect "tracker out of range without feeder" ($line -match "result=out_of_range")
}

if (-not $KeepRunning) {
    Step "cleanup" {
        if (-not $steamvrWasRunning) {
            Get-Process -Name vrmonitor, vrserver, vrcompositor, vrdashboard, vrwebhelper, vrstartup -ErrorAction SilentlyContinue | Stop-Process -Force
        }
        & $vrpathreg removedriver $DriverDir | Out-Null
    }
}

Write-Host "== result: $failures failure(s)"
exit $(if ($failures -eq 0) { 0 } else { 1 })
