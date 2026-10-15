# Manual deployment

The [one-command installer](SERVER_INSTALL.md) is the supported way to run a Ket server. This page is for operators who assemble the Docker Compose stack themselves.

```bash
cp .env.example .env
openssl rand -base64 48
# Put the generated value in KET_ADMIN_TOKEN, then set the public URL and location.
set -a; . ./.env; set +a
./packaging/validate-env.sh
docker compose up --build -d
curl --fail http://127.0.0.1:8787/healthz
```

The Compose default publishes only on loopback. Put a TLS reverse proxy in front of port `8787`, or explicitly set `KET_PUBLISH_ADDRESS` when a different binding is required. Do not expose an unencrypted control API over the public internet, and deny all public requests to `/internal/`; that namespace is reserved for private data-plane callbacks.

The shell preflight rejects malformed operator-facing URL and node fields before Compose starts. `ket-server` repeats the authoritative structured validation at process startup, including HTTPS-or-loopback URL policy, map metadata, transport count and identifier bounds, endpoint/TLS names, and option maps, so it cannot emit a manifest that Ket clients reject for shape.

To enable the Hysteria2 data plane, install a valid certificate under `secrets/tls`, set `KET_HYSTERIA_ENABLED=true` and the related Hysteria values in `.env`, then use the overlay:

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml up --build -d
```

Before starting the overlay, validate the fully rendered configuration (this
also catches missing `.env` values and malformed port mappings):

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml config --quiet
```

The overlay keeps the control plane on its private Compose network and publishes
only Hysteria2's UDP listener. The generated Hysteria configuration is written
with mode `0600` into the shared `ket-dataplane` volume; do not bind-mount that
volume or expose the internal authentication and statistics endpoints.

This publishes Hysteria2 on UDP `443` by default. The hostname must reach the server over UDP; normal Cloudflare orange-cloud HTTP proxying is not a UDP tunnel. Use a DNS-only record or a deliberately configured Layer 4 product compatible with unmodified Hysteria packets.

To enable the VLESS + REALITY server data plane, generate the X25519 and server-only credential keys described in [the deployment guide](XRAY_REALITY.md), set `KET_XRAY_ENABLED=true`, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.xray.yaml -f compose.edge.yaml config --quiet
docker compose -f compose.yaml -f compose.xray.yaml -f compose.edge.yaml up --build -d
```

The edge inspects only the TLS ClientHello SNI on TCP `443`: the configured REALITY disguise SNI is passed directly to Xray, while normal Ket HTTPS continues to the certificate-terminating HTTP edge. The REALITY hostname must resolve directly to the server through a DNS-only record; an ordinary Cloudflare Tunnel or orange-cloud HTTP proxy does not forward unmodified VLESS + REALITY traffic. The pinned Xray image is a multi-architecture manifest, so Docker selects the native `linux/amd64` or `linux/arm64` image for the host.

To enable the TLS-shaped Stealth path, set `KET_XHTTP_ENABLED=true`, configure the public Cloudflare hostname and an unguessable `KET_XHTTP_PATH`, then include the XHTTP overlay:

```bash
docker compose -f compose.yaml -f compose.xhttp.yaml config --quiet
docker compose -f compose.yaml -f compose.xhttp.yaml up --build -d
```

The XHTTP origin is published only on host loopback port `8445`. Route only its configured path from Cloudflare Tunnel to that origin and keep the remaining hostname routes pointed at the control API. The public client connection is certificate-verified TLS and browser-fingerprinted HTTP; the private origin hop is plain HTTP inside the authenticated outbound Cloudflare Tunnel. Exact ingress configuration and validation commands are in [the HTTPS Stealth guide](XHTTP_STEALTH.md).

To enable Shadowsocks 2022, generate its independent credential key, configure an inclusive port range containing at least `KET_MAX_SESSIONS` ports, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.shadowsocks.yaml config --quiet
docker compose -f compose.yaml -f compose.shadowsocks.yaml up --build -d
```

Each active lease owns one stable TCP+UDP port and a distinct SIP022 key. Open the complete configured range for both TCP and UDP in the cloud security list and host firewall. The public hostname must be a direct DNS-only record; an ordinary Cloudflare orange-cloud proxy or Cloudflare Tunnel does not carry native Shadowsocks. The UDP manager stays isolated on the private Compose network. Exact key, range, and validation commands are in [the Shadowsocks 2022 guide](SHADOWSOCKS2022.md).

To enable WireGuard TLS, generate its server key pair and two independent server-only secrets, set an unguessable WebSocket path prefix, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.wireguard.yaml config --quiet
docker compose -f compose.yaml -f compose.wireguard.yaml up --build -d
```

The overlay runs a capability-limited WireGuard kernel agent and an unprivileged `wstunnel` origin published only on host loopback port `8446`. Route the configured WebSocket path from Cloudflare Tunnel to `http://localhost:8446`; clients connect to certificate-verified `wss://` on the public hostname. This path needs no public OCI ingress rule. It is a self-hosted transport inspired by the same general WireGuard-over-TLS strategy as Proton Stealth, but it is not Proton-compatible. Exact setup and validation commands are in [the WireGuard TLS guide](WIREGUARD_TLS.md).

To enable OpenVPN over stunnel, create the OpenVPN and stunnel PKI material, generate two independent server tokens, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.openvpn.yaml config --quiet
docker compose -f compose.yaml -f compose.openvpn.yaml up --build -d
```

The outer stunnel connection is certificate-verified TLS and the inner OpenVPN connection independently verifies its server certificate and `tls-crypt` key. This is direct generic TLS, not HTTP, so use a DNS-only record and open the configured TCP port. The OpenVPN agent owns only `NET_ADMIN` and `/dev/net/tun`; its management API and the authentication callback remain on a private Compose network. Exact PKI, ingress, port-collision, and revocation details are in [the OpenVPN/stunnel guide](OPENVPN_STUNNEL.md).

Hysteria2 and REALITY can share port `443` because they use UDP and TCP respectively. OpenVPN/stunnel is also TCP and therefore needs a different host bind address or public port from REALITY. HTTPS Stealth and WireGuard TLS use separate Cloudflare hostnames and loopback-only origins, while Shadowsocks uses its own TCP+UDP range. To run all six server/desktop transports, enable every environment section, assign OpenVPN a non-conflicting TCP listener, and include all overlays:

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml -f compose.edge.yaml config --quiet
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml -f compose.edge.yaml up --build -d
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml -f compose.edge.yaml ps
```

The control, XHTTP, and WireGuard TLS hostnames may remain behind Cloudflare Tunnel, but Hysteria2, REALITY, Shadowsocks, and OpenVPN/stunnel session profiles must advertise a direct server IP or DNS-only hostname. Open only the exact raw-transport ports in both the cloud network policy and host firewall; keep all Cloudflare Tunnel origin ports closed publicly.

On a server set up by the installer, create invites with `sudo ket invite` (see [SERVER_INSTALL.md](SERVER_INSTALL.md#managing-the-server)). The raw admin API calls below work on any deployment.

Create one client access code on an installed server:

```bash
sudo bash -c '
  set -euo pipefail
  cd /opt/ket
  set -a; . ./.env; set +a
  max_connections=$((KET_MAX_SESSIONS < 5 ? KET_MAX_SESSIONS : 5))
  response_file=$(mktemp)
  trap "rm -f \"$response_file\"" EXIT
  http_status=$(curl --silent --show-error \
    --request POST \
    --header "Authorization: Bearer ${KET_ADMIN_TOKEN}" \
    --header "Content-Type: application/json" \
    --data "{\"label\":\"Personal devices\",\"max_connections\":${max_connections},\"valid_for_minutes\":43200}" \
    --output "$response_file" \
    --write-out "%{http_code}" \
    "http://127.0.0.1:${KET_CONTROL_PORT}/v1/admin/access-grants")
  if [[ $http_status != 201 ]]; then
    jq -r "\"Ket error (\\(.code // \\\"unknown\\\")): \\(.message // \\\"request failed\\\")\"" "$response_file" >&2
    exit 1
  fi
  jq -r "\"Grant ID: \\(.id)\\nAccess code: \\(.access_code)\\nConcurrent connections: \\(.max_connections)\\nValid for: \\(.valid_for_minutes) minutes\\nExpires at: \\(.expires_at_epoch_seconds)\"" "$response_file"
'
```

Create ten separate one-device codes:

```bash
sudo bash -c '
  set -euo pipefail
  cd /opt/ket
  set -a; . ./.env; set +a
  response_file=$(mktemp)
  trap "rm -f \"$response_file\"" EXIT
  http_status=$(curl --silent --show-error \
    --request POST \
    --header "Authorization: Bearer ${KET_ADMIN_TOKEN}" \
    --header "Content-Type: application/json" \
    --data "{\"label_prefix\":\"Client\",\"count\":10,\"max_connections\":1,\"valid_for_minutes\":43200}" \
    --output "$response_file" \
    --write-out "%{http_code}" \
    "http://127.0.0.1:${KET_CONTROL_PORT}/v1/admin/access-grants/batch")
  if [[ $http_status != 201 ]]; then
    jq -r "\"Ket error (\\(.code // \\\"unknown\\\")): \\(.message // \\\"request failed\\\")\"" "$response_file" >&2
    exit 1
  fi
  jq -r ".[] | [.label, .access_code, .valid_for_minutes, .expires_at_epoch_seconds, .id] | @tsv" "$response_file"
'
```

`max_connections` is the number of concurrent sessions allowed by that code and must not exceed the server's `KET_MAX_SESSIONS`. `valid_for_minutes` is required and accepts `1` through `525600` minutes. Ket returns each plaintext code only once and stores only its Argon2 hash. Existing codes cannot be recovered; create a replacement when a code is lost. Use the server URL such as `https://ket.nsa.tools` plus the returned 32-character code in any Ket client. Clients retain the profile and show its remaining access time until this server-enforced lifetime ends.
