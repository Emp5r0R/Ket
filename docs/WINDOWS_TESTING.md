# Windows Package Test Plan

Use this checklist for the unsigned Ket NSIS package on a physical Windows 10 or
Windows 11 x64 machine. Run it from an administrator PowerShell session on a
machine that can be restored or cleaned afterward. Never publish an access code,
`tunnel.token`, server credential, or unredacted Ket runtime file.

## Obtain and verify the package

Download these files from the same GitHub prerelease into an empty directory:

- `Ket_0.1.0_x64-setup.exe`
- `SHA256SUMS`

An unsigned-publisher warning is expected for the current prerelease. A checksum
mismatch is not expected and must stop testing.

```powershell
$Work = Join-Path $HOME "Downloads\Ket-test"
New-Item -ItemType Directory -Force -Path $Work | Out-Null
Set-Location $Work

$Installer = Resolve-Path ".\Ket_0.1.0_x64-setup.exe"
$Expected = (Select-String -Path ".\SHA256SUMS" -Pattern "Ket_0.1.0_x64-setup.exe").Line.Split()[0].ToLowerInvariant()
$Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Installer).Hash.ToLowerInvariant()
if ($Actual -ne $Expected) { throw "Ket installer checksum mismatch" }
```

Record the baseline public IP and DNS state before installation:

```powershell
$BeforeIp = (curl.exe --fail --silent --show-error https://api.ipify.org).Trim()
Resolve-DnsName example.com
Get-NetRoute -AddressFamily IPv4 | Sort-Object RouteMetric | Select-Object -First 20
```

## Install and inspect

```powershell
$Process = Start-Process -FilePath $Installer -Verb RunAs -Wait -PassThru
if ($Process.ExitCode -ne 0) { throw "Ket installer failed with exit code $($Process.ExitCode)" }

$InstallDir = Join-Path $env:ProgramFiles "Ket"
$DataDir = Join-Path $env:ProgramData "Ket"
$Service = Get-Service -Name KetTunnel
$Service | Format-List Name, Status, StartType
Get-CimInstance Win32_Service -Filter "Name='KetTunnel'" | Format-List Name, State, StartMode, PathName
```

The service must be `Running`, use automatic startup, and point to
`C:\Program Files\Ket\ket-tunnel-service.exe`. Confirm the core payload and
broker token exist:

```powershell
@(
    "$InstallDir\ket-desktop.exe",
    "$InstallDir\ket-tunnel-service.exe",
    "$InstallDir\hysteria.exe",
    "$InstallDir\openvpn\openvpn.exe",
    "$InstallDir\sslocal.exe",
    "$InstallDir\stunnel\stunnel.exe",
    "$InstallDir\xray.exe",
    "$InstallDir\wstunnel.exe",
    "$InstallDir\tun2proxy.exe",
    "$InstallDir\wintun.dll",
    "$DataDir\tunnel.token"
) | ForEach-Object {
    if (-not (Test-Path -LiteralPath $_ -PathType Leaf)) { throw "Missing Ket payload: $_" }
}

if ((Get-Item "$DataDir\tunnel.token").Length -ne 32) { throw "Invalid broker token length" }
icacls.exe "$DataDir\tunnel.token"
```

The current user may read the token but must not be able to modify it without
administrator elevation. Save its hash for the reinstall check; do not copy the
token itself.

```powershell
$TokenHash = (Get-FileHash -Algorithm SHA256 "$DataDir\tunnel.token").Hash
```

## Client and tunnel checks

1. Launch Ket and add a current server access code.
2. Confirm the profile survives an app restart and shows the server-provided
   remaining access time.
3. While disconnected, confirm the header says **You are being watched** and the
   viewing icon animates subtly.
4. Connect with **Automatic**. The state must change to **Liberated**, the client
   accent must change to Ket red, and traffic counters must increase.
5. Open multiple HTTPS sites, resolve a new hostname, and download a file large
   enough to observe bidirectional counters.
6. Confirm the public IP changed from `$BeforeIp` to the Ket server egress IP.
7. Disconnect. The original public IP, DNS resolution, and normal routes must
   return without restarting Windows.

Run these checks while connected and again immediately after disconnecting:

```powershell
$ConnectedIp = (curl.exe --fail --silent --show-error https://api.ipify.org).Trim()
Resolve-DnsName example.com
Test-NetConnection 1.1.1.1 -Port 443
Get-NetRoute -AddressFamily IPv4 | Sort-Object RouteMetric | Select-Object -First 20
```

Repeat the connect, traffic, and disconnect sequence for every protocol offered
by the server:

| Protocol | Required result |
| --- | --- |
| VLESS + REALITY | Connects, routes HTTPS and DNS, and restores the direct route on disconnect |
| HTTPS Stealth | Connects through the TLS/XHTTP route and restores cleanly |
| Shadowsocks 2022 | Routes HTTPS and DNS with increasing counters |
| WireGuard TLS | Establishes the WSS carrier and routes full-device traffic |
| OpenVPN over stunnel | Establishes both TLS layers and routes through the OpenVPN TUN |
| Hysteria2 | Connects where UDP is permitted and reports useful failure where UDP is blocked |

For each protocol, also test these transitions:

- Cancel while the client is reconnecting; cancellation must be immediate and
  the connect control must become available again.
- Disable and re-enable Wi-Fi once while connected.
- Switch between Wi-Fi and mobile tethering when available.
- Sleep Windows for at least one minute, resume it, and verify recovery.
- Exit and reopen Ket. The saved profile must remain until its server expiry.
- Allow a short-lived test code to expire; the client must erase the profile and
  refuse a new session.

Any state that says connected while HTTPS or DNS is unusable is a failure. Any
disconnect that leaves Windows without direct internet is also a failure.

## Reinstall and uninstall

Run the same installer again as administrator. The service must return to
`Running`, the app must still launch, and the broker token hash must be unchanged:

```powershell
$Process = Start-Process -FilePath $Installer -Verb RunAs -Wait -PassThru
if ($Process.ExitCode -ne 0) { throw "Ket reinstall failed with exit code $($Process.ExitCode)" }

$ReinstalledTokenHash = (Get-FileHash -Algorithm SHA256 "$DataDir\tunnel.token").Hash
if ($ReinstalledTokenHash -ne $TokenHash) { throw "Reinstall replaced the broker token" }
```

Disconnect Ket, then uninstall it from **Settings > Apps > Installed apps**.
Verify the service and program directory are removed. Normal uninstall retains
the broker token as upgrade-safe application state.

```powershell
if (Get-Service -Name KetTunnel -ErrorAction SilentlyContinue) { throw "KetTunnel service remains" }
if (Test-Path -LiteralPath $InstallDir) { throw "Ket program directory remains" }
if (-not (Test-Path -LiteralPath "$DataDir\tunnel.token")) { throw "Uninstall removed persistent state" }
```

After recording results, remove the retained test state from an administrator
PowerShell session:

```powershell
Remove-Item -LiteralPath $DataDir -Recurse -Force
```

## Evidence and automated lifecycle

Store local evidence under `windows-test-results\`; this directory and downloaded
release artifacts are ignored by Git. Redact server codes, tokens, credentials,
and private hostnames before sharing any file.

```powershell
$Evidence = Join-Path (Get-Location) "windows-test-results"
New-Item -ItemType Directory -Force -Path $Evidence | Out-Null
Get-ComputerInfo | Out-File "$Evidence\computer.txt"
Get-CimInstance Win32_Service -Filter "Name='KetTunnel'" | Format-List * | Out-File "$Evidence\service.txt"
Get-NetAdapter | Format-Table -AutoSize | Out-File "$Evidence\adapters.txt"
Get-NetRoute | Sort-Object InterfaceIndex, DestinationPrefix | Out-File "$Evidence\routes.txt"
Get-ChildItem -LiteralPath $DataDir -Recurse | Select-Object FullName, Length, LastWriteTime | Out-File "$Evidence\ket-files.txt"
```

On a disposable Windows host with this repository checked out, the destructive
installer lifecycle can be run from an elevated PowerShell session:

```powershell
$env:KET_PACKAGE_TEST_ALLOW_HOST_MUTATION = "1"
$Installer = (Resolve-Path ".\Ket_0.1.0_x64-setup.exe").Path
.\packaging\verify-windows-nsis.ps1 -Installer $Installer
```

That script requires a clean host, performs silent install and reinstall,
validates service registration and token ACLs, uninstalls Ket, and removes its
test state. Do not run it on a machine with a Ket profile that must be retained.
