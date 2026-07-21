# Server installation

`packaging/install-server.sh` provides a fail-fast installation path for Debian 12+ and Ubuntu 22.04+ on `amd64` or `arm64`. It installs the official Docker packages when needed, obtains a Let's Encrypt certificate, creates independent protocol keys and OpenVPN PKI, enables all six implemented transports, installs certificate renewal, applies rules when UFW is already active, and returns one access code exactly once.

## DNS modes

### Direct VPS

Create an `A` or `AAAA` record for the control hostname that resolves directly to the VPS. The same name is used for raw transports and certificate verification.

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/packaging/install-server.sh | sudo bash -s -- \
  --mode direct --domain ket.example.com --email operator@example.com
```

### Cloudflare proxy

Create two records before installation:

| Record | Cloudflare status | Purpose |
| --- | --- | --- |
| `ket.example.com` | Proxied | Control API, XHTTP/TLS Stealth, WireGuard WebSocket/TLS |
| `direct-ket.example.com` | DNS only | Hysteria2, REALITY, Shadowsocks 2022, OpenVPN/stunnel |

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/packaging/install-server.sh | sudo bash -s -- \
  --mode cloudflare \
  --domain ket.example.com \
  --direct-host direct-ket.example.com \
  --email operator@example.com
```

Set Cloudflare SSL/TLS encryption to **Full (strict)** and leave WebSockets enabled. This mode uses Cloudflare's normal reverse proxy, not a pre-existing Cloudflare Tunnel. A Tunnel deployment can use the same loopback XHTTP and WireGuard origins, but its tunnel credentials and ingress ownership remain an explicit operator step.

Cloudflare must forward `/.well-known/acme-challenge/` over HTTP during initial issuance and renewal. Temporarily disable an account-level HTTP-to-HTTPS redirect if it prevents the standalone Certbot challenge from reaching TCP `80` on the VPS.

Use a release tag in place of `main` for a reproducible production installation after a Ket release is published.

## Ingress

With the default capacity of 32 sessions, allow these stateful rules in the VPS provider's security list. The installer cannot modify a cloud provider firewall.

| Protocol | Port | Purpose |
| --- | --- | --- |
| TCP | `80` | ACME issuance and unattended renewal |
| TCP | `443` | Control, XHTTP/TLS, WireGuard WebSocket/TLS |
| UDP | `443` | Hysteria2 |
| TCP | `8443` | VLESS + REALITY |
| TCP | `9443` | OpenVPN over stunnel |
| TCP+UDP | `20000-20031` | Shadowsocks lease ports |

The Shadowsocks range contains exactly `--max-sessions` ports starting at `20000`. `--plan` prints the final range without requiring root or changing the host:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/packaging/install-server.sh | \
  bash -s -- --domain ket.example.com --email operator@example.com --max-sessions 64 --plan
```

## Installed layout

- Application and secrets: `/opt/ket` by default; `.env` is mode `0600`.
- Persistent grants and sessions: Docker volume `ket_ket-state`.
- Certificate source: `/etc/letsencrypt/live/<control-host>/`.
- Renewal hook: `/etc/letsencrypt/renewal-hooks/deploy/ket-server`.
- Runtime management: `/opt/ket/packaging/server/compose.sh`.

Common checks:

```bash
cd /opt/ket
sudo ./packaging/server/compose.sh ps
sudo ./packaging/server/compose.sh logs --tail=100
curl --fail https://ket.example.com/readyz
sudo certbot renew --dry-run
```

Do not delete or regenerate `.env` during an upgrade. Its server-only derivation keys are required to reconcile active Xray, WireGuard, Shadowsocks, and OpenVPN leases. Back up `.env`, `secrets/`, and the `ket_ket-state` volume before replacing a release.

A deliberate source upgrade keeps those files in place:

```bash
cd /opt/ket
sudo cp .env /root/ket.env.backup
sudo tar -C /opt/ket -czf /root/ket-secrets.backup.tgz secrets
sudo git fetch --tags origin
sudo git checkout <release-tag>
sudo ./packaging/validate-env.sh
sudo ./packaging/server/compose.sh config --quiet
sudo ./packaging/server/compose.sh up --detach --build --remove-orphans
sudo ./packaging/server/compose.sh ps
```

Review release notes before checkout when a version declares a state or configuration migration.

## Security boundary

The edge publishes only HTTPS `443`; control remains host-loopback on `8787`, XHTTP on `8445`, and WireGuard WebSocket on `8446`. `/internal` always returns `404` at the edge. The WireGuard and OpenVPN manager APIs remain on private Compose networks. Hysteria2 and REALITY share port `443` only when one is UDP and the other TCP; the one-command layout moves REALITY to TCP `8443` because the HTTPS edge owns TCP `443`.
