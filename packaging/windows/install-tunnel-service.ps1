#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$ServiceBinary,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$HysteriaBinary,

    [string]$DesktopUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
)

$ErrorActionPreference = "Stop"
$ServiceName = "KetTunnel"
$InstallDir = Join-Path $env:ProgramFiles "Ket"
$DataDir = Join-Path $env:ProgramData "Ket"
$RuntimeDir = Join-Path $DataDir "runtime"
$ServiceTarget = Join-Path $InstallDir "ket-tunnel-service.exe"
$HysteriaTarget = Join-Path $InstallDir "hysteria.exe"
$TokenFile = Join-Path $DataDir "tunnel.token"

function Invoke-CheckedNative {
    param([string]$FilePath, [string[]]$Arguments)
    & $FilePath @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath failed with exit code $LASTEXITCODE"
    }
}

function Copy-IfDifferent {
    param([string]$Source, [string]$Destination)
    $SourcePath = [System.IO.Path]::GetFullPath($Source)
    $DestinationPath = [System.IO.Path]::GetFullPath($Destination)
    if (-not $SourcePath.Equals($DestinationPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        Copy-Item -LiteralPath $SourcePath -Destination $DestinationPath -Force
    }
}

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($null -ne $existing) {
    if ($existing.Status -ne "Stopped") {
        Stop-Service -Name $ServiceName -Force
        $existing.WaitForStatus("Stopped", [TimeSpan]::FromSeconds(20))
    }
    Invoke-CheckedNative "sc.exe" @("delete", $ServiceName)
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        if ($null -eq (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue)) { break }
        Start-Sleep -Milliseconds 250
    }
}

New-Item -ItemType Directory -Force -Path $InstallDir, $DataDir, $RuntimeDir | Out-Null
Copy-IfDifferent -Source $ServiceBinary -Destination $ServiceTarget
Copy-IfDifferent -Source $HysteriaBinary -Destination $HysteriaTarget
& $HysteriaTarget "version" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "The Hysteria engine failed its version check" }

Invoke-CheckedNative "icacls.exe" @(
    $DataDir, "/inheritance:r",
    "/grant:r", "SYSTEM:(F)", "BUILTIN\Administrators:(F)", "${DesktopUser}:(RX)"
)
Invoke-CheckedNative "icacls.exe" @(
    $RuntimeDir, "/inheritance:r",
    "/grant:r", "SYSTEM:(OI)(CI)(F)", "BUILTIN\Administrators:(OI)(CI)(F)"
)

if (-not (Test-Path -LiteralPath $TokenFile -PathType Leaf)) {
    & $ServiceTarget "--init-token"
    if ($LASTEXITCODE -ne 0) { throw "Failed to initialize the broker token" }
}
if ((Get-Item -LiteralPath $TokenFile).Length -ne 32) {
    throw "The existing broker token is invalid; it was not overwritten"
}
Invoke-CheckedNative "icacls.exe" @(
    $TokenFile, "/inheritance:r",
    "/grant:r", "SYSTEM:(F)", "BUILTIN\Administrators:(F)", "${DesktopUser}:(R)"
)

$service = New-Service `
    -Name $ServiceName `
    -BinaryPathName ('"' + $ServiceTarget + '"') `
    -DisplayName "Ket Tunnel Service" `
    -Description "Authenticated privileged tunnel broker for the Ket desktop client" `
    -StartupType Automatic
Invoke-CheckedNative "sc.exe" @(
    "failure", $ServiceName, "reset=", "86400", "actions=", "restart/5000/restart/15000/none/0"
)
Invoke-CheckedNative "sc.exe" @("failureflag", $ServiceName, "1")
Start-Service -Name $ServiceName
$service.WaitForStatus("Running", [TimeSpan]::FromSeconds(20))

Write-Host "Installed and started the Ket tunnel service for $DesktopUser."
