# Privileged tunnel service

Ket keeps the desktop UI and control-plane client unprivileged. A small system service owns the selected Hysteria2 or Xray process together with `tun2proxy`, the TUN device, DNS setup, and route changes. The desktop reaches it only through `127.0.0.1:39731`.

## Trust boundary

- The listener rejects non-loopback peers and accepts at most eight concurrent connections.
- Every connection receives a fresh 256-bit challenge and must prove possession of a 256-bit installation token with HMAC-SHA-256.
- Frames are length-prefixed, JSON encoded, and capped at 128 KiB. Buffers containing transport credentials are zeroized after use.
- The service permits one full-route tunnel at a time. A desktop heartbeat renews a 12-second lease; a crashed client therefore cannot leave routing under an unmanaged process indefinitely.
- Tunnel IDs, HMAC proofs, access codes, and transport credentials use redacted debug implementations. The broker token is never sent over the socket.
- The selected engine receives an ephemeral `0600` configuration under the service runtime directory. Ket removes that file after engine and bridge readiness and disables Hysteria update checks.
- All implemented transports expose only an unauthenticated loopback SOCKS endpoint to the same supervised bridge. The bridge requires virtual DNS, captures IPv4 and IPv6, and bypasses every pre-resolved data-plane or CDN IP.

The token authenticates a local desktop installation; it does not elevate arbitrary requests. The service exposes a fixed command set and validates the server transport description before it starts Hysteria or Xray. Administrators and root remain trusted by the operating-system security model.

## Linux installation

Build `ket-tunnel-service`, obtain the pinned Hysteria, Xray, and `tun2proxy` executables for the target architecture, then run:

```bash
sudo packaging/linux/install-tunnel-service.sh \
  target/release/ket-tunnel-service \
  /path/to/hysteria \
  /path/to/xray \
  /path/to/tun2proxy \
  "$USER"
```

The installer creates the `ket` system group, preserves an existing valid token, grants the desktop user read-only token access, and installs a hardened `systemd` unit. The user must sign in again after the first installation so the new group membership applies.

The service runs as root with a capability bounding set containing `CAP_NET_ADMIN` and `CAP_SYS_ADMIN` for full-route setup. Its filesystem is read-only apart from `/run/ket` and the private `/etc/resolv.conf` mount used by `tun2proxy`; `/dev/net/tun` is the only allowed device.

## Windows installation

Build the service for Windows and run an elevated PowerShell session:

```powershell
.\packaging\windows\install-tunnel-service.ps1 `
  -ServiceBinary .\ket-tunnel-service.exe `
  -HysteriaBinary .\hysteria.exe `
  -XrayBinary .\xray.exe `
  -Tun2ProxyBinary .\tun2proxy.exe `
  -WintunLibrary .\wintun.dll
```

The installer creates the `KetTunnel` automatic Windows service, configures restart-on-failure, places binaries under `%ProgramFiles%\Ket`, and restricts `%ProgramData%\Ket` so the selected desktop identity can read only the installation token. The Rust binary uses the Windows Service Control Manager dispatcher and reports start, running, and stopped states.

The production NSIS package runs this installer automatically. It uses a per-machine installation, stops the broker before upgrades, installs the bundled service and checksum-pinned transport engines, initializes the local authentication token, and removes the Windows service during uninstall.

## Overrides

Development builds accept these process environment variables on both sides of the broker. Production packages should use the defaults.

| Variable | Default | Purpose |
| --- | --- | --- |
| `KET_BROKER_ADDRESS` | `127.0.0.1:39731` | Loopback broker address |
| `KET_BROKER_TOKEN_FILE` | `/etc/ket/tunnel.token` or `%ProgramData%\Ket\tunnel.token` | Installation token |
| `KET_HYSTERIA_BINARY` | `/usr/libexec/ket/hysteria` or `%ProgramFiles%\Ket\hysteria.exe` | Managed Hysteria engine |
| `KET_XRAY_BINARY` | `/usr/libexec/ket/xray` or `%ProgramFiles%\Ket\xray.exe` | Managed Xray engine |
| `KET_TUN2PROXY_BINARY` | `/usr/libexec/ket/tun2proxy` or `%ProgramFiles%\Ket\tun2proxy.exe` | Managed full-route bridge |
| `KET_BROKER_RUNTIME_DIR` | `/run/ket` or `%ProgramData%\Ket\runtime` | Ephemeral engine configuration |

`KET_BROKER_ADDRESS` is rejected unless it resolves to an explicit loopback IP literal.
