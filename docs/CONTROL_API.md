# Control API v1

The base URL is the configured `KET_PUBLIC_URL`. JSON requests use `Content-Type: application/json`. Admin and session endpoints authenticate with `Authorization: Bearer <token>`.

## Public endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/healthz` | Liveness, returns `204` |
| `GET` | `/readyz` | Readiness |
| `GET` | `/metrics` | Prometheus session, capacity, crypto-pressure, overload, and data-plane authentication metrics |
| `GET` | `/v1/node/status` | Location, health, system load, and session capacity |
| `POST` | `/v1/sessions` | Exchange a 32-character access code for a short-lived session |

Create a session:

```json
{
  "access_code": "32-character-code-from-the-operator",
  "client_name": "Aadhish's Android phone"
}
```

The response contains a bearer `session_token`, its expiry, the complete node status used by map and capacity UI, and an ordered `transports` array. An implemented transport has a scoped `credential.auth` value that is distinct from the control bearer. Clients must treat transport `options` as adapter-owned values and reject unknown required options.

```json
{
  "id": "hy2-primary",
  "protocol": "hysteria2",
  "endpoint": "vpn.example.com",
  "port": 443,
  "network": "udp",
  "tls_server_name": "vpn.example.com",
  "options": {},
  "credential": {
    "auth": "lease-scoped-secret",
    "secrets": {}
  }
}
```

Protocol metadata under `options` is non-secret. Protocol-specific passwords or keys are returned under `credential.secrets` and receive the same lifecycle and redaction treatment as `credential.auth`. For example, an obfuscated Hysteria2 profile uses `options.obfs` for the mode and `credential.secrets.obfs_password` for its password.

A VLESS + REALITY profile uses `credential.auth` for its lease-scoped UUID, `credential.secrets.reality_password` for Xray's REALITY public key, and `credential.secrets.reality_short_id` for the short ID. Its non-secret options declare `flow=xtls-rprx-vision`, `transport=raw`, `encryption=none`, and the selected TLS fingerprint. The server installs this UUID in Xray before returning `201`; provisioning failure rolls back the lease and returns `data_plane_unavailable`.

An HTTPS Stealth profile uses `protocol=stealth` for a concrete VLESS + XHTTP/TLS adapter. Its `credential.auth` is the same lease-scoped Xray UUID and its `credential.secrets` object must be empty. The exact option set is `encryption=none`, `transport=xhttp`, `security=tls`, `mode=packet-up`, a 16-128 character absolute `path`, and a supported browser `fingerprint`. Desktop and Android reject unknown options, additional secrets, invalid paths, certificate downgrades, and unsupported XHTTP modes.

A Shadowsocks 2022 profile uses `protocol=shadowsocks2022`, `network=tcp_and_udp`, and the lease-specific public port. `credential.auth` is a standard-base64 32-byte SIP022 key and `credential.secrets` is empty. The exact option set is `method=2022-blake3-aes-256-gcm`, `mode=tcp_and_udp`, and `port_allocation=lease_slot`; there is no TLS name. The key and port are deterministic for a server key and persisted lease resource slot, but only the slot is stored. Unknown options, malformed keys, extra secrets, or a TLS field fail closed on desktop and Android.

A WireGuard TLS profile uses `protocol=wire_guard`, `network=tcp`, and a certificate-verified TLS server name. `credential.auth` is a standard-base64 32-byte client private key; `credential.secrets` contains only `preshared_key` and `server_public_key`, both standard-base64 32-byte WireGuard keys. The exact options are `address_allocation=lease_slot`, `allowed_ips=0.0.0.0/0`, `keepalive_seconds=25`, `mtu=1280`, `transport=websocket_tls`, `client_address`, `path_prefix`, and the manager-restricted `remote_address`. Unknown fields, malformed keys or addresses, unsafe paths, extra secrets, TLS downgrades, and route changes fail closed on desktop and Android.

An OpenVPN/stunnel profile uses `protocol=open_vpn_stunnel`, `network=tcp`, and a certificate-verified TLS server name. `credential.auth` is the 44-character scoped OpenVPN password. Its secret keys are exactly `username`, `ca_certificate_pem_b64`, `stunnel_ca_certificate_pem_b64`, and `tls_crypt_key_b64`; the username must equal the public 12-character prefix of the scoped password. The exact options are `auth_mode=session_token`, `cipher=aes_256_gcm`, `remote_cert_tls=server`, `tls_crypt=v1`, `tls_minimum=1.2`, and `transport=stunnel_tls`. Linux/Windows reject unknown fields, malformed key envelopes, extra secrets, certificate identity mismatches, and downgrade-shaped values. Android currently skips this unsupported profile and continues ranked fallback.

## Session endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/v1/sessions/current` | Current lease, node status, bytes sent/received, and online connections |
| `PUT` | `/v1/sessions/current` | Renew the current lease and return current metrics |
| `DELETE` | `/v1/sessions/current` | Release the current lease |

## Admin endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/v1/admin/access-grants` | Create a grant and return its code once |
| `GET` | `/v1/admin/access-grants` | List grants and active lease counts |
| `POST` | `/v1/admin/access-grants/batch` | Create 1-100 grants in one authenticated request |
| `DELETE` | `/v1/admin/access-grants/{id}` | Revoke a grant and all of its sessions |

Create a grant:

```json
{
  "label": "Personal devices",
  "max_connections": 5,
  "expires_at_epoch_seconds": null
}
```

The plaintext `access_code` is available only in the creation response. Store it in a password manager. Revocation removes its leases before the server asks each configured data plane to kick the corresponding client IDs.

## Data-plane endpoint

`POST /internal/v1/hysteria2/auth` implements Hysteria2's HTTP authentication contract. It always returns HTTP `200` with `{"ok":true,"id":"session-id"}` or `{"ok":false,"id":""}`. The protocol container reaches it over the private Compose network. It is not a client API, and an ingress or reverse proxy must reject the entire `/internal/` namespace.

`POST /internal/v1/openvpn/auth` accepts OpenVPN's scoped username/password pair only from the auth helper. It additionally requires the independent `KET_OPENVPN_AUTH_TOKEN` bearer and returns `204` on success or `401` on rejection. The username must match the token's session prefix; release, expiry, and revocation reject it immediately.

The WireGuard agent exposes its own bearer-authenticated `/healthz` and `/v1/peers` manager API only on the dedicated private Compose network. It is not part of the public control API and must never be routed by cloudflared or a reverse proxy.

The OpenVPN agent similarly exposes bearer-authenticated `/healthz`, `GET /v1/sessions`, `PUT /v1/sessions/reconcile`, and `POST /v1/sessions/remove` endpoints only on `openvpn-control`. They wrap a Unix OpenVPN management socket and must never be publicly routed.

The batch request uses `label_prefix`, `count`, `max_connections`, and optional `expires_at_epoch_seconds`. Labels receive `-1` through `-N` suffixes. Each response contains a distinct 32-character code, returned only once and never persisted in plaintext.

## Errors

Errors use a stable machine code and a user-safe message:

```json
{
  "code": "grant_capacity_reached",
  "message": "the access grant connection limit has been reached"
}
```

Known codes are `unauthorized`, `invalid_request`, `grant_expired`, `grant_capacity_reached`, `node_capacity_reached`, `server_busy`, `data_plane_unavailable`, `not_found`, and `internal_error`. Secret verification uses a bounded worker pool and bounded admission queue. When that queue is full, the server returns `429 server_busy` with `Retry-After: 1` instead of accumulating unbounded Argon2 work; clients may retry after that delay.
