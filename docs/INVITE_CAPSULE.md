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
terminates elsewhere (Cloudflare) or a certificate is mis-issued. It does not
hide anything: whoever terminates TLS still reads the access code, the session
token, and the credentials in the manifest. Sealed control fixes that.

## Sealed control

Invites carry a range of control API versions. Version 1 is the signed
enrollment above. Version 2 seals every session API request to the node key.
Servers that support it issue invites for versions 1 through 2, so older apps
keep enrolling in the clear while newer apps must seal: an app holding a
version 2 invite never sends a session request in the clear, even when the
server answers `404`. Profiles saved from version 1 invites seal when the server
offers it and fall back to signed enrollment otherwise.

Every sealed request is one `POST /v1/sealed` with an opaque
`application/octet-stream` body:

```text
request  = 0x01 || enc (32 bytes) || HPKE ciphertext
response = response_nonce (32 bytes) || ChaCha20-Poly1305 ciphertext
```

- The request is HPKE (RFC 9180) base mode with DHKEM(X25519, HKDF-SHA256),
  HKDF-SHA256, and ChaCha20-Poly1305, `info = "KET-SEAL-V1 request" ||
  node_ed25519_public_key`, and the version byte as associated data. The
  node's X25519 key is the Montgomery form of its Ed25519 key, so every invite
  already carries it; [Thormarker 2021](https://eprint.iacr.org/2021/509)
  covers using one key pair for both.
- The response follows Oblivious HTTP (RFC 9458, section 4.4): a secret
  exported from the request's HPKE context (`"KET-SEAL-V1 response"`), salted
  with `enc || response_nonce` through HKDF-SHA256, gives the AEAD key
  (`"key"`) and nonce (`"nonce"`). A fresh response nonce means answering a
  replayed request never reuses a key and nonce.
- The plaintext request is JSON: `method`, `path` (`/v1/sessions` or
  `/v1/sessions/current`), `sent_at_epoch_seconds`, and optionally `bearer`
  (the session token), `client` (the `Ket-Client` value), and `body` (the
  enrollment JSON). The plaintext answer is `status`,
  `server_time_epoch_seconds`, and an optional `body`: exactly what the session
  API would have sent in the clear.
- The node refuses a request more than 300 seconds from its clock with
  `400 request_clock_skew` and its own time; the app corrects its offset once
  and retries. It answers a request it has already answered with
  `409 request_replayed`.
- The node answers `200` for every request it can open. A request it cannot
  open gets the same neutral `404` as an unknown page.

A front that terminates TLS therefore sees one route and opaque bodies: not the
access code, the session token, the credentials the node returns, the app
version, or whether a request succeeded. Only the pinned node can open a
request, and only the app that sealed it can open the answer, so a sealed
answer also proves the node's identity. The data-plane protocols still carry
their own credentials; a front that relays a data-plane protocol (XHTTP through
a CDN, for example) sees that protocol's credential the way it always has.

Implementations: `crates/ket-seal` (the construction), the gateway in
`crates/ket-server/src/api/sealed.rs`, the desktop client in
`crates/ket-client-core/src/sealed_control.rs`, and Android through the same
Rust code over JNI (`SealedControlChannel.kt`).

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

- Shared format: `crates/invite-capsule` (MIT OR Apache-2.0), one implementation
  used by Ket and Abyssal. It owns the canonical CBOR, payload, signature,
  locators, and text forms; each application supplies a profile (application
  ID, signature domain, capability, accepted locators, text prefix), so an
  invite for one application is `wrong_application` in the other. Signatures
  use `ed25519-dalek` with strict verification.
- Ket profile: `crates/ket-invite` (the access-code capability, HTTPS and
  loopback locators, and node-authenticated enrollment). Server issuance is in
  `crates/ket-server/src/invite.rs`, desktop enrollment in
  `crates/ket-client-core/src/invite.rs`.
- Android: the same Rust code over JNI. `crates/ket-invite-android` builds
  `libket_invite_jni.so` for every ABI; the app reads invites, validates stored node
  keys, checks enrollment signatures, and seals control requests only through it
  (`apps/ket-android/app/src/main/java/com/ket/android/invite/NativeInvites.kt`), so
  Android reports exactly the Rust error codes. There is no Kotlin port and no Tink.

## Client experience

- Desktop: the Add server panel takes an invite (masked, whitespace-stripped),
  a QR image (Open QR image), or a pasted screenshot. The native
  `inspect_invite` and `decode_invite_qr` commands decode (`crates/ket-qr`,
  adapted from Abyssal) and verify it without network access; images travel
  as raw bytes, are limited to 8 MiB PNG/JPEG, and are never kept. The saved
  profile keeps the node key (`ket.node-key`) for every restore.
- Android: paste, camera scan (ZXing + CameraX, same as Abyssal), or open a QR
  image. Text is verified before it fills the field and again on submit. The
  encrypted profile stores `node_public_key`.
- Server: `sudo ket invite` (wrapping `packaging/server/create-invite.sh`) issues invites with a terminal QR
  code; the installer uses it for the first invite.
- URL + access code entry stays under Advanced and never pins a node key.

## Conformance vectors

Fixed, non-production vectors live in
`crates/ket-invite/tests/vectors/invite-v1.json`. The Rust crate regenerates
them in its tests and fails on drift; the Android JVM tests replay the same
file through the JNI library built for the build machine.
