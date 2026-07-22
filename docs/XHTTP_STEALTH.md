# HTTPS Stealth deployment

Ket's `stealth` profile is VLESS carried by Xray-core 26.3.27 XHTTP in `packet-up` mode. The client reaches a normal certificate-verified TLS hostname through Cloudflare or a compatible HTTP intermediary; that intermediary forwards the configured path to a loopback-only, plaintext XHTTP origin. This is a concrete adapter, not Proton VPN's proprietary service or a claim of identical wire behavior.

XHTTP is preferred over WebSocket because Xray documents WebSocket's fixed HTTP/1.1 ALPN as a significant traffic characteristic. `packet-up` uses ordinary bounded POST requests for upload and a streaming HTTP response for download, making it the most compatible XHTTP mode for restrictive HTTP middleboxes. Ket fixes the mode and rejects unknown or downgrade-shaped options on every client.

## Configure Ket

Generate an independent Xray credential key and an unguessable path:

```bash
openssl rand -base64 48
printf '/ket-%s\n' "$(openssl rand -hex 16)"
```

Set the generated values and public TLS hostname in `.env`:

```dotenv
KET_XHTTP_ENABLED=true
KET_XHTTP_PUBLIC_HOST=stealth.example.com
KET_XHTTP_PUBLIC_PORT=443
KET_XHTTP_SNI=stealth.example.com
KET_XHTTP_PATH=/ket-replace-with-32-random-hex-characters
KET_XHTTP_FINGERPRINT=chrome
KET_XHTTP_ORIGIN_BIND_ADDRESS=127.0.0.1
KET_XHTTP_ORIGIN_PORT=8445
KET_XRAY_CREDENTIAL_KEY=replace-with-the-independent-generated-key
```

The public host and SNI must reach the Cloudflare route. The path is protocol metadata delivered only after authenticated enrollment, but it must still be random to reduce unauthenticated probing. Do not reuse the admin token, access grants, REALITY private key, or Hysteria secrets.

Start XHTTP independently:

```bash
set -a; . ./.env; set +a
./packaging/validate-env.sh
docker compose -f compose.yaml -f compose.xhttp.yaml config --quiet
docker compose -f compose.yaml -f compose.xhttp.yaml up --build -d
```

To run REALITY from the same Xray container, set `KET_XRAY_ENABLED=true` and include both Xray overlays. The Rust control plane renders two inbounds and provisions, reconciles, accounts, and revokes the same lease-scoped UUID in both:

```bash
docker compose -f compose.yaml -f compose.xray.yaml -f compose.xhttp.yaml config --quiet
docker compose -f compose.yaml -f compose.xray.yaml -f compose.xhttp.yaml up --build -d
```

## Route Cloudflare Tunnel

The Compose overlay publishes the XHTTP origin as `127.0.0.1:8445`; it must never be bound to a public address. Put the path-specific rule before the control API rule. XHTTP appends session routing segments to the configured prefix, so the ingress regular expression must include descendants:

```yaml
ingress:
  - hostname: ket.example.com
    path: ^/ket-replace-with-32-random-hex-characters(?:/.*)?$
    service: http://127.0.0.1:8445
  - hostname: ket.example.com
    service: http://127.0.0.1:8787
  - service: http_status:404
```

A dedicated hostname such as `stealth.example.com` is also valid; keep the path rule anyway so unrelated requests do not reach Xray. For a remotely managed tunnel, create the equivalent path-specific public-hostname route in the Cloudflare dashboard and ensure it takes precedence over the hostname-wide control route.

Validate and test locally managed ingress before restarting `cloudflared`:

```bash
sudo cloudflared tunnel ingress validate
sudo cloudflared tunnel ingress rule \
  'https://ket.example.com/ket-replace-with-32-random-hex-characters/test'
```

Do not put Cloudflare Access browser authentication in front of the XHTTP path because the Xray client cannot complete an interactive Access flow. Keep `/internal/` denied and do not route it to the control plane. Provider request limits and acceptable-use rules still apply; the release gate must include a sustained transfer test through the selected Cloudflare plan.

## Security and failure behavior

- Public TLS certificate verification is mandatory; clients never emit `allowInsecure`.
- The client resolves the CDN hostname before installing full routes and excludes every returned edge address to prevent tunnel recursion.
- The origin has no TLS because it is reachable only through host loopback from the authenticated `cloudflared` process. Exposing port `8445` would violate this boundary.
- Each lease receives a deterministic scoped UUID. Session release, expiry, and grant revocation remove it from every configured Xray inbound.
- Multi-inbound provisioning rolls back already-added users if any later inbound fails. Startup reconciliation fails readiness until all configured inbound APIs are healthy.
- XHTTP does not guarantee censorship bypass. Ket ranks it first when configured, then falls back through REALITY and Hysteria2 according to observed failures and cooldown.

The disposable local traffic harness uses the pinned Xray server and client, a one-run CA, and stunnel as the HTTPS intermediary. It validates the Ket-issued profile, rejects a wrong UUID, carries certificate-verified HTTPS through XHTTP `packet-up`, observes Xray traffic counters, and proves both session release and grant revocation stop new connections:

```bash
./packaging/verify-xhttp-tls-traffic.sh
```

The client configuration contains no insecure TLS override. The one-run CA is trusted only through the client Xray process environment, and the production Xray abuse guards remain enabled. The local harness proves the engine and lease lifecycle. Separately, on 2026-07-22, the production Cloudflare route passed Ket's full-route HTTPS gate on an owner-signed arm64 Android client over a restricted Wi-Fi network. Sustained provider limits and broader platform/network coverage remain deployment gates.
