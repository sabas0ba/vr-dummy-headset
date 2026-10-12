<#
.SYNOPSIS
Tests CLI registration with real vrpathreg and an isolated OpenVR registry.
.DESCRIPTION
Requires an installed SteamVR and a built driver. Does not start SteamVR or
modify the user's registration. Artifacts remain under the repository's .tmp.
#>
[CmdletBinding()]
param([string]$Cli = '', [string]$DriverDir = '', [string]$Gui = '')

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if ($Cli -eq '') { $Cli = Join-Path $repo 'feeder/target/debug/vrdummy-cli.exe' }
if ($DriverDir -eq '') { $DriverDir = Join-Path $repo 'driver/build/msvc-x64/vrdummy' }
$Cli = (Resolve-Path -LiteralPath $Cli).Path
$DriverDir = (Resolve-Path -LiteralPath $DriverDir).Path
if ($Gui -ne '') { $Gui = (Resolve-Path -LiteralPath $Gui).Path }
$guiProcess = $null
$originalOverride = $env:VR_PATHREG_OVERRIDE
$source = if ($originalOverride) { $originalOverride } else { Join-Path $env:LOCALAPPDATA 'openvr/openvrpaths.vrpath' }
$sourceHash = (Get-FileHash -LiteralPath $source).Hash
$testDir = Join-Path $repo ".tmp/registration-$PID"
New-Item -ItemType Directory -Force -Path $testDir | Out-Null
$package = Join-Path $testDir 'driver with spaces'
Copy-Item -LiteralPath $DriverDir -Destination $package -Recurse
$other = Join-Path $testDir 'other-driver'
New-Item -ItemType Directory -Force -Path $other | Out-Null
Set-Content -LiteralPath (Join-Path $other 'driver.vrdrivermanifest') -Encoding ascii -Value '{"name":"other"}'
$registry = Get-Content -LiteralPath $source -Raw -Encoding UTF8 | ConvertFrom-Json
$registry.external_drivers = @($other)
$isolated = Join-Path $testDir 'openvrpaths.vrpath'
[System.IO.File]::WriteAllText($isolated, ($registry | ConvertTo-Json -Depth 20))

function Invoke-Driver([string]$Action, [int]$ExpectedExit = 0, [string]$Path = $package) {
    $output = & $Cli driver $Action --path $Path --json --timeout-ms 5000
    if ($LASTEXITCODE -ne $ExpectedExit) { throw "Unexpected exit code $LASTEXITCODE : $output" }
    if (@($output).Count -ne 1) { throw "Expected single JSON response: $output" }
    $reply = $output | ConvertFrom-Json
    if ($reply.schema_version -ne 1 -or $reply.ok -ne ($ExpectedExit -eq 0)) { throw 'Invalid envelope' }
    return $reply
}

function Invoke-GuiButton([string]$Name) {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $guiProcess.Refresh()
        if ($guiProcess.HasExited) { throw 'GUI exited before registration test' }
        if ($guiProcess.MainWindowHandle -ne [IntPtr]::Zero) {
            $rootElement = [System.Windows.Automation.AutomationElement]::FromHandle($guiProcess.MainWindowHandle)
            $condition = New-Object System.Windows.Automation.PropertyCondition(
                [System.Windows.Automation.AutomationElement]::NameProperty, $Name)
            $button = $rootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
            if ($null -ne $button -and $button.Current.IsEnabled) {
                $pattern = $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
                $pattern.Invoke()
                return
            }
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "GUI button unavailable: $Name"
}

function Wait-Registration([bool]$Registered) {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        if ((Invoke-Driver 'status').result.registered -eq $Registered) { return }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "GUI did not change registration to $Registered"
}

try {
    $env:VR_PATHREG_OVERRIDE = $isolated
    if ((Invoke-Driver 'status').result.registered) { throw 'Unexpected initial registration' }
    $registered = (Invoke-Driver 'register').result
    if (-not $registered.registered -or -not $registered.changed -or -not $registered.restart_required) { throw 'Registration failed' }
    if ((Invoke-Driver 'register').result.changed) { throw 'Register is not idempotent' }
    if (-not (Invoke-Driver 'status').result.registered) { throw 'Registration not persisted' }
    if ($Gui -ne '') {
        Add-Type -AssemblyName UIAutomationClient
        Add-Type -AssemblyName UIAutomationTypes
        $config = Join-Path $testDir 'gui.toml'
        Set-Content -LiteralPath $config -Encoding ascii -Value @(
            '[driver]', "pipe_name = 'vrdummy-registration-no-driver-$PID'",
            '[control]', "pipe_name = 'vrdummy-registration-control-$PID'"
        )
        $guiProcess = Start-Process -FilePath $Gui -ArgumentList @('--config', ('"{0}"' -f $config)) `
            -WorkingDirectory $repo -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $testDir 'gui.out') -RedirectStandardError (Join-Path $testDir 'gui.err')
        Invoke-GuiButton 'Driver registration'
        Invoke-GuiButton 'Unregister'
        Wait-Registration $false
        Invoke-GuiButton 'Register'
        Wait-Registration $true
        Write-Host 'PASS: GUI Register / Unregister buttons changed the isolated registration'
    }
    $errorReply = Invoke-Driver 'unregister' 1 $other
    if ($errorReply.error.kind -ne 'driver_management_error') { throw 'Wrong error classification' }
    $removed = (Invoke-Driver 'unregister').result
    if ($removed.registered -or -not $removed.changed) { throw 'Unregistration failed' }
    if ((Invoke-Driver 'unregister').result.changed) { throw 'Unregister is not idempotent' }
    $after = Get-Content -LiteralPath $isolated -Raw -Encoding UTF8 | ConvertFrom-Json
    if (@($after.external_drivers).Count -ne 1 -or $after.external_drivers[0] -ne $other) { throw 'Other driver registration changed' }
    Write-Host "PASS: register, status, unregister, idempotence, other-driver protection ($testDir)"
} finally {
    if ($null -ne $guiProcess -and -not $guiProcess.HasExited) {
        $guiProcess.CloseMainWindow() | Out-Null
        if (-not $guiProcess.WaitForExit(5000)) { Stop-Process -Id $guiProcess.Id -Force }
    }
    $env:VR_PATHREG_OVERRIDE = $originalOverride
    if ((Get-FileHash -LiteralPath $source).Hash -ne $sourceHash) { throw 'Original registry changed during test' }
}
$global:LASTEXITCODE = 0
