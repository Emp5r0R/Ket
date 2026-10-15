# Ket Invite Capsule V1

## Purpose

An invite replaces typing a server URL and a 32-character access code with one
signed string (and, from Phase 3, a QR code). It carries everything a client
needs to enroll and the node's Ed25519 public key, so the client can check that
the server it enrolls with is the node that issued the invite.

The format is adapted from Abyssal's Invite Capsule V1. Differences: the
capability is Ket's existing access code (so the session API is unchanged), the
locators are HTTPS control endpoints, there is no manual Crockford form (the
URL + access code entry remains available under "Advanced"), and node identity
is proven by a signed enrollment response instead of a separate public node
descriptor route, which would add a probe-visible endpoint.

## Primitive types

- Deterministic CBOR subset: unsigned integers in the shortest form, definite
  byte strings, text strings, and arrays, plus `null`. Maps, tags, floats,
  negative integers, indefinite lengths, and trailing bytes are rejected.
- Decoders re-encode the decoded object and require a byte-for-byte match, so
  every accepted invite has exactly one encoding.
- Ed25519 keys are 32 bytes and signatures 64 bytes (RFC 8032).
- Length prefixes in signature inputs are unsigned big-endian.

## Signed invite

```text
signed_invite = [ bstr(canonical_payload), bstr64(signature) ]
```

`canonical_payload` is an array of exactly ten fields:

| Index | Type | Meaning |
| --- | --- | --- |
| 0 | uint | format version, exactly `1` |
| 1 | tstr | application ID, exactly `org.ket.access` |
| 2 | uint | capability type, `1` = access grant |
| 3 | bstr32 | node Ed25519 public key |
| 4 | array | one to four unique control locators |
| 5 | tstr | access code: exactly 32 ASCII letters and digits |
| 6 | uint | minimum control API version, `1` |
| 7 | uint | maximum control API version, `>=` minimum |
| 8 | uint | flags, exactly `0` in V1 |
| 9 | uint or null | access expiry in Unix seconds |

The node signs:

```text
"KET-INVITE-CAPSULE-V1" || u16be(len(application_id)) || application_id || canonical_payload
```

## Locators

```text
Https               = [1, lowercase_dns_host, port]
LoopbackDevelopment = [2, host_tag, port]
```

`Https` maps to `https://<host>[:port]/`. Hosts must be canonical lowercase
ASCII DNS names: IP literals, single-label names, trailing dots, and the
`.local`, `.localhost`, `.internal`, `.onion`, `.i2p`, `.home.arpa`, `.test`,
`.example`, and `.invalid` suffixes are rejected. Ports are 1-65535.

Development host tags are `1 = localhost`, `2 = 127.0.0.1`, `3 = ::1`, and
`4 = 10.0.2.2` (Android emulator), mapping to `http://<host>:<port>/`.
Clients use the first locator they support. Unknown locator tags fail the whole
invite.

## Text form

```text
ket:invite:<base64url without padding of signed_invite>
```

The bare Base64URL value is also accepted. Padding, whitespace, other
alphabets, and non-canonical Base64URL are rejected.

## Limits and validation

- Encoded text at most 2,048 bytes; binary invite at most 1,024 bytes.
- Unknown versions, application IDs, capability types, flags, and locator tags
  fail closed. Node key must not be all zeroes.
- Expiry is covered by the signature. Clients reject an expired invite; the
  server remains authoritative for grant expiry and revocation.
- The access code is a bearer secret. Invites must not be logged or placed in
  URLs, analytics, crash reports, or tickets.

## Node-authenticated enrollment

A client that enrolls from an invite pins the node public key and sends:

```text
POST /v1/sessions
Ket-Enrollment-Nonce: <base64url of 32 random bytes>
```

The server answers a successful enrollment with:

```text
Ket-Node-Signature: <base64url of 64-byte Ed25519 signature>
```

over

```text
"KET-ENROLLMENT-V1" || nonce || u32be(len(body)) || body
```

where `body` is the exact response body bytes. The client verifies the
signature before parsing the body and fails closed when the header is missing,
malformed, or invalid. A request without the nonce header is unchanged, so
URL + access-code clients and v1.0.1 clients keep working. A malformed nonce is
rejected with `400`, which the public edge masks as its neutral `404`.

This proves the response came from the holder of the node key even when TLS
terminates elsewhere (Cloudflare) or a certificate is mis-issued. Renewal and
status responses are not signed; they carry no transport credentials.

## Node signing key

`KET_NODE_SIGNING_KEY` holds the 32-byte Ed25519 seed as Base64URL without
padding in the server `.env` (mode `0600`, like the other server secrets). The
installer generates it once and never rotates it. Rotating it invalidates every
outstanding invite and every client profile pinned to the old key. When the key
is set, `KET_PUBLIC_URL` must be an `https://` DNS-name URL without a path (or a
loopback development URL) so the server can derive the invite locator.

Grant creation (`POST /v1/admin/access-grants` and `/batch`) returns an
`invite` field next to `access_code` when the key is configured. Servers
installed before invites can add a key once; see
[SERVER_INSTALL.md](SERVER_INSTALL.md#installed-layout).

## Trust boundary

The signature detects modification of an invite received through a trusted
channel; it cannot tell an intended invite from a complete, independently
valid replacement. Possession of an invite grants the access code it contains.

## Implementations

- Rust reference: `crates/ket-invite` (server issuance in `crates/ket-server/src/invite.rs`,
  desktop enrollment in `crates/ket-client-core/src/invite.rs`). Signatures use
  `ed25519-dalek` with strict verification.
- Android: `apps/ket-android/app/src/main/java/com/ket/android/invite/`, a direct port
  that runs the same checks in the same order. Android 8 (API 26) has no platform
  Ed25519, so verification uses Google Tink's `Ed25519Verify`. Known difference: a
  node key that is 32 nonzero bytes but not a valid curve point is reported as
  `invalid_node_key` by Rust and `invalid_invite_signature` by Android; both reject it.

## Conformance vectors

Fixed, non-production vectors live in
`crates/ket-invite/tests/vectors/invite-v1.json`. The Rust crate regenerates
them in its tests and fails on drift; the Android JVM tests parse the same
file.
