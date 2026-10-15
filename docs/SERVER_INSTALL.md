# Server installation

One command sets up a complete Ket server on a fresh Linux VPS:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash
```

With your own domain (pointing straight at the server, or proxied by Cloudflare; Ket detects which):

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- vpn.example.com
```

`install.sh` looks up the latest Ket release, downloads that release, and runs its own installer (`packaging/install-server.sh`), so the installer always matches the code it installs. Set `KET_REF=<tag>` to install a specific release. Releases published before one-command setup (v1.0.1 and earlier) are recognised and the bootstrap prints their own install command instead; for those, see this guide at that tag.

When it finishes, the installer prints the first invite as text and as a terminal QR code. Paste or scan it in a Ket app.

## What you need

- A VPS with a public IPv4 address (not behind carrier-grade NAT), `amd64` or `arm64`, running systemd.
- Root access and outbound HTTPS.
- Recommended: 2 CPU cores, 2 GiB RAM, 10 GiB free disk (images are built on the server).
- `curl` and `tar`, which almost every image has. Everything else is installed or run in containers.

### Supported systems

Ket runs entirely in Docker, so the host only needs Docker with Compose v2.

- **Docker already installed** (with the Compose v2 plugin, 2.17 or newer): any systemd Linux works.
- **Docker installed for you** from Docker's official repositories: Debian, Ubuntu, and their derivatives (Linux Mint, Raspberry Pi OS, Pop!_OS, ...), and RHEL-compatible systems (RHEL, Rocky, AlmaLinux, CentOS Stream, Oracle Linux) and Fedora.
- **Other systems** (Amazon Linux, openSUSE, Arch, ...): install Docker Engine and its Compose plugin first, then run the command.

Missing base tools (`curl`, `tar`, `openssl`, `jq`) are installed with the system package manager. `qrencode` is installed when available for the terminal QR code; without it the invite is printed as text only.

WireGuard TLS needs kernel WireGuard support (built into Linux 5.6+, backported by many distributions). On older kernels the installer turns that one transport off and everything else works.

The installer refuses to touch an existing installation directory or ports that are already in use, and it never removes conflicting packages: if an existing Docker setup conflicts with Docker's official packages, it stops and tells you what to remove.

## Open these ports first

Your cloud provider's firewall (security list, security group, ...) must allow these inbound rules, source `0.0.0.0/0`, before you install. The installer cannot change a cloud provider's firewall.

| Protocol | Port | Used for |
| --- | --- | --- |
| TCP | 80 | Getting and renewing the TLS certificate |
| TCP | 443 | Control, VLESS + REALITY, HTTPS Stealth, WireGuard TLS |
| UDP | 443 | Hysteria2 |
| TCP | 9443 | OpenVPN over TLS |
| TCP + UDP | 20000-20031 | Shadowsocks 2022 (one port per session) |

The Shadowsocks range holds exactly `--max-sessions` ports from 20000 (32 by default). Keep your SSH rule. Never open TCP 8787, 8445, or 8446; they stay private.

- **Oracle Cloud:** VCN -> Security Lists (or a Network Security Group) -> Add Ingress Rules.
- **AWS:** EC2 -> Security Groups -> Inbound rules -> Edit inbound rules (Custom TCP / Custom UDP).
- **Google Cloud:** VPC network -> Firewall -> Create firewall rule (ingress, target the VPS).
- **Others:** look for Firewall, Security Group, Network ACL, or Ingress Rules.

If UFW or firewalld is active on the host, the installer adds the same rules there. Docker-published ports can bypass host firewalls, so the provider firewall remains the real boundary.

## Addresses: automatic, direct, or Cloudflare

The installer chooses the mode for you:

| You run | Ket uses | DNS you set up |
| --- | --- | --- |
| no domain | `ket-<ip>.sslip.io` | none |
| a domain that points at this server | that domain for everything | an `A` record to the server IP |
| a domain proxied by Cloudflare | the domain for HTTPS-shaped transports, and `ket-<ip>.sslip.io` for raw UDP/TLS transports | a proxied `A` record |

- **Automatic** relies on [sslip.io](https://sslip.io/), a public wildcard DNS service. It needs no account, but some restrictive resolvers block such names; a domain you control is better for long-lived use.
- **Cloudflare:** set SSL/TLS to **Full (strict)** and keep WebSockets on. Cloudflare's proxy cannot carry Hysteria2, REALITY, Shadowsocks, or OpenVPN, so those use a DNS-only name. Ket picks the automatic sslip.io name unless you give `--direct-host direct.example.com` (a DNS-only record to the same server). Cloudflare must let `/.well-known/acme-challenge/` through over plain HTTP; turn off an "Always Use HTTPS" redirect if certificate issuance fails.
- A domain that resolves somewhere else, or not at all yet, stops the installer with an explanation. `--mode auto|direct|cloudflare` forces a mode.

## Options

```text
install-server.sh [DOMAIN] [options]
  --email EMAIL          Let's Encrypt expiry notices (optional)
  --direct-host HOST     DNS-only name for raw transports behind Cloudflare
  --mode MODE            Force auto, direct, or cloudflare
  --public-ip IPv4       Override public IPv4 detection
  --max-sessions N       Concurrent sessions, 1-512 (default 32)
  --first-code-valid-minutes N
                         Lifetime of the first invite (default 43200 = 30 days)
  --country-code XX --country-name NAME --city NAME --latitude N --longitude N
                         Map location, all five together (default: detected)
  --install-dir PATH     Installation directory (default /opt/ket)
  --plan                 Show what would be set up and change nothing
```

Pass options after the domain through the bootstrap, for example `... | sudo bash -s -- vpn.example.com --max-sessions 64`. `--plan` needs no root and prints the address, the exact port list, and the first invite's limits.

The map location is detected from the public IP (two providers, validated). Installation stops rather than publishing an unknown location; then pass all five location options.

## What the installer does

1. Checks the system (systemd, CPU architecture), installs missing base tools, and detects the public IPv4 address and the mode.
2. Checks that the direct address points at this server and that the ports are free; adds UFW or firewalld rules when active.
3. Installs or verifies Docker with Compose v2.
4. Gets one Let's Encrypt certificate for every name using the pinned certbot container (no certbot package on the host).
5. Generates independent secrets, X25519 keys (in the pinned Xray container), the node signing key, and the OpenVPN PKI; validates `.env` and the Compose configuration; builds the image. All of this happens in a temporary directory, so a failure leaves no partial `/opt/ket`.
6. Moves the validated tree to `/opt/ket`, starts the stack, and installs the `ket-certificates.timer` renewal timer.
7. Waits until the control plane is ready and the public edge serves its certificate, then prints the first invite.

If the certificate request fails, check that TCP 80 reaches the server, that DNS has propagated, and any Cloudflare redirect, then rerun. If a failure happens after `/opt/ket` exists, keep that directory for its logs instead of rerunning.

## After installation

- More invites: `cd /opt/ket && sudo ./packaging/server/create-invite.sh --label "Family" --devices 3 --days 30`. `--devices` limits simultaneous connections, `--days` (or `--minutes`) sets the lifetime, and each run creates a separate grant that can be revoked on its own.
- Status and logs: `sudo ./packaging/server/compose.sh ps` and `sudo ./packaging/server/compose.sh logs --tail=100` in `/opt/ket`.
- Health on the server: `curl --fail http://127.0.0.1:8787/readyz`. From outside, `https://<your address>/` shows a neutral placeholder page; Ket's API is deliberately invisible.
- Certificates are checked twice a day by `ket-certificates.timer` (`systemctl list-timers ket-certificates.timer`); run a check now with `sudo systemctl start ket-certificates.service`.

Clients: install the Ket app from the [latest release](https://github.com/Emp5r0R/Ket/releases/latest), verify it as described in [release signing](RELEASE_SIGNING.md), then paste or scan the invite. **Advanced** in the app still accepts the server URL and access code that the installer prints for older apps.

## Installed layout

- `/opt/ket`: application, `.env` (mode `0600`), and `secrets/`.
- Docker volume `ket_ket-state`: grants and sessions.
- `/etc/letsencrypt`: the certificate; Ket keeps its own copies under `secrets/`.
- `/etc/systemd/system/ket-certificates.{service,timer}`: renewal.
- `packaging/server/compose.sh`: runs Docker Compose with exactly the transports `.env` enables.

`KET_NODE_SIGNING_KEY` in `.env` signs invites. Never rotate or share it: a new key invalidates every outstanding invite and every app profile pinned to the old one.

## Upgrades

Never delete or regenerate `.env` or `secrets/`; their keys reconcile active sessions. Back up `.env`, `secrets/`, and the `ket_ket-state` volume first, and read the release notes for migrations.

Installations made by the one-command installer are plain release trees. Unpack the new release over them (your `.env` and `secrets/` are not part of a release and stay in place):

```bash
ref=vX.Y.Z
cd /opt/ket
sudo cp .env /root/ket.env.backup
sudo tar -czf /root/ket-secrets.backup.tgz secrets
curl -fsSL "https://codeload.github.com/Emp5r0R/Ket/tar.gz/$ref" | sudo tar -xzf - --strip-components=1 -C /opt/ket
sudo ./packaging/validate-env.sh
sudo ./packaging/server/compose.sh up --detach --build --remove-orphans
```

Servers installed by the v1.0.1 installer are git checkouts and use the host certbot renewal hook. Upgrade them with `sudo git fetch --tags origin && sudo git checkout <tag>` followed by the same validate and `compose.sh up` steps; their renewal keeps working.

Servers installed before invites have no node signing key. Add one once; existing access codes keep working and new grants also get invites:

```bash
cd /opt/ket
grep -q '^KET_NODE_SIGNING_KEY=' .env || \
  printf 'KET_NODE_SIGNING_KEY=%s\n' "$(openssl rand 32 | base64 | tr -d '\n=' | tr '+/' '-_')" | sudo tee -a .env >/dev/null
sudo ./packaging/validate-env.sh
sudo ./packaging/server/compose.sh up --detach --remove-orphans
```

## Security boundary

The edge is the only public TCP 443 listener. It reads the TLS server name and passes the configured REALITY disguise name to Xray without terminating TLS; everything else reaches the HTTPS edge. That edge forwards only the client session routes and the exact deployment-secret XHTTP and WireGuard paths. Every other request, including `/healthz`, `/readyz`, `/metrics`, `/v1/node/status`, `/v1/admin`, and `/internal`, gets one neutral 404 page, and so do unauthenticated session requests (see [CONTROL_API.md](CONTROL_API.md#public-exposure)). The control API stays on host loopback 8787, the XHTTP origin on 8445, and the WireGuard WebSocket origin on 8446. The WireGuard and OpenVPN manager APIs stay on private Compose networks. Hysteria2 shares public port 443 over UDP.
