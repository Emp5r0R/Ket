# WireGuard TLS deployment

Ket carries WireGuard UDP through the maintained [wstunnel](https://github.com/erebe/wstunnel) WebSocket client and a certificate-verified public `wss://` endpoint. The server uses the Linux WireGuard implementation; desktop and Android use Xray's userspace WireGuard client, then feed the existing full-route SOCKS/TUN bridge. Ket does not implement WireGuard, WebSocket, TLS, or their cryptography.

This is not Proton Stealth and is not wire-compatible with Proton VPN. Proton documents Stealth as WireGuard tunneled over obfuscated TLS, but its production server contract is not a self-hostable interoperability target. Ket uses the same broad censorship-resistance idea with an independently deployable WebSocket/TLS carrier and labels the transport `WireGuard TLS`.

## Trust and lifecycle

1. Each active lease receives a deterministic WireGuard private key, preshared key, and address in `10.66.0.0/16`. Only the resource slot is persisted.
2. The control plane provisions the peer through an authenticated private HTTP manager before it returns the session manifest.
3. Release, expiry, and grant revocation remove the peer. Startup reconciliation replaces stale kernel peer state with the persisted active lease set.
4. The privileged agent owns only the WireGuard interface, forwarding policy, and NAT. The unprivileged wstunnel container cannot call the manager or access control-plane state.
5. Clients resolve the public hostname before changing routes, connect wstunnel to the selected numeric address, retain the hostname for TLS SNI and certificate verification, and reject every profile that asks for a downgrade.
6. Xray sends WireGuard packets to the loopback UDP side of wstunnel. Ket proves a certificate-authenticated HTTPS request through the resulting SOCKS path before attaching the full-route bridge.

The agent rejects private, loopback, link-local, carrier-grade NAT, multicast, and outbound SMTP destinations. Its manager token, WireGuard private key, derived client keys, and preshared keys must never be logged or placed in client-visible transport options.

## Host prerequisite

The server requires a Linux host whose kernel can create WireGuard interfaces and a rootful Docker daemon. Check this before deployment:

```bash
sudo modprobe wireguard
sudo ip link add dev ketwg-check type wireguard
sudo ip link delete dev ketwg-check
```

The overlay grants only `NET_ADMIN` to the agent. Do not give the control plane or wstunnel container that capability. Rootless or nested Docker environments that deny `ip link add ... type wireguard` cannot run this server data plane.

## Configure

Create the WireGuard key pair with restrictive permissions:

```bash
umask 077
wg genkey | tee /tmp/ket-wireguard-private.key | wg pubkey > /tmp/ket-wireguard-public.key
cat /tmp/ket-wireguard-private.key
cat /tmp/ket-wireguard-public.key
openssl rand -base64 48
openssl rand -base64 48
openssl rand -hex 24
```

Put the private and public key in `KET_WIREGUARD_SERVER_PRIVATE_KEY` and `KET_WIREGUARD_SERVER_PUBLIC_KEY`. Put the two independent base64 values in `KET_WIREGUARD_MANAGER_TOKEN` and `KET_WIREGUARD_CREDENTIAL_KEY`. Use the hexadecimal value in a path prefix such as `ket-<value>`, then set:

```dotenv
KET_WIREGUARD_ENABLED=true
KET_WIREGUARD_PUBLIC_HOST=wg.example.com
KET_WIREGUARD_PUBLIC_PORT=443
KET_WIREGUARD_SNI=wg.example.com
KET_WIREGUARD_WS_PATH_PREFIX=ket-replace-with-the-random-prefix
KET_WIREGUARD_ORIGIN_BIND_ADDRESS=127.0.0.1
KET_WIREGUARD_ORIGIN_PORT=8446
KET_WIREGUARD_SERVER_PRIVATE_KEY=<private key>
KET_WIREGUARD_SERVER_PUBLIC_KEY=<public key>
KET_WIREGUARD_MANAGER_TOKEN=<independent 32+ character secret>
KET_WIREGUARD_CREDENTIAL_KEY=<independent 32+ character secret>
```

Validate and start the overlay:

```bash
set -a; . ./.env; set +a
./packaging/validate-env.sh
docker compose -f compose.yaml -f compose.wireguard.yaml config --quiet
docker compose -f compose.yaml -f compose.wireguard.yaml up --build -d
docker compose -f compose.yaml -f compose.wireguard.yaml ps
```

## Cloudflare Tunnel ingress

[Cloudflare Tunnel supports WebSockets](https://developers.cloudflare.com/cloudflare-one/faq/cloudflare-tunnels-faq/) and connects to the origin outbound, so the origin remains bound to `127.0.0.1:8446`. A locally managed `/etc/cloudflared/config.yml` can route a dedicated hostname as follows:

```yaml
ingress:
  - hostname: wg.example.com
    service: http://localhost:8446
    originRequest:
      httpHostHeader: wg.example.com
  - service: http_status:404
```

When sharing a hostname with another service, put the WireGuard rule first and restrict it to the configured prefix:

```yaml
ingress:
  - hostname: ket.example.com
    path: ^/ket-replace-with-the-random-prefix
    service: http://localhost:8446
  - hostname: ket.example.com
    service: http://localhost:8787
  - service: http_status:404
```

Validate the effective rule before restarting cloudflared:

```bash
sudo cloudflared --config /etc/cloudflared/config.yml tunnel ingress validate
sudo cloudflared --config /etc/cloudflared/config.yml tunnel ingress rule \
  'https://wg.example.com/ket-replace-with-the-random-prefix'
sudo systemctl restart cloudflared
```

Configure the Cloudflare DNS/public-hostname record as proxied. Do not add an OCI public ingress rule for `8446` or `51820`: `8446` is loopback-only and `51820/udp` exists only between the two private Compose services. The public client connects to Cloudflare on TCP `443`.

## Platform support

- The server image and desktop packages pin wstunnel 10.6.2 for Linux amd64/arm64 and Windows amd64. Docker selects the native image architecture; an amd64 image does not run transparently on arm64 without emulation.
- Android bundles the official wstunnel arm64 payload, which targets API 28. Arm64 Android 9+ can attempt WireGuard TLS; other Android architectures and older releases skip it and continue through ranked fallback.
- Desktop and Android require the pinned Xray WireGuard outbound. They do not invoke operating-system WireGuard configuration or store a persistent client profile.

## Verification

Image and configuration validation do not prove the public path. After deployment, create a fresh session and verify all of these conditions:

1. `wireguard-agent` is healthy and `wg show ketwg0` lists only current lease peers.
2. The client selects `WireGuard TLS`, completes its HTTPS path proof, and carries TCP plus UDP traffic.
3. `/v1/sessions/current` reports nonzero WireGuard sent and received bytes and a recent online handshake.
4. Releasing the session removes the peer; reconnecting with the old profile fails.
5. A wrong hostname, untrusted certificate, wrong WebSocket path, or blocked origin fails closed and falls back without disabling TLS verification.
6. The same flow succeeds from the restricted Wi-Fi and cellular networks that matter for the deployment.

Ket's local checks cover strict profile parsing, deterministic/redacted credentials, Xray configuration validation, wstunnel UDP forwarding, Android packaging, Docker image construction, and manager API parsing. A kernel WireGuard peer lifecycle test and restricted-network packet-flow test remain deployment gates when the build host cannot create WireGuard interfaces.
