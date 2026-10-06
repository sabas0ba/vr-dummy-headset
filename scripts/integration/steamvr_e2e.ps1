<#
.SYNOPSIS
Runs the end-to-end scenarios (vrdummy_e2e) against a real SteamVR.

.DESCRIPTION
  1. vrpathreg adddriver <driver folder>
  2. start SteamVR unless it is already running; wait for the virtual HMD
  3. start `vrdummy-cli run --no-stdin` (control pipe server)
  4. run vrdummy_e2e (OpenVR scene app with doctest scenarios)
  5. run a scenario file through `vrdummy-cli scenario` and check the result
  6. stop the feeder and check that trackers lose tracking
  7. stop SteamVR (if this script started it) and unregister the driver

Exit code 0 when every scenario passed.

.PARAMETER KeepRunning
Leave SteamVR running and the driver registered afterwards.

.PARAMETER JUnit
Write the vrdummy_e2e report as JUnit XML to this path.
#>
[CmdletBinding()]
param(
    [string]$DriverDir = "",
    [string]$Probe = "",
    [string]$E2E = "",
    [string]$Feeder = "",
    [string]$SteamVRDir = "C:\Program Files (x86)\Steam\steamapps\common\SteamVR",
    [string]$VRChatDir = "C:\Program Files (x86)\Steam\steamapps\common\VRChat\VRChat_Data\StreamingAssets\SteamVR",
    [string]$JUnit = "",
    [switch]$KeepRunning
)

$ErrorActionPreference = "Stop"

# Defaults resolved here: $PSScriptRoot is empty inside param() on PowerShell 5.1.
if ($DriverDir -eq "") { $DriverDir = Join-Path $PSScriptRoot "..\..\driver\build\msvc-x64\vrdummy" }
if ($Probe -eq "") { $Probe = Join-Path $PSScriptRoot "..\..\driver\build\msvc-x64\Release\vrdummy_probe.exe" }
if ($E2E -eq "") { $E2E = Join-Path $PSScriptRoot "..\..\driver\build\msvc-x64\Release\vrdummy_e2e.exe" }
if ($Feeder -eq "") { $Feeder = Join-Path $PSScriptRoot "..\..\feeder\target\release\vrdummy-cli.exe" }
$scenarioFile = Join-Path $PSScriptRoot "scenarios\walk.txt"

$failures = 0
function Step($name, [scriptblock]$body) { Write-Host "== $name"; & $body }
function Expect($label, $ok) {
    if ($ok) { Write-Host "  PASS $label" } else { Write-Host "  FAIL $label"; $script:failures++ }
}

foreach ($p in @($DriverDir, $Probe, $E2E, $Feeder, $scenarioFile)) {
    if (-not (Test-Path $p)) { throw "not found: $p (build the driver and feeder first)" }
}
$DriverDir = (Resolve-Path $DriverDir).Path
$vrpathreg = Join-Path $SteamVRDir "bin\win64\vrpathreg.exe"
$vrstartup = Join-Path $SteamVRDir "bin\win64\vrstartup.exe"
foreach ($p in @($vrpathreg, $vrstartup)) { if (-not (Test-Path $p)) { throw "not found: $p" } }

$steamvrWasRunning = $null -ne (Get-Process -Name vrserver -ErrorAction SilentlyContinue)
$feederLog = Join-Path $env:TEMP "vrdummy-feeder-e2e.log"
$feederProc = $null

try {
    Step "back up chaperone settings" {
        # SteamVR stores room setup per universe id; the tools below only write
        # the vrdummy universe, but a copy is kept so that any real headset's
        # room setup can be restored by hand (docs/coordinates.md).
        $chaperone = Join-Path (Split-Path $SteamVRDir -Parent | Split-Path -Parent | Split-Path -Parent) "config\chaperone_info.vrchap"
        if (Test-Path $chaperone) {
            $backupDir = Join-Path $PSScriptRoot "..\..\.tmp\chaperone-backup"
            New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
            $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
            Copy-Item $chaperone (Join-Path $backupDir "chaperone_info-$stamp.vrchap")
            Write-Host "  copied $chaperone to $backupDir"
        } else {
            Write-Host "  no chaperone_info.vrchap found at $chaperone"
        }
    }

    Step "register driver" {
        & $vrpathreg adddriver $DriverDir | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "vrpathreg adddriver failed" }
    }

    Step "start SteamVR" {
        if (-not $steamvrWasRunning) { Start-Process -FilePath $vrstartup | Out-Null }
        & $Probe wait VRDUMMY-HMD-0 90
        Expect "virtual HMD visible to OpenVR clients" ($LASTEXITCODE -eq 0)
    }

    Step "chaperone: standing origin = raw origin, 3 m x 3 m" {
        & $Probe chaperone 3
        Expect "chaperone committed" ($LASTEXITCODE -eq 0)
        $state = (& $Probe chaperone-state) -join ""
        Write-Host "  $state"
        Expect "calibration ok" ($state -match "calibration=ok")
    }

    Step "start feeder" {
        $script:feederProc = Start-Process -FilePath $Feeder -ArgumentList @("--level", "debug", "run", "--no-stdin") `
            -PassThru -WindowStyle Hidden -RedirectStandardError $feederLog -RedirectStandardOutput (Join-Path $env:TEMP "vrdummy-feeder-e2e.out")
        Start-Sleep -Seconds 2
        $status = & $Feeder set status
        Write-Host "  $status"
        Expect "feeder connected to driver" ($status -match "connected=true")
    }

    Step "e2e scenarios" {
        $args = @("--no-colors", "-tce=S9*")
        if ($JUnit -ne "") { $args += @("--reporters=junit", "--out=$JUnit") }
        & $E2E @args
        Expect "vrdummy_e2e scenarios" ($LASTEXITCODE -eq 0)
    }

    # S9 needs VRChat's action manifest, shipped with the game. The e2e app
    # identifies itself as VRChat so that the driver's default binding loads.
    $vrchatActions = Join-Path $VRChatDir "actions.json"
    if (Test-Path $vrchatActions) {
        Step "e2e scenario S9: VRChat default bindings" {
            $env:VRDUMMY_E2E_VRCHAT_ACTIONS = $vrchatActions
            try {
                $args = @("--no-colors", "-tc=S9*")
                if ($JUnit -ne "") { $args += @("--reporters=junit", "--out=" + [System.IO.Path]::ChangeExtension($JUnit, ".vrchat.xml")) }
                & $E2E @args
                Expect "VRChat bindings" ($LASTEXITCODE -eq 0)
            } finally {
                Remove-Item Env:\VRDUMMY_E2E_VRCHAT_ACTIONS -ErrorAction SilentlyContinue
            }
        }
    } else {
        Write-Host "== S9 skipped: VRChat not found at $VRChatDir"
    }

    Step "scenario file via vrdummy-cli scenario" {
        # A second feeder instance would conflict on the control pipe, so the
        # resident one is stopped first and restarted afterwards.
        & $Feeder set quit | Out-Null
        Start-Sleep -Seconds 1
        & $Feeder scenario $scenarioFile
        Expect "scenario completed" ($LASTEXITCODE -eq 0)
        & $Probe expect VRDUMMY-HMD-0 1.0 1.6 -1.0 0.02
        Expect "hmd at the scenario's final position" ($LASTEXITCODE -eq 0)
    }

    Step "trackers lose tracking without the feeder" {
        Start-Sleep -Seconds 1
        $line = (& $Probe pose VRDUMMY-TRK-3) -join ""
        Write-Host "  $line"
        Expect "tracker out of range" ($line -match "result=out_of_range")
        $hmd = (& $Probe pose VRDUMMY-HMD-0) -join ""
        Expect "hmd keeps its last pose" ($hmd -match "valid=1")
    }

    if (-not $steamvrWasRunning) {
        Step "chaperone persists across a SteamVR restart" {
            Get-Process -Name vrmonitor, vrserver, vrcompositor, vrdashboard, vrwebhelper, vrstartup -ErrorAction SilentlyContinue | Stop-Process -Force
            Start-Sleep -Seconds 5
            Start-Process -FilePath $vrstartup | Out-Null
            & $Probe wait VRDUMMY-HMD-0 90 | Out-Null
            Start-Sleep -Seconds 2
            $state = (& $Probe chaperone-state) -join ""
            Write-Host "  $state"
            Expect "calibration ok after restart without re-commit" ($state -match "calibration=ok")
            Expect "play area kept" ($state -match "play_area=3\.00x3\.00")
        }
    }
} finally {
    if ($null -ne $feederProc -and -not $feederProc.HasExited) { Stop-Process -Id $feederProc.Id -Force }
    if (-not $KeepRunning) {
        Step "cleanup" {
            if (-not $steamvrWasRunning) {
                Get-Process -Name vrmonitor, vrserver, vrcompositor, vrdashboard, vrwebhelper, vrstartup -ErrorAction SilentlyContinue | Stop-Process -Force
            }
            & $vrpathreg removedriver $DriverDir | Out-Null
        }
    }
}

if (Test-Path $feederLog) {
    Write-Host "== feeder log ($feederLog), last lines"
    Get-Content $feederLog | Select-Object -Last 10 | ForEach-Object { Write-Host "  | $_" }
}
Write-Host "== result: $failures failure(s)"
exit $(if ($failures -eq 0) { 0 } else { 1 })
