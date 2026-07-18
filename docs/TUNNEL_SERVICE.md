# Privileged tunnel service

Ket keeps the desktop UI and control-plane client unprivileged. A small system service owns the Hysteria2 process, TUN device, and route changes. The desktop reaches it only through `127.0.0.1:39731`.

## Trust boundary

- The listener rejects non-loopback peers and accepts at most eight concurrent connections.
- Every connection receives a fresh 256-bit challenge and must prove possession of a 256-bit installation token with HMAC-SHA-256.
- Frames are length-prefixed, JSON encoded, and capped at 128 KiB. Buffers containing transport credentials are zeroized after use.
- The service permits one full-route tunnel at a time. A desktop heartbeat renews a 12-second lease; a crashed client therefore cannot leave routing under an unmanaged process indefinitely.
- Tunnel IDs, HMAC proofs, access codes, and transport credentials use redacted debug implementations. The broker token is never sent over the socket.
- The engine receives an ephemeral `0600` configuration under the service runtime directory. Ket removes that file after engine startup and disables Hysteria update checks.

The token authenticates a local desktop installation; it does not elevate arbitrary requests. The service exposes a fixed command set and validates the server transport description before it starts Hysteria. Administrators and root remain trusted by the operating-system security model.

## Linux installation

Build `ket-tunnel-service`, obtain the pinned Hysteria executable for the target architecture, then run:

```bash
sudo packaging/linux/install-tunnel-service.sh \
  target/release/ket-tunnel-service \
  /path/to/hysteria \
  "$USER"
```

The installer creates the `ket` system group, preserves an existing valid token, grants the desktop user read-only token access, and installs a hardened `systemd` unit. The user must sign in again after the first installation so the new group membership applies.

The service runs as root with a capability bounding set containing only `CAP_NET_ADMIN`. Its filesystem is read-only apart from `/run/ket`, and `/dev/net/tun` is the only allowed device.

## Windows installation

Build the service for Windows and run an elevated PowerShell session:

```powershell
.\packaging\windows\install-tunnel-service.ps1 `
  -ServiceBinary .\ket-tunnel-service.exe `
  -HysteriaBinary .\hysteria.exe
```

The installer creates the `KetTunnel` automatic Windows service, configures restart-on-failure, places binaries under `%ProgramFiles%\Ket`, and restricts `%ProgramData%\Ket` so the selected desktop identity can read only the installation token. The Rust binary uses the Windows Service Control Manager dispatcher and reports start, running, and stopped states.

## Overrides

Development builds accept these process environment variables on both sides of the broker. Production packages should use the defaults.

| Variable | Default | Purpose |
| --- | --- | --- |
| `KET_BROKER_ADDRESS` | `127.0.0.1:39731` | Loopback broker address |
| `KET_BROKER_TOKEN_FILE` | `/etc/ket/tunnel.token` or `%ProgramData%\Ket\tunnel.token` | Installation token |
| `KET_HYSTERIA_BINARY` | `/usr/libexec/ket/hysteria` or `%ProgramFiles%\Ket\hysteria.exe` | Managed engine |
| `KET_BROKER_RUNTIME_DIR` | `/run/ket` or `%ProgramData%\Ket\runtime` | Ephemeral engine configuration |

`KET_BROKER_ADDRESS` is rejected unless it resolves to an explicit loopback IP literal.
