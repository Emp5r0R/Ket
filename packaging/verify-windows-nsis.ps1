#Requires -Version 5.1
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Installer
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Fail {
    param([string]$Message)
    throw "Ket NSIS verification failed: $Message"
}

if ($env:KET_PACKAGE_TEST_ALLOW_HOST_MUTATION -ne "1") {
    Fail "set KET_PACKAGE_TEST_ALLOW_HOST_MUTATION=1 on an ephemeral test host"
}

$CurrentIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$CurrentPrincipal = [System.Security.Principal.WindowsPrincipal]::new($CurrentIdentity)
$AdministratorRole = [System.Security.Principal.WindowsBuiltInRole]::Administrator
if (-not $CurrentPrincipal.IsInRole($AdministratorRole)) {
    Fail "the lifecycle verifier must run as an administrator"
}

$InstallerPath = (Resolve-Path -LiteralPath $Installer).Path
$ServiceName = "KetTunnel"
$NativeProgramFiles = if ([string]::IsNullOrWhiteSpace($env:ProgramW6432)) {
    $env:ProgramFiles
}
else {
    $env:ProgramW6432
}
$InstallDir = Join-Path $NativeProgramFiles "Ket"
$DataDir = Join-Path $env:ProgramData "Ket"
$TokenFile = Join-Path $DataDir "tunnel.token"
$InstallLog = Join-Path $DataDir "install-service.log"
$RequiredFiles = @(
    (Join-Path $InstallDir "ket-desktop.exe"),
    (Join-Path $InstallDir "ket-tunnel-service.exe"),
    (Join-Path $InstallDir "hysteria.exe"),
    (Join-Path $InstallDir "xray.exe"),
    (Join-Path $InstallDir "tun2proxy.exe"),
    (Join-Path $InstallDir "wintun.dll"),
    (Join-Path $InstallDir "install-tunnel-service.ps1")
)

function Invoke-CheckedProcess {
    param(
        [string]$FilePath,
        [string[]]$Arguments = @()
    )

    $Process = Start-Process `
        -FilePath $FilePath `
        -ArgumentList $Arguments `
        -PassThru `
        -WindowStyle Hidden
    if (-not $Process.WaitForExit(150000)) {
        Write-KetInstallLog
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        Fail "$FilePath did not exit within 150 seconds"
    }
    if ($Process.ExitCode -ne 0) {
        Write-KetInstallLog
        Fail "$FilePath failed with exit code $($Process.ExitCode)"
    }
}

function Write-KetInstallLog {
    if (Test-Path -LiteralPath $InstallLog -PathType Leaf) {
        Write-Host "--- Ket service installer log ---"
        try {
            Get-Content -LiteralPath $InstallLog | ForEach-Object { Write-Host $_ }
        }
        catch {
            Write-Warning "Ket service installer log could not be read: $($_.Exception.Message)"
        }
        Write-Host "--- end Ket service installer log ---"
    }
}

function Invoke-CheckedNative {
    param(
        [string]$FilePath,
        [string[]]$Arguments = @()
    )

    & $FilePath @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Fail "$FilePath failed with exit code $LASTEXITCODE"
    }
}

function Get-KetUninstallEntries {
    $RegistryRoots = @(
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"
    )

    foreach ($RegistryRoot in $RegistryRoots) {
        Get-ItemProperty -Path $RegistryRoot -ErrorAction SilentlyContinue |
            Where-Object {
                $null -ne $_.PSObject.Properties["DisplayName"] -and
                    $_.DisplayName -eq "Ket"
            }
    }
}

function Get-KetUninstaller {
    $Entries = @(Get-KetUninstallEntries)
    if ($Entries.Count -ne 1) {
        Fail "expected one Ket uninstall registration, found $($Entries.Count)"
    }

    $Command = [string]$Entries[0].UninstallString
    if ($Command -notmatch '^"([^"]+)"') {
        Fail "the Ket uninstall command is not a quoted executable path"
    }
    $Uninstaller = $Matches[1]
    if (-not (Test-Path -LiteralPath $Uninstaller -PathType Leaf)) {
        Fail "the registered uninstaller is missing: $Uninstaller"
    }
    return $Uninstaller
}

function Wait-KetServiceRemoved {
    for ($Attempt = 0; $Attempt -lt 40; $Attempt++) {
        if ($null -eq (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue)) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    Fail "the Ket tunnel service remains registered"
}

function Wait-PathRemoved {
    param([string]$Path)

    for ($Attempt = 0; $Attempt -lt 40; $Attempt++) {
        if (-not (Test-Path -LiteralPath $Path)) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    Fail "the installer path remains after uninstall: $Path"
}

function Get-AllowRightsForSid {
    param(
        [System.Security.AccessControl.FileSecurity]$Acl,
        [string]$Sid
    )

    $Rights = 0
    foreach ($Rule in $Acl.Access) {
        if ($Rule.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow) {
            continue
        }
        $RuleSid = $Rule.IdentityReference.Translate(
            [System.Security.Principal.SecurityIdentifier]
        ).Value
        if ($RuleSid -eq $Sid) {
            $Rights = $Rights -bor [int]$Rule.FileSystemRights
        }
    }
    return [System.Security.AccessControl.FileSystemRights]$Rights
}

function Assert-TokenAcl {
    $Acl = Get-Acl -LiteralPath $TokenFile
    if (-not $Acl.AreAccessRulesProtected) {
        Fail "the broker token still inherits access rules"
    }

    $FullControl = [System.Security.AccessControl.FileSystemRights]::FullControl
    $Read = [System.Security.AccessControl.FileSystemRights]::Read
    $Write = [System.Security.AccessControl.FileSystemRights]::Write
    $SystemRights = Get-AllowRightsForSid $Acl "S-1-5-18"
    $AdministratorRights = Get-AllowRightsForSid $Acl "S-1-5-32-544"
    $UserRights = Get-AllowRightsForSid $Acl $CurrentIdentity.User.Value

    if (($SystemRights -band $FullControl) -ne $FullControl) {
        Fail "SYSTEM does not have full control of the broker token"
    }
    if (($AdministratorRights -band $FullControl) -ne $FullControl) {
        Fail "Administrators do not have full control of the broker token"
    }
    if (($UserRights -band $Read) -ne $Read) {
        Fail "$($CurrentIdentity.Name) cannot read the broker token"
    }
    if (($UserRights -band $Write) -ne 0) {
        Fail "$($CurrentIdentity.Name) can write the broker token"
    }
}

function Assert-KetInstallation {
    foreach ($Path in $RequiredFiles) {
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
            Fail "required installed payload is missing: $Path"
        }
    }

    $Service = Get-Service -Name $ServiceName -ErrorAction Stop
    $Service.WaitForStatus("Running", [TimeSpan]::FromSeconds(20))
    $ServiceRecord = Get-CimInstance Win32_Service -Filter "Name='$ServiceName'"
    if ($ServiceRecord.StartMode -ne "Auto") {
        Fail "the Ket tunnel service is not configured for automatic startup"
    }
    $ExpectedServicePath = Join-Path $InstallDir "ket-tunnel-service.exe"
    $RegisteredServicePath = [Environment]::ExpandEnvironmentVariables(
        [string]$ServiceRecord.PathName
    ).Trim().Trim('"')
    $ResolvedExpectedServicePath = (Resolve-Path -LiteralPath $ExpectedServicePath).ProviderPath
    $ResolvedRegisteredServicePath = Resolve-Path `
        -LiteralPath $RegisteredServicePath `
        -ErrorAction SilentlyContinue
    if (
        $null -eq $ResolvedRegisteredServicePath -or
        -not $ResolvedRegisteredServicePath.ProviderPath.Equals(
            $ResolvedExpectedServicePath,
            [System.StringComparison]::OrdinalIgnoreCase
        )
    ) {
        Fail "the Ket tunnel service points at '$($ServiceRecord.PathName)', expected '$ExpectedServicePath'"
    }

    if ((Get-Item -LiteralPath $TokenFile).Length -ne 32) {
        Fail "the broker token must contain 32 bytes"
    }
    Assert-TokenAcl

    Invoke-CheckedNative (Join-Path $InstallDir "hysteria.exe") @("version")
    Invoke-CheckedNative (Join-Path $InstallDir "xray.exe") @("version")
    Invoke-CheckedNative (Join-Path $InstallDir "tun2proxy.exe") @("--version")

    Add-Type -AssemblyName System.Drawing
    $Icon = [System.Drawing.Icon]::ExtractAssociatedIcon(
        (Join-Path $InstallDir "ket-desktop.exe")
    )
    if ($null -eq $Icon) {
        Fail "the desktop executable has no associated application icon"
    }
    $Icon.Dispose()

    [void](Get-KetUninstaller)
}

function Remove-KetTestState {
    $Service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if ($null -ne $Service) {
        if ($Service.Status -ne "Stopped") {
            Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue
        }
        & sc.exe delete $ServiceName | Out-Null
        Wait-KetServiceRemoved
    }

    $Entries = @(Get-KetUninstallEntries)
    if ($Entries.Count -eq 1) {
        $Command = [string]$Entries[0].UninstallString
        if ($Command -match '^"([^"]+)"' -and (Test-Path -LiteralPath $Matches[1])) {
            Invoke-CheckedProcess $Matches[1] @("/S")
        }
    }

    Remove-Item -LiteralPath $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $DataDir -Recurse -Force -ErrorAction SilentlyContinue
}

if ($null -ne (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue)) {
    Fail "the Ket tunnel service is already installed; use a disposable host"
}
if (Test-Path -LiteralPath $InstallDir) {
    Fail "the Ket installation directory already exists; use a disposable host"
}
if (Test-Path -LiteralPath $DataDir) {
    Fail "the Ket data directory already exists; use a disposable host"
}

$CleanupRequired = $true
try {
    Invoke-CheckedProcess $InstallerPath @("/S")
    Assert-KetInstallation
    $OriginalTokenHash = (Get-FileHash -LiteralPath $TokenFile -Algorithm SHA256).Hash

    Invoke-CheckedProcess $InstallerPath @("/S")
    Assert-KetInstallation
    $ReinstalledTokenHash = (Get-FileHash -LiteralPath $TokenFile -Algorithm SHA256).Hash
    if ($ReinstalledTokenHash -ne $OriginalTokenHash) {
        Fail "reinstall replaced the broker token"
    }

    $Uninstaller = Get-KetUninstaller
    Invoke-CheckedProcess $Uninstaller @("/S")
    Wait-KetServiceRemoved
    Wait-PathRemoved $InstallDir
    if ((@(Get-KetUninstallEntries)).Count -ne 0) {
        Fail "the Ket uninstall registration remains after uninstall"
    }
    if (-not (Test-Path -LiteralPath $TokenFile -PathType Leaf)) {
        Fail "uninstall deleted persistent broker state"
    }
    $UninstalledTokenHash = (Get-FileHash -LiteralPath $TokenFile -Algorithm SHA256).Hash
    if ($UninstalledTokenHash -ne $OriginalTokenHash) {
        Fail "uninstall modified the broker token"
    }

    Remove-Item -LiteralPath $DataDir -Recurse -Force
    $CleanupRequired = $false
}
finally {
    if ($CleanupRequired) {
        try {
            Remove-KetTestState
        }
        catch {
            Write-Warning "Ket lifecycle cleanup failed: $($_.Exception.Message)"
        }
    }
}

Write-Host "Ket NSIS install, reinstall, service, and uninstall verification passed."
