<#
.SYNOPSIS
Registers the built driver folder with SteamVR using vrpathreg.

.DESCRIPTION
Adds <repo>/driver/build/<preset>/vrdummy as an external driver. SteamVR must
be closed. Run uninstall_driver.ps1 to remove it.

.PARAMETER DriverDir
Path of the driver folder containing driver.vrdrivermanifest. Defaults to the
msvc-x64 preset output.

.PARAMETER SteamVRDir
SteamVR installation folder. Defaults to the Steam library under Program Files (x86).
#>
[CmdletBinding()]
param(
    [string]$DriverDir = (Join-Path $PSScriptRoot "..\driver\build\msvc-x64\vrdummy"),
    [string]$SteamVRDir = "C:\Program Files (x86)\Steam\steamapps\common\SteamVR"
)

$ErrorActionPreference = "Stop"

$DriverDir = (Resolve-Path $DriverDir).Path
$manifest = Join-Path $DriverDir "driver.vrdrivermanifest"
if (-not (Test-Path $manifest)) {
    throw "driver.vrdrivermanifest not found in $DriverDir. Build the driver first."
}

$vrpathreg = Join-Path $SteamVRDir "bin\win64\vrpathreg.exe"
if (-not (Test-Path $vrpathreg)) {
    throw "vrpathreg.exe not found at $vrpathreg. Pass -SteamVRDir."
}

& $vrpathreg adddriver $DriverDir
if ($LASTEXITCODE -ne 0) {
    throw "vrpathreg adddriver failed with exit code $LASTEXITCODE"
}
Write-Host "Registered driver: $DriverDir"
& $vrpathreg show
