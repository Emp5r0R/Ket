# Server installation

`packaging/install-server.sh` provides a fail-fast installation path for a fresh Debian 12+ or Ubuntu 22.04+ VPS on `amd64` or `arm64`. It installs Docker Engine and Compose from Docker's official apt repository when needed, obtains a Let's Encrypt certificate, detects node location from the public IP, creates independent protocol keys and OpenVPN PKI, enables all six implemented transports, installs certificate renewal, applies rules when UFW is already active, and returns one access code exactly once. The default installed source is the stable `v1.0.1` tag; `--ref` deliberately selects another release or branch.

The first code is valid for 43,200 minutes (30 days) by default. Set `--first-code-valid-minutes` to any value from `1` through `525600`; the installer prints the server-derived absolute expiry with the code.

## Fresh-server prerequisites

- A public IPv4 address without carrier-grade NAT.
- Debian 12+ or Ubuntu 22.04+ on `amd64` or `arm64`, using systemd.
- Root access, working apt repositories, and outbound HTTPS.
- Recommended minimum: 2 CPU cores, 2 GiB RAM, and 10 GiB free disk for native multi-stage image builds.
- `/opt/ket` absent and public listener ports unused. The installer refuses to replace an existing installation.
- Docker Compose v2. When Docker is absent, Ket installs the official packages. When an existing Docker installation lacks Compose v2 or conflicts with Docker's official packages, the installer stops instead of removing software or risking existing containers.

The copy-paste commands require `curl`. On a completely fresh image, install it first:

```bash
sudo apt-get update && sudo apt-get install -y ca-certificates curl
```

## DNS modes

### No domain

Automatic mode detects the VPS public IPv4 address and derives a hostname such as `ket-161-118-195-126.sslip.io`. [sslip.io](https://sslip.io/) resolves hostnames containing an IP address to that address, so no DNS account or manual record is required. Ket still obtains a normal Let's Encrypt certificate and all clients retain certificate verification.

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/v1.0.1/packaging/install-server.sh | sudo bash -s -- \
  --mode auto --email operator@example.com
```

Automatic mode depends on a third-party DNS service and can be blocked by DNS rebinding filters. Prefer a domain you control for a long-lived or multi-user service. If public-IP detection is wrong because of unusual routing, pass the verified address with `--public-ip 161.118.195.126`.

### Direct VPS

Create an `A` record for the control hostname that resolves directly to the VPS public IPv4 address. The same name is used for raw transports and certificate verification.

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/v1.0.1/packaging/install-server.sh | sudo bash -s -- \
  --mode direct --domain ket.example.com --email operator@example.com \
  --first-code-valid-minutes 43200
```

### Cloudflare proxy

Create two records before installation:

| Record | Cloudflare status | Purpose |
| --- | --- | --- |
| `ket.example.com` | Proxied | Control API, XHTTP/TLS Stealth, WireGuard WebSocket/TLS |
| `direct-ket.example.com` | DNS only | Hysteria2, REALITY, Shadowsocks 2022, OpenVPN/stunnel |

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/v1.0.1/packaging/install-server.sh | sudo bash -s -- \
  --mode cloudflare \
  --domain ket.example.com \
  --direct-host direct-ket.example.com \
  --email operator@example.com \
  --first-code-valid-minutes 43200
```

Set Cloudflare SSL/TLS encryption to **Full (strict)** and leave WebSockets enabled. This mode uses Cloudflare's normal reverse proxy, not a pre-existing Cloudflare Tunnel. A Tunnel deployment can use the same loopback XHTTP and WireGuard origins, but its tunnel credentials and ingress ownership remain an explicit operator step.

Cloudflare must forward `/.well-known/acme-challenge/` over HTTP during initial issuance and renewal. Temporarily disable an account-level HTTP-to-HTTPS redirect if it prevents the standalone Certbot challenge from reaching TCP `80` on the VPS.

The streamed installer defaults the server checkout to `v1.0.1`. Pass `--ref <release-tag>` to select another published server version explicitly.

## Node location

The normal install commands require no location flags. Ket tries two HTTPS public-IP location providers and validates the returned country, city, latitude, and longitude before writing `.env`. Installation stops instead of publishing an `Unknown` or `0,0` node when both providers fail.

To override provider data, supply the complete location as one consistent set. Partial overrides are rejected:

```bash
--country-code SG --country-name Singapore --city Singapore \
--latitude 1.290270 --longitude 103.851959
```

## Ingress

With the default capacity of 32 sessions, allow these stateful rules in the VPS provider's security list **before** installation. Use source CIDR `0.0.0.0/0`. The installer cannot modify a cloud provider firewall.

| Protocol | Port | Purpose |
| --- | --- | --- |
| TCP | `80` | ACME issuance and unattended renewal |
| TCP | `443` | Control, VLESS + REALITY, XHTTP/TLS, WireGuard WebSocket/TLS |
| UDP | `443` | Hysteria2 |
| TCP | `9443` | OpenVPN over stunnel |
| TCP+UDP | `20000-20031` | Shadowsocks lease ports |

The Shadowsocks range contains exactly `--max-sessions` ports starting at `20000`. `--plan` prints the final range without requiring root or changing the host:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/v1.0.1/packaging/install-server.sh | \
  bash -s -- --domain ket.example.com --email operator@example.com --max-sessions 64 --plan
```

Keep the existing SSH rule that you use to administer the VPS. Do **not** open control TCP `8787`, XHTTP origin TCP `8445`, WireGuard WebSocket origin TCP `8446`, or any container-private manager port.

Provider UI names differ, but each rule needs an inbound direction, a stateful connection type, source CIDR `0.0.0.0/0`, the listed IP protocol, source port `All`, and the listed destination port or range:

- **Oracle Cloud:** VCN -> Security Lists or Network Security Group -> Add Ingress Rules.
- **AWS:** EC2 -> Security Groups -> Inbound rules -> Edit inbound rules; use Custom TCP or Custom UDP.
- **Google Cloud:** VPC network -> Firewall -> Create firewall rule; use ingress and the VPS's target tag/service account.
- **Other VPS providers:** look for Firewall, Security Group, Network ACL, or Ingress Rules.

When UFW is already active, the installer adds the same Ket allowances before Certbot runs. It does not enable UFW or change SSH policy. Provider ingress remains the primary boundary: [Docker documents that published container ports can bypass UFW rules](https://docs.docker.com/engine/install/ubuntu/#firewall-limitations), so do not rely on UFW alone.

TCP `80` must remain reachable for unattended HTTP-01 renewals. [Let's Encrypt's HTTP-01 validation always connects to port `80`](https://letsencrypt.org/docs/challenge-types/#http-01-challenge); changing the application's HTTPS port does not remove that requirement.

## Preflight and installation flow

`--plan` validates arguments and prints hostname, DNS, access lifetime, and exact ingress without installing packages. In automatic mode, add `--public-ip` for an offline deterministic plan; otherwise the plan performs public-IP detection.

During a real installation Ket:

1. Validates the OS version, architecture, requested capacity, location override, public IPv4, and DNS.
2. Rejects occupied TCP/UDP listeners before changing Docker or Ket state.
3. Adds rules to an already-active UFW firewall before requesting the certificate.
4. Requests one certificate for the control and raw hostnames, then installs or validates Docker Compose v2.
5. Builds in a temporary sibling directory. DNS, certificate, clone, or build failures do not create a partial `/opt/ket`; the temporary source checkout is removed automatically.
6. Generates independent credentials and PKI, validates `.env` and rendered Compose, moves the validated tree to `/opt/ket`, and starts the stack.
7. Waits for certificate-verified edge readiness, creates the first access grant, and prints client setup instructions.

If Certbot fails, verify TCP `80`, DNS propagation, NAT, provider ingress, and any Cloudflare redirect rule, then rerun the same command. If the failure happens after `/opt/ket` is created, retain that directory for logs; do not blindly rerun or regenerate `.env`.

## Client handoff

After the readiness check, the installer prints the first invite (`ket:invite:...`) as text and as a terminal QR code, plus the same grant as a server URL and access code for older apps. Store it immediately; only the code's protected verifier remains on the server. The installer also generates the node signing key (`KET_NODE_SIGNING_KEY` in `.env`) that signs invites; never rotate or share it. Download the matching package from the [stable v1.0.1 release](https://github.com/Emp5r0R/Ket/releases/tag/v1.0.1):

- Linux Debian/Ubuntu: `Ket_1.0.1_amd64.deb`
- Windows x64: `Ket_1.0.1_x64-setup.exe`
- Android: `Ket_1.0.1_android-universal.apk`

Verify `SHA256SUMS.asc` and the package signature as described in [release signing](RELEASE_SIGNING.md), install the client, and paste or scan the invite (Android can scan the terminal QR code or a screenshot of it). The URL and access code remain available under **Advanced**. The client keeps the profile, including the node key it pins, until it expires or the user chooses Forget.

Create more invites at any time on the server:

```bash
cd /opt/ket
sudo ./packaging/server/create-invite.sh --label "Family" --devices 3 --days 30
```

`--devices` limits simultaneous connections for that invite and `--days` (or `--minutes`) sets its lifetime. Each run creates a new, independently revocable grant.

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
curl --fail http://127.0.0.1:8787/readyz   # on the server
curl --fail https://ket.example.com/         # neutral public page
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

Servers installed before invites existed have no node signing key. Add one once, then restart; existing access codes keep working and new grants also return an invite:

```bash
cd /opt/ket
grep -q '^KET_NODE_SIGNING_KEY=' .env || \
  printf 'KET_NODE_SIGNING_KEY=%s\n' "$(openssl rand 32 | base64 | tr -d '\n=' | tr '+/' '-_')" | sudo tee -a .env >/dev/null
sudo ./packaging/validate-env.sh
sudo ./packaging/server/compose.sh up --detach --remove-orphans
```

## Security boundary

The edge is the only public TCP `443` listener. It prereads SNI and passes the configured REALITY disguise name to Xray without TLS termination; all other names continue to the HTTPS edge. The HTTPS edge forwards only the client session routes and the exact deployment-secret XHTTP and WireGuard paths; every other request, including `/healthz`, `/readyz`, `/metrics`, `/v1/node/status`, `/v1/admin`, and `/internal`, receives one neutral `404` page, and unauthenticated session requests receive the same page (see [CONTROL_API.md](CONTROL_API.md#public-exposure)). Control remains host-loopback on `8787`, XHTTP on `8445`, and WireGuard WebSocket on `8446`; the edge health check uses a container-loopback listener. New installs generate transport paths without a product-identifying prefix. The WireGuard and OpenVPN manager APIs remain on private Compose networks. Hysteria2 independently shares public port `443` over UDP.
