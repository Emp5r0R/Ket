#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Container })]
    [string]$InstallDir,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$ServiceBinary,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$HysteriaBinary,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$ShadowsocksBinary,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$XrayBinary,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$WstunnelBinary,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Tun2ProxyBinary,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$WintunLibrary,

    [string]$DesktopUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
)

$ErrorActionPreference = "Stop"
$ServiceName = "KetTunnel"
$DataDir = Join-Path $env:ProgramData "Ket"
$RuntimeDir = Join-Path $DataDir "runtime"
$ServiceTarget = Join-Path $InstallDir "ket-tunnel-service.exe"
$HysteriaTarget = Join-Path $InstallDir "hysteria.exe"
$ShadowsocksTarget = Join-Path $InstallDir "sslocal.exe"
$XrayTarget = Join-Path $InstallDir "xray.exe"
$WstunnelTarget = Join-Path $InstallDir "wstunnel.exe"
$Tun2ProxyTarget = Join-Path $InstallDir "tun2proxy.exe"
$WintunTarget = Join-Path $InstallDir "wintun.dll"
$TokenFile = Join-Path $DataDir "tunnel.token"
$InstallLog = Join-Path $DataDir "install-service.log"

New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
Set-Content -LiteralPath $InstallLog -Value "Ket tunnel service installation started."

trap {
    $Failure = $_.Exception.Message
    Add-Content -LiteralPath $InstallLog -Value "FAILED: $Failure" -ErrorAction SilentlyContinue
    [Console]::Error.WriteLine("Ket tunnel service installation failed: $Failure")
    exit 1
}

function Write-InstallStage {
    param([string]$Message)
    Write-Host $Message
    Add-Content -LiteralPath $InstallLog -Value $Message
}

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
    Write-InstallStage "Removing the previous Ket tunnel service registration."
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

Write-InstallStage "Preparing Ket tunnel service files."
New-Item -ItemType Directory -Force -Path $InstallDir, $DataDir, $RuntimeDir | Out-Null
Copy-IfDifferent -Source $ServiceBinary -Destination $ServiceTarget
Copy-IfDifferent -Source $HysteriaBinary -Destination $HysteriaTarget
Copy-IfDifferent -Source $ShadowsocksBinary -Destination $ShadowsocksTarget
Copy-IfDifferent -Source $XrayBinary -Destination $XrayTarget
Copy-IfDifferent -Source $WstunnelBinary -Destination $WstunnelTarget
Copy-IfDifferent -Source $Tun2ProxyBinary -Destination $Tun2ProxyTarget
Copy-IfDifferent -Source $WintunLibrary -Destination $WintunTarget
Write-InstallStage "Validating bundled tunnel engines."
& $HysteriaTarget "version" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "The Hysteria engine failed its version check" }
& $ShadowsocksTarget "--version" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "The Shadowsocks engine failed its version check" }
& $XrayTarget "version" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "The Xray engine failed its version check" }
& $WstunnelTarget "--version" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "The wstunnel engine failed its version check" }
& $Tun2ProxyTarget "--version" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "The full-route bridge failed its version check" }

Write-InstallStage "Applying Ket data directory permissions."
Invoke-CheckedNative "icacls.exe" @(
    $DataDir, "/inheritance:r",
    "/grant:r", "SYSTEM:(F)", "BUILTIN\Administrators:(F)", "${DesktopUser}:(RX)"
)
Invoke-CheckedNative "icacls.exe" @(
    $InstallLog, "/inheritance:r",
    "/grant:r", "SYSTEM:(F)", "BUILTIN\Administrators:(F)", "${DesktopUser}:(R)"
)
Invoke-CheckedNative "icacls.exe" @(
    $RuntimeDir, "/inheritance:r",
    "/grant:r", "SYSTEM:(OI)(CI)(F)", "BUILTIN\Administrators:(OI)(CI)(F)"
)

Write-InstallStage "Initializing the Ket broker token."
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

Write-InstallStage "Registering the Ket tunnel service."
New-Service `
    -Name $ServiceName `
    -BinaryPathName ('"' + $ServiceTarget + '"') `
    -DisplayName "Ket Tunnel Service" `
    -Description "Authenticated privileged tunnel broker for the Ket desktop client" `
    -StartupType Automatic | Out-Null
Invoke-CheckedNative "sc.exe" @(
    "failure", $ServiceName, "reset=", "86400", "actions=", "restart/5000/restart/15000/none/0"
)
Invoke-CheckedNative "sc.exe" @("failureflag", $ServiceName, "1")
Write-InstallStage "Starting the Ket tunnel service."
Start-Service -Name $ServiceName
$service = Get-Service -Name $ServiceName
$service.WaitForStatus("Running", [TimeSpan]::FromSeconds(20))

Write-InstallStage "Ket tunnel service installation completed."
Write-Host "Installed and started the Ket tunnel service for $DesktopUser."
