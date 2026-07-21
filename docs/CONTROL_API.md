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
