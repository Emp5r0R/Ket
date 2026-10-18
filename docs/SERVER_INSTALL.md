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

When it finishes, the installer prints the first invite as text and as a terminal QR code. Paste or scan it in a Ket app. From then on, the `ket` command manages the server ([below](#managing-the-server)); the same bootstrap also takes `update` and `uninstall`.

## What you need

- A VPS with a public IPv4 address (not behind carrier-grade NAT), `amd64` or `arm64`, running systemd.
- Root access and outbound HTTPS.
- Recommended: 2 CPU cores, 2 GiB RAM, 10 GiB free disk. Releases with a published server image need far less to install; without one, the image is built on the server.
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
  --auto-update POLICY   Releases the daily check applies on its own:
                         off, critical, security (default), or all
  --plan                 Show what would be set up and change nothing
```

Pass options after the domain through the bootstrap, for example `... | sudo bash -s -- vpn.example.com --max-sessions 64`. `--plan` needs no root and prints the address, the exact port list, and the first invite's limits.

The map location is detected from the public IP (two providers, validated). Installation stops rather than publishing an unknown location; then pass all five location options.

## What the installer does

1. Checks the system (systemd, CPU architecture), installs missing base tools, and detects the public IPv4 address and the mode.
2. Checks that the direct address points at this server and that the ports are free; adds UFW or firewalld rules when active.
3. Installs or verifies Docker with Compose v2.
4. Gets one Let's Encrypt certificate for every name using the pinned certbot container (no certbot package on the host).
5. Generates independent secrets, X25519 keys (in the pinned Xray container), the node signing key, and the OpenVPN PKI; validates `.env` and the Compose configuration; gets the server image (the one published with the release when available, otherwise built on the server). All of this happens in a temporary directory, so a failure leaves no partial `/opt/ket`.
6. Moves the validated tree to `/opt/ket`, starts the stack, and installs the `ket` command, the `ket-certificates.timer` renewal timer, and the `ket-update.timer` daily update check.
7. Waits until the control plane is ready and the public edge serves its certificate, then prints the first invite.

If the certificate request fails, check that TCP 80 reaches the server, that DNS has propagated, and any Cloudflare redirect, then rerun. If a failure happens after `/opt/ket` exists, keep that directory for its logs instead of rerunning.

## Managing the server

The installer adds the `ket` command:

| Command | What it does |
| --- | --- |
| `sudo ket invite` | Create an invite. With no options it asks who it is for, how many devices may connect at once, for how many days, which protocols it may use, and how much data its devices may use. Options: `--label`, `--devices`, `--days` or `--minutes`, `--protocols`, `--data 50GB/month`, `--no-qr` |
| `sudo ket invites` | List invites: status, devices in use, data used, when they end, and their protocols |
| `sudo ket invites show ID` | One invite and the devices connected through it right now |
| `sudo ket invites pause ID` | Stop an invite for now and disconnect its devices; `resume ID` turns it back on |
| `sudo ket invites edit ID` | Change an invite. With no options it asks; options: `--label`, `--devices`, `--extend 7d`, `--shorten 3d`, `--ends-in 30d`, `--protocols`, `--data 10GB/week` (or `none`), `--reset-data` |
| `sudo ket invites protocols` | The protocol names this server offers, for `--protocols` |
| `sudo ket revoke ID` | Revoke an invite for good and disconnect its devices |
| `sudo ket status` | Version, services, connected devices, certificate expiry, last update check |
| `sudo ket doctor` | Check Docker, the configuration, every service, DNS, listening ports, the certificate, and the timers, and say how to fix each problem |
| `sudo ket logs [SERVICE] [--follow]` | Recent logs, for example `sudo ket logs control-plane` |
| `sudo ket restart` | Apply changes made to `.env` and restart |
| `sudo ket update` | Update to the latest release (`--check` only reports, `--to vX.Y.Z` picks a release) |
| `sudo ket rollback` | Go back to the release before the last update (and forward again) |
| `sudo ket backup` | Save `.env`, `secrets/`, and the grants and sessions to `/var/backups/ket` |
| `sudo ket shaping` | Fair sharing of a full connection: `set 950mbit` (a little below your plan), `off`, or nothing to see whether it is on ([FAIRNESS.md](FAIRNESS.md)) |
| `sudo ket keys` | Show the node fingerprint that apps check invites against; `rotate-admin-token`, or `rotate-node-key` after a leak (that invalidates every invite) |
| `sudo ket uninstall` | Remove Ket (see below) |

Invite changes take effect at once. Apps a change disconnects connect again by themselves with the new settings; while an invite is paused, the apps keep it and wait. Devices see a new end date the next time they reach the server. The invite text itself keeps its original date, so after that date new people need a new invite.

`--protocols` takes `all` or names from `sudo ket invites protocols`, for example `--protocols "stealth reality"`. `--data` takes a size and a period, such as `50GB/month`, `2GB/day`, or `10GB/week`, shared by all of the invite's devices; [FAIRNESS.md](FAIRNESS.md) explains how it is counted. Ket v1.1.0 and older ignore pauses and protocol limits, so after `sudo ket rollback` to such a release they are off until you update again. The same holds for data allowances and fair sharing with a release from before them: allowances stop counting, and fair sharing is not turned on again after a reboot.

From outside, `https://<your address>/` shows a neutral placeholder page; Ket's API is deliberately invisible. Certificates are checked twice a day by `ket-certificates.timer`.

Clients: install the Ket app from the [latest release](https://github.com/Emp5r0R/Ket/releases/latest), verify it as described in [release signing](RELEASE_SIGNING.md), then paste or scan the invite. **Advanced** in the app still accepts the server URL and access code that `ket invite` prints for older apps.

## Installed layout

- `/opt/ket`: application, `.env` (mode `0600`), `secrets/`, and `.ket-release` (the installed release).
- `/opt/ket.previous`: the release before the last update, for `ket rollback`.
- Docker volume `ket_ket-state`: grants and sessions.
- `/etc/letsencrypt`: the certificate; Ket keeps its own copies under `secrets/`.
- `/etc/systemd/system/ket-certificates.{service,timer}` and `ket-update.{service,timer}`.
- `/usr/local/bin/ket`: the `ket` command (a link into `/opt/ket`).
- `/var/backups/ket`: backups (the newest 10 are kept); `/var/lib/ket`: the last update check.

`KET_NODE_SIGNING_KEY` in `.env` signs invites. Do not rotate or share it: a new key invalidates every outstanding invite and every app profile pinned to the old one.

## Updates

`ket-update.timer` checks for a new release once a day. What it does depends on `KET_AUTO_UPDATE` in `.env` and on the release's rating ([UPDATES.md](UPDATES.md)):

| `KET_AUTO_UPDATE` | Applied on its own |
| --- | --- |
| `security` (default) | Security and critical releases |
| `critical` | Critical releases only |
| `all` | Every release |
| `off` | Nothing; `ket status` shows what is waiting |

Everything else waits for `sudo ket update`. An update:

1. downloads the release and runs **that release's** updater, so the newer code decides how it is installed;
2. builds the new tree next to the old one with your `.env`, `secrets/`, and any `compose.override.yaml`, adds settings the release needs (for example a node signing key on servers from before invites), validates it, and gets its image, while the old release keeps serving;
3. backs up `.env`, `secrets/`, and the state volume to `/var/backups/ket`;
4. swaps the trees, starts the new release, and waits until it is healthy;
5. if it does not become healthy, puts the old release back automatically and keeps the failed one, with its logs, in `/opt/ket.failed-<time>`.

`sudo ket rollback` switches back to the previous release later. A release that changed the stored state may not be readable by the older release; restore the backup taken before the update in that case.

Servers installed with v1.0.0 or v1.0.1 have no `ket` command yet. Update them once with the bootstrap; afterwards they have `ket` and the daily check, and keep renewing their certificate through the host certbot:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- update
```

## Uninstalling

```bash
sudo ket uninstall
```

It asks you to type `uninstall`, writes a final backup of your keys, settings, and grants to `/var/backups/ket` (so a reinstall can reuse them), then removes the containers, the state volume, the timers, the `ket` command, and `/opt/ket`. `--purge` also deletes the backups, the Let's Encrypt certificate, and the container images. Firewall rules the installer added are left in place, as other services may use those ports. Servers without the `ket` command can use `curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- uninstall`.

## Security boundary

The edge is the only public TCP 443 listener. It reads the TLS server name and passes the configured REALITY disguise name to Xray without terminating TLS; everything else reaches the HTTPS edge. That edge forwards only the client session routes and the exact deployment-secret XHTTP and WireGuard paths. Every other request, including `/healthz`, `/readyz`, `/metrics`, `/v1/node/status`, `/v1/admin`, and `/internal`, gets one neutral 404 page, and so do unauthenticated session requests (see [CONTROL_API.md](CONTROL_API.md#public-exposure)). The control API stays on host loopback 8787, the XHTTP origin on 8445, and the WireGuard WebSocket origin on 8446. The WireGuard and OpenVPN manager APIs stay on private Compose networks. Hysteria2 shares public port 443 over UDP.
