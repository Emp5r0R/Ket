# Ket architecture

## Product boundary

Ket separates orchestration from packet transport. The Rust control plane owns node identity, access grants, bounded session leases, location data, health telemetry, and transport discovery. Protocol-specific data planes are replaceable adapters and must use maintained upstream implementations; Ket will not invent cryptography or reimplement mature tunnel protocols.

This separation keeps the API and user experience consistent while allowing a node to offer different transports for different network conditions.

## Target components

| Component | Responsibility | Technology | Current state |
| --- | --- | --- | --- |
| `ket-core` | Shared API models, protocol identifiers, secret primitives | Rust | Implemented |
| `ket-server` | Control API, access grants, sessions, telemetry | Rust/Axum | Implemented |
| Data-plane control | Scoped credentials, engine configuration, auth, traffic, health, kicks | Rust | Hysteria2, VLESS + REALITY, XHTTP/TLS Stealth, Shadowsocks 2022, WireGuard TLS, and OpenVPN/stunnel implemented |
| Transport engines | VLESS Reality, VLESS XHTTP/TLS, Hysteria2, Shadowsocks 2022, WireGuard over WebSocket/TLS, OpenVPN over stunnel, future maintained adapters | Maintained upstream engines | Hysteria2 2.10, Xray-core 26.3.27, shadowsocks-rust 1.24.0, Linux WireGuard, wstunnel 10.6.2, OpenVPN 2.7.5, and stunnel 5.79 integrated |
| Desktop client core | Node enrollment, strategy selection, tunnel lifecycle, metrics | Rust | Six implemented transports with bounded fallback |
| Desktop privilege broker | Authenticated TUN/route ownership and engine supervision | Rust system service | Implemented for Linux/Windows |
| Linux/Windows desktop | Map-first connection UI and native packaging | Tauri 2 plus shared Rust core | UI, service installers, and unsigned lifecycle gates implemented; signed bundles pending |
| Android | `VpnService`, map-first Compose UI, shared contracts | Kotlin/Compose, Natural Earth, Android Keystore, Hysteria2, Xray, shadowsocks-rust, wstunnel, OpenVPN 2, hev-socks5-tunnel | Six-transport implementation; current arm64 Hysteria2/REALITY packet flow verified; OpenVPN/WireGuard TLS/Shadowsocks/XHTTP restricted-network, corrected handover, Doze/revoke, API 26, DNS leak, always-on/reboot, and owner-signing physical gates pending |

## Control flow

1. An operator creates one or more 32-character access grants through the admin API.
2. A client sends its server URL, access code, and local device label to `POST /v1/sessions`.
3. The server validates the Argon2 hash, global node capacity, grant expiry, and per-grant connection limit.
4. The client receives a short-lived control bearer, node location/health, and configured transport profiles. Implemented transports also include a separate data-plane credential.
5. Hysteria2 submits its scoped credential to Ket's HTTP authentication backend. VLESS + REALITY and XHTTP/TLS Stealth receive the same deterministic lease-scoped UUID, which Ket atomically installs in every configured Xray inbound through the private Handler API before returning the manifest. Shadowsocks receives a deterministic 32-byte SIP022 key and stable TCP+UDP port. WireGuard TLS receives deterministic client/private preshared keys and an address. OpenVPN receives a scoped username/password plus authenticated CA and `tls-crypt` material. Only each lease's crash-safe resource slot is persisted.
6. Clients renew the lease while connected and release it on disconnect. Release, grant revocation, and the expiry reaper reject future authentication and remove or kick the session in every configured data plane. Ket reconciles persisted active leases with Xray, the Shadowsocks manager, the WireGuard agent, and the OpenVPN agent at startup before reporting ready.
7. On desktop, the unprivileged Tauri process sends validated transport requests to a loopback-only system service. The service authenticates each connection and supervises the selected upstream process group. SOCKS-based engines use the shared `tun2proxy` full-route and virtual-DNS bridge; OpenVPN owns its native TUN and default route through a certificate-verifying local stunnel carrier. Every path bypasses all pre-resolved server IPs to prevent recursion.
8. On Android, a flagged app start consumes the in-memory launch request while an unflagged always-on start restores a Keystore-sealed session manifest. Ket renews and resumes a surviving lease, re-enrolls from the sealed access grant only after confirmed authorization loss, and retains the session when the control endpoint is temporarily blocked. `VpnService` then resolves and excludes every advertised data-plane endpoint before it attempts ranked transports, and Hysteria additionally protects its QUIC descriptor. Ket attaches hev-socks5-tunnel with bridge-local mapped DNS to the Android-owned TUN only after a SOCKS path check succeeds, then requires certificate-verified HTTPS through the full route before publishing connected state. If an established engine exits, repeated HTTPS renewal proves the routed path unhealthy, or the underlying network changes, Android retains the lease and a fail-closed TUN guard while it rebuilds the route against ranked alternatives with bounded cooldown and retries.

## Security invariants

- Access codes are exactly 32 ASCII alphanumeric characters. Only a lookup prefix and an Argon2 hash are persisted.
- Session bearer tokens use the same split lookup/hash pattern and are never persisted in plaintext.
- Data-plane tokens share only the public session lookup ID with the control token. Their independent high-entropy secrets use BLAKE2 hashes and constant-time verification on the handshake hot path.
- Client-side secret values redact diagnostics and zero their allocations on drop. Protocol secret options live inside the credential object rather than public transport metadata.
- The admin token must be independent, at least 32 characters, and is compared in constant time.
- State replacement is atomic and the state file is mode `0600` on Unix. State loading is size-bounded and fails closed on unknown schemas, duplicate identities, orphan sessions, malformed records, impossible grant/session lifetimes, or password hashes outside Ket's exact Argon2id cost profile.
- Mutations are serialized and persisted before becoming visible in memory.
- Request bodies are capped at 16 KiB, requests time out, and Argon2 concurrency plus pending work are bounded. Saturation fails fast with a retryable `429` response instead of building an unbounded secret-processing queue.
- Server startup structurally parses the public URL and bounds every emitted node/location/transport field, including 32-profile maximum, identifier and display text, host and TLS names, and option maps; invalid operator configuration fails before listeners or data-plane runtime files are created.
- Docker runs the control plane as an unprivileged user with all Linux capabilities dropped and a read-only root filesystem.
- The Hysteria2 container is isolated from persistent control state, runs as UID `10001`, has a read-only root filesystem, and receives no Linux capabilities.
- The Xray container has the same UID, read-only root, and capability restrictions. It can only read the mode-`0600` generated runtime configuration, while its gRPC control API remains private to the Compose network. XHTTP's plaintext origin is published only to host loopback for the authenticated Cloudflare Tunnel process.
- The Shadowsocks manager runs as UID `10001` with a read-only root filesystem, no Linux capabilities, and an ACL that rejects private and special destinations. Its unauthenticated UDP manager API is reachable only from the dedicated private Compose network; public exposure is limited to the configured per-session TCP+UDP port range.
- The WireGuard manager runs as root with only `NET_ADMIN`, a read-only filesystem, a private bearer-authenticated API, fixed validated command arguments, private-key input through child stdin, destination and SMTP filtering, and no control-state volume. Its wstunnel origin is a separate unprivileged container bound only to host loopback for cloudflared.
- The OpenVPN agent runs as root with only `NET_ADMIN` and `/dev/net/tun`, a read-only filesystem, fixed validated command arguments, private bearer-authenticated management, private and SMTP destination filtering, and no control-state volume. stunnel runs separately with every capability dropped, and the control-plane authentication callback requires an independent internal bearer token.
- Hysteria2 rejects loopback, link-local, private, multicast, carrier-grade NAT, and outbound SMTP destinations to reduce lateral-movement, metadata-service, and spam abuse.
- Xray rejects private, loopback, link-local, multicast, carrier-grade NAT, BitTorrent, and outbound SMTP destinations for the same abuse boundary.
- The client accepts plaintext control HTTP only on loopback by default, refuses redirects and system proxies, requires TLS 1.2 or newer for HTTPS, caps response bodies, and sanitizes server errors before UI delivery.
- The Rust desktop core validates and bounds every enrollment manifest before storing it, binds refreshed status to the session ID encoded in the active token and the enrolled device name, requires a live lease plus valid node/traffic telemetry, and retains the last known-good snapshot when a response fails validation.
- Desktop transport credentials exist only in memory and an ephemeral mode-`0600` configuration that is deleted after engine and route readiness.
- Desktop Hysteria, Reality, XHTTP Stealth, Shadowsocks, and WireGuard TLS require the supervised bridge's virtual-DNS mode; the bridge captures IPv4 and IPv6, while each engine connects to pre-resolved server or CDN IPs outside those routes. OpenVPN instead owns a native full-route TUN, pins both certificate chains, and installs host routes for every resolved stunnel endpoint through the pre-tunnel gateway.
- Android caps control responses at 128 KiB, requires strict UTF-8 and exact HTTP results, validates token/lease/session/client identity plus traffic consistency, and bounds transport counts, identifiers, and credentials before state or engine configuration. It also rejects unknown transport options and downgrade-shaped TLS fields, pins and excludes data-plane endpoint addresses before routing traffic, keeps DNS inside hev's mapped-DNS bridge so IPv4-only servers receive domains, never enables application bypass, additionally protects Hysteria's QUIC socket, requires end-to-end HTTPS before publishing connected state, and deletes its mode-`0600` engine configuration after SOCKS readiness.
- Android persists only an AES-256-GCM authenticated record under `noBackupFilesDir`; the encryption key is generated and retained by Android Keystore, all debug representations redact credentials, and a failed atomic save releases the newly created server lease.
- Android native inputs are reproducible: Hysteria, Xray, Shadowsocks, and wstunnel executables plus the complete hev source archive are version- and checksum-pinned, and CI verifies the expected payload matrix.
- Desktop broker connections require a fresh challenge response using a 256-bit per-installation token. Protocol frames are bounded, credential buffers are zeroized, and debug output redacts proofs and tunnel IDs.
- The privileged broker allows one full-route tunnel and stops orphaned engine processes when the desktop heartbeat lease expires.

## Transport strategy

Clients use a policy engine rather than a hard-coded default. The implemented selector ranks configured transports using operator priority, explicit user preference, recent latency, consecutive failures, and bounded exponential cooldown. Automatic fallback has bounded attempts and never silently downgrades certificate or server-key verification. Desktop connection intent survives a round in which every transport is blocked: maintenance preserves the lease and retries on later ticks, while an explicit pause suppresses reconnect. A deterministic dual-transport censorship regression covers that boundary. Packet-loss sampling will be added with the desktop diagnostics surface.

`Stealth` is now a concrete VLESS + XHTTP `packet-up` adapter with certificate-verified TLS at the public CDN edge. It is deliberately not called Proton Stealth: Ket uses Xray's maintained, self-hostable protocol and makes no wire-compatibility claim. The server accepts plaintext XHTTP only on its loopback-published origin because Cloudflare Tunnel owns the public TLS hop.

`Shadowsocks2022` uses maintained shadowsocks-rust 1.24.0 rather than Xray's deprecated Shadowsocks implementation. Ket fixes the method to `2022-blake3-aes-256-gcm`, allocates a distinct port/key pair to every active lease, reconciles the complete port pool after a restart, and removes the server on expiry or revocation. The upstream manager exposes only an aggregate per-port byte counter, so Ket does not present that value as directional session traffic; control-plane traffic fields remain unavailable when Shadowsocks is the only reporting data plane.

`WireGuard` is a concrete WireGuard-over-WebSocket/TLS adapter. The server uses Linux WireGuard behind an authenticated manager and loopback-only wstunnel origin; desktop and Android use Xray's userspace WireGuard outbound behind a certificate-verifying wstunnel client. It is independently self-hosted and deliberately makes no Proton Stealth compatibility claim. Android support is currently arm64 API 28+ because that is the available upstream wstunnel payload.

`OpenVpnStunnel` is a concrete desktop/server/Android adapter. OpenVPN TCP runs through certificate-verified stunnel-compatible TLS, then independently verifies its own CA and `tls-crypt` key. The authenticated manager exposes directional counters and immediate revocation. Android runs the maintained OpenVPN 2 executable as a separate process, answers its private Unix management channel, passes a `VpnService` TUN descriptor with `SCM_RIGHTS`, and keeps route ownership in Ket.

`Ikev2` remains a discovery identifier rather than an executable adapter. A future implementation must use a maintained engine while preserving lease-scoped credentials, live revocation, accounting, and platform parity.

`XorScrambled` is represented only as an obfuscation layer. It is not encryption and must wrap an authenticated encrypted transport. Hysteria2 can retain HTTP/3 masquerading or enable Salamander/Gecko packet obfuscation; the client must receive the exact configured mode and password. CDN or reverse-proxy compatibility is transport-specific and cannot be assumed for UDP.

## Client parity

Desktop and Android consume the same versioned control contract. The Rust desktop core implements probing, bounded fallback, reconnecting, and the full lifecycle snapshot. Android implements disconnected, enrolling, connecting, reconnecting, connected, stopping, and failed states with ranked multi-transport startup fallback and bounded post-connect recovery. Both surfaces now plot the server's real coordinates on Natural Earth geography and expose node health, capacity, CPU, memory, uptime, handshake latency, selected protocol, sessions, and transfer metrics; both reject invalid geographic and telemetry values and bind refreshed status identity to the active lease before publishing it.
