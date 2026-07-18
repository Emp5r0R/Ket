# VLESS + REALITY deployment

Ket uses the maintained Xray-core engine for the VLESS + REALITY data plane. The pinned `26.3.27` image is referenced by multi-architecture digest and provides native `linux/amd64` and `linux/arm64` variants. The Rust control plane renders the runtime configuration, installs a lease-scoped UUID before returning a session, reads per-user traffic and online counts, and removes users on release, expiry, or grant revocation.

## Network requirements

- Point a dedicated DNS-only `A` or `AAAA` record at the server, such as `ket-vpn.example.com`.
- Allow inbound TCP `443` in the cloud security list and host firewall.
- Do not put the record behind an ordinary Cloudflare Tunnel or orange-cloud HTTP proxy. VLESS + REALITY is a raw TCP data plane, not an HTTP origin.
- Keep the Ket control API on its separate HTTPS hostname. Deny public access to `/internal/` on that ingress.
- Select a stable TLS 1.3 and HTTP/2 REALITY target reachable from the server. Its certificate chain must fit REALITY's handshake capture boundary. `KET_XRAY_SNI` must appear in `KET_XRAY_SERVER_NAMES` and match the target certificate; verify the completed tunnel before production rollout because ordinary HTTPS reachability alone is insufficient.

TCP `443` can coexist with Hysteria2 UDP `443` on the same server and hostname because they use different network protocols.

## Generate secrets

Generate the REALITY X25519 keypair with the exact pinned engine:

```bash
docker run --rm \
  ghcr.io/xtls/xray-core:26.3.27@sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7 \
  x25519
```

Put `PrivateKey` in `KET_XRAY_PRIVATE_KEY` and `Password (PublicKey)` in `KET_XRAY_PUBLIC_KEY`. Generate the remaining values independently:

```bash
openssl rand -hex 8
openssl rand -base64 48
```

The hexadecimal value is `KET_XRAY_SHORT_ID`; the base64 value is the server-only `KET_XRAY_CREDENTIAL_KEY`. Do not reuse `KET_ADMIN_TOKEN` or expose the private or credential keys to clients.

## Configure and start

Set these values in `.env`:

```dotenv
KET_XRAY_ENABLED=true
KET_XRAY_PUBLIC_HOST=ket-vpn.example.com
KET_XRAY_PUBLIC_PORT=443
KET_XRAY_SNI=www.cloudflare.com
KET_XRAY_SERVER_NAMES=www.cloudflare.com
KET_XRAY_REALITY_TARGET=www.cloudflare.com:443
KET_XRAY_PRIVATE_KEY=<PrivateKey>
KET_XRAY_PUBLIC_KEY=<Password (PublicKey)>
KET_XRAY_SHORT_ID=<16 hex characters>
KET_XRAY_CREDENTIAL_KEY=<independent 32+ character secret>
KET_XRAY_FINGERPRINT=chrome
```

Validate before starting:

```bash
set -a; . ./.env; set +a
./packaging/validate-env.sh
docker compose -f compose.yaml -f compose.xray.yaml config --quiet
docker compose -f compose.yaml -f compose.xray.yaml up --build -d
docker compose -f compose.yaml -f compose.xray.yaml ps
curl --fail http://127.0.0.1:8787/readyz
```

The control container writes `/var/lib/ket-dataplane/xray.json` atomically with mode `0600` before serving liveness. Compose starts Xray only after that liveness check passes and propagates control-service upgrades to the dependent sidecar, preventing Xray from retaining or loading a stale volume configuration. The Xray sidecar mounts the volume read-only and exposes its gRPC API only on the private Compose network. Ket keeps `/readyz` and session exchange unavailable while it retries startup reconciliation, and reports ready only after Xray responds.

## Operations

Session UUIDs are deterministic HMAC derivatives of the server-only credential key and the random Ket session ID. They are stable across control-plane restarts without storing plaintext credentials. Changing `KET_XRAY_CREDENTIAL_KEY`, the REALITY keypair, or the short ID invalidates active client configuration and requires new sessions.

Inspect readiness and logs without printing the generated configuration or `.env` secrets:

```bash
curl --fail http://127.0.0.1:8787/readyz
docker compose -f compose.yaml -f compose.xray.yaml logs --tail=100 control-plane xray
```

Use the normal session release or grant revocation APIs to remove users. Do not expose Xray's port `10085`; it is an unauthenticated private control API within this deployment boundary.
