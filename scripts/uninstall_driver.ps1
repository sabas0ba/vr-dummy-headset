<#
.SYNOPSIS
Removes the driver folder registration from SteamVR using vrpathreg.
#>
[CmdletBinding()]
param(
    [string]$DriverDir = (Join-Path $PSScriptRoot "..\driver\build\msvc-x64\vrdummy"),
    [string]$SteamVRDir = "C:\Program Files (x86)\Steam\steamapps\common\SteamVR"
)

$ErrorActionPreference = "Stop"

$DriverDir = (Resolve-Path $DriverDir).Path
$vrpathreg = Join-Path $SteamVRDir "bin\win64\vrpathreg.exe"
if (-not (Test-Path $vrpathreg)) {
    throw "vrpathreg.exe not found at $vrpathreg. Pass -SteamVRDir."
}

& $vrpathreg removedriver $DriverDir
if ($LASTEXITCODE -ne 0) {
    throw "vrpathreg removedriver failed with exit code $LASTEXITCODE"
}
Write-Host "Removed driver: $DriverDir"
& $vrpathreg show
