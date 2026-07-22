# Hysteria2 data plane

Ket integrates the maintained Hysteria2 engine instead of implementing QUIC, congestion control, or cryptography itself. The integration follows Hysteria's official [HTTP authentication](https://v2.hysteria.network/docs/advanced/Full-Server-Config/#http-authentication) and [traffic statistics](https://v2.hysteria.network/docs/advanced/Traffic-Stats-API/) contracts.

## Credential lifecycle

1. Ket creates independent control and data-plane secrets under the same public session ID.
2. The session manifest places only the data-plane secret in the Hysteria2 transport's `credential.auth` field. An optional obfuscation password is placed in `credential.secrets`, never public profile options.
3. Hysteria sends that value to Ket for each new client connection.
4. Ket checks the secret hash, lease expiry, and parent grant before returning the session ID.
5. Hysteria uses the ID for `/traffic`, `/online`, and `/kick`; Ket maps those values into the session status API.
6. Release, revocation, and automatic expiry remove authorization before attempting a connection kick, so reconnect loops cannot regain access.

Neither secret is stored in plaintext. The transport credential cannot call bearer-protected control endpoints.

## Desktop packet path

The privileged desktop service resolves the advertised endpoint before changing routes, pins Hysteria to one resulting IP while retaining the configured hostname for TLS verification, and starts Hysteria's UDP-capable loopback SOCKS5 mode. The shared `tun2proxy` bridge then captures IPv4, IPv6, and DNS with virtual-DNS mode while bypassing every resolved server IP. Hysteria and the bridge are supervised as one tunnel: either process exiting stops the other, and explicit disconnect or lease expiry restores bridge-owned system state.

## Production prerequisites

- A direct server IP or DNS-only hostname for the transport endpoint, plus an SNI hostname covered by the certificate. These may be different values.
- A valid certificate and key readable by UID `10001` at `secrets/tls/fullchain.pem` and `secrets/tls/privkey.pem` by default.
- UDP `443` allowed by both the cloud network policy and host firewall.
- Independent random values of at least 32 characters for `KET_HYSTERIA_STATS_SECRET` and, when enabled, `KET_HYSTERIA_OBFS_PASSWORD`.
- A deliberate HTTPS masquerade origin whose behavior and hostname make sense for the deployment.

Cloudflare's normal proxied DNS records handle HTTP/HTTPS, not arbitrary UDP. Keep the Hysteria hostname [DNS-only](https://developers.cloudflare.com/dns/proxy-status/) unless you intentionally deploy a compatible [Spectrum UDP application](https://developers.cloudflare.com/spectrum/); custom TCP/UDP Spectrum applications may require Enterprise service.

The control hostname may still use Cloudflare Tunnel. In that layout, set `KET_HYSTERIA_PUBLIC_HOST` to the direct IP or DNS-only data-plane endpoint and `KET_HYSTERIA_SNI` to the certificate hostname. The client connects to the former while verifying the latter.

## Modes

`KET_HYSTERIA_OBFS=none` preserves Hysteria's standard HTTP/3 appearance and active-probe masquerade behavior. Use `salamander` when a network blocks recognizable QUIC but permits other UDP. `gecko` additionally fragments handshake datagrams and is currently marked experimental upstream. Obfuscation hides the HTTP/3 shape, so it does not also behave as a normal HTTP/3 server.

Ket renders the selected mode into public profile options and its secret into the authenticated credential object. It never labels obfuscation as encryption.

## Start

Set `KET_HYSTERIA_ENABLED=true` and complete every `KET_HYSTERIA_*` value in `.env`, then run:

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml config --quiet
docker compose -f compose.yaml -f compose.hysteria.yaml up --build -d
docker compose -f compose.yaml -f compose.hysteria.yaml ps
```

Hysteria2 UDP `443` and REALITY TCP `443` can run simultaneously:

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.edge.yaml config --quiet
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.edge.yaml up --build -d
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.edge.yaml ps
```

The control API remains loopback-only by default. The overlay publishes only the Hysteria UDP port publicly. Terminate control-plane HTTPS in a separate reverse proxy and deny public access to `/internal/`, which is reserved for the Hysteria container's private authentication callback.

## Certificate renewal through Cloudflare Tunnel

When the control hostname is a Cloudflare Tunnel route, HTTP-01 can reach a loopback-only Certbot responder without opening another Oracle or host port. Put a narrowly scoped rule before the control catch-all while retaining the `/internal` denial first:

```yaml
ingress:
  - hostname: ket.example.com
    path: "^/internal(?:/.*)?$"
    service: http_status:404
  - hostname: ket.example.com
    path: "^/.well-known/acme-challenge/.+$"
    service: http://127.0.0.1:8888
  - hostname: ket.example.com
    service: http://127.0.0.1:8787
  - service: http_status:404
```

Validate and restart `cloudflared`, then issue the certificate without publishing port `8888`:

```bash
sudo cloudflared --config /etc/cloudflared/config.yml tunnel ingress validate
sudo systemctl restart cloudflared
sudo certbot certonly --standalone --http-01-port 8888 \
  --preferred-challenges http --domain ket.example.com
```

Copy rather than symlink the renewed files into `secrets/tls`: the Hysteria container cannot traverse Certbot's root-only archive directories. Keep the destination directory `0750` and the certificate/key `0640`, owned by `root` and group `10001`. Install a Certbot deploy hook that refreshes those copies atomically and restarts only the Hysteria service. Verify the complete renewal path with `certbot renew --dry-run`; a certificate's existence alone does not prove unattended renewal.

After deployment, exercise an authenticated Hysteria session from outside the VCN, carry HTTPS through it, and confirm `/v1/sessions/current` reports `traffic.available=true` with nonzero sent and received bytes. A listening UDP socket is not sufficient evidence that the cloud ingress rule, certificate, obfuscation credentials, and lease-scoped authentication all work together.

## Abuse controls

The generated ACL blocks private, loopback, link-local, multicast, carrier-grade NAT, benchmark-network, and outbound SMTP destinations. This prevents common metadata-service, synthetic-DNS, and lateral-network access through the proxy. Operators needing private-network access must make a conscious code/config change rather than silently weakening the default.

The Hysteria stats API is never published to the host and requires the configured secret on the private Compose network. The engine runs without Linux capabilities and cannot read Ket's control-state volume.
