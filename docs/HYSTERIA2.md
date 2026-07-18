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

## Production prerequisites

- A hostname resolving to the server.
- A valid certificate and key readable by UID `10001` at `secrets/tls/fullchain.pem` and `secrets/tls/privkey.pem` by default.
- UDP `443` allowed by both the cloud network policy and host firewall.
- Independent random values of at least 32 characters for `KET_HYSTERIA_STATS_SECRET` and, when enabled, `KET_HYSTERIA_OBFS_PASSWORD`.
- A deliberate HTTPS masquerade origin whose behavior and hostname make sense for the deployment.

Cloudflare's normal proxied DNS records handle HTTP/HTTPS, not arbitrary UDP. Keep the Hysteria hostname [DNS-only](https://developers.cloudflare.com/dns/proxy-status/) unless you intentionally deploy a compatible [Spectrum UDP application](https://developers.cloudflare.com/spectrum/); custom TCP/UDP Spectrum applications may require Enterprise service.

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

The control API remains loopback-only by default. The overlay publishes only the Hysteria UDP port publicly. Terminate control-plane HTTPS in a separate reverse proxy and deny public access to `/internal/`, which is reserved for the Hysteria container's private authentication callback.

## Abuse controls

The generated ACL blocks private, loopback, link-local, multicast, carrier-grade NAT, and outbound SMTP destinations. This prevents common metadata-service and lateral-network access through the proxy. Operators needing private-network access must make a conscious code/config change rather than silently weakening the default.

The Hysteria stats API is never published to the host and requires the configured secret on the private Compose network. The engine runs without Linux capabilities and cannot read Ket's control-state volume.
