<#
.SYNOPSIS
Configures, builds and tests the driver with the msvc-x64 CMake preset.

.PARAMETER Configuration
Release (default) or Debug.

.PARAMETER SkipTests
Skip ctest. Useful on machines where locally built executables cannot run.
#>
[CmdletBinding()]
param(
    [ValidateSet("Release", "Debug")]
    [string]$Configuration = "Release",
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

$driverDir = (Resolve-Path (Join-Path $PSScriptRoot "..\driver")).Path

$cmake = Get-Command cmake -ErrorAction SilentlyContinue
if ($null -eq $cmake) {
    $candidates = @(
        "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
        "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $cmake = $c; break }
    }
    if ($null -eq $cmake) { throw "cmake not found. Install CMake or the Visual Studio CMake component." }
} else {
    $cmake = $cmake.Source
}

Push-Location $driverDir
try {
    & $cmake --preset msvc-x64
    if ($LASTEXITCODE -ne 0) { throw "configure failed" }
    & $cmake --build --preset msvc-x64 --config $Configuration
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
    if (-not $SkipTests) {
        & $cmake --build --preset msvc-x64 --config $Configuration --target RUN_TESTS
        if ($LASTEXITCODE -ne 0) { throw "tests failed" }
    }
} finally {
    Pop-Location
}
