# Ket architecture

## Product boundary

Ket separates orchestration from packet transport. The Rust control plane owns node identity, access grants, bounded session leases, location data, health telemetry, and transport discovery. Protocol-specific data planes are replaceable adapters and must use maintained upstream implementations; Ket will not invent cryptography or reimplement mature tunnel protocols.

This separation keeps the API and user experience consistent while allowing a node to offer different transports for different network conditions.

## Target components

| Component | Responsibility | Technology | Current state |
| --- | --- | --- | --- |
| `ket-core` | Shared API models, protocol identifiers, secret primitives | Rust | Implemented |
| `ket-server` | Control API, access grants, sessions, telemetry | Rust/Axum | Implemented |
| Data-plane control | Scoped credentials, engine configuration, auth, traffic, health, kicks | Rust | Hysteria2 and VLESS + REALITY implemented |
| Transport engines | VLESS Reality, Hysteria2, Shadowsocks 2022, OpenVPN/stunnel, IKEv2 | Maintained upstream engines | Hysteria2 2.10 and Xray-core 26.3.27 integrated server/client-side |
| Desktop client core | Node enrollment, strategy selection, tunnel lifecycle, metrics | Rust | Hysteria2 and VLESS + REALITY implemented |
| Desktop privilege broker | Authenticated TUN/route ownership and engine supervision | Rust system service | Implemented for Linux/Windows |
| Linux/Windows desktop | Map-first connection UI and native packaging | Tauri 2 plus shared Rust core | UI, service installers, and unsigned lifecycle gates implemented; signed bundles pending |
| Android | `VpnService`, map-first Compose UI, shared contracts | Kotlin/Compose, Hysteria2, Xray, hev-socks5-tunnel | Current arm64 packet flow, fallback, recovery, cancellation, and disconnect verified; corrected handover, Doze/revoke, API 26, DNS leak, and owner-signing gates pending |

## Control flow

1. An operator creates one or more 32-character access grants through the admin API.
2. A client sends its server URL, access code, and local device label to `POST /v1/sessions`.
3. The server validates the Argon2 hash, global node capacity, grant expiry, and per-grant connection limit.
4. The client receives a short-lived control bearer, node location/health, and configured transport profiles. Implemented transports also include a separate data-plane credential.
5. Hysteria2 submits its scoped credential to Ket's HTTP authentication backend. VLESS + REALITY instead receives a deterministic lease-scoped UUID that Ket installs through Xray's private Handler API before returning the manifest.
6. Clients renew the lease while connected and release it on disconnect. Release, grant revocation, and the expiry reaper reject future authentication and remove or kick the session in every configured data plane. Ket reconciles persisted active leases with Xray at startup.
7. On desktop, the unprivileged Tauri process sends validated transport requests to a loopback-only system service. The service authenticates each connection before it owns Hysteria TUN mode or the Xray/`tun2proxy` process pair and their routes.
8. On Android, `VpnService` resolves and excludes every advertised data-plane endpoint before it attempts ranked transports, and Hysteria additionally protects its QUIC descriptor. Ket attaches hev-socks5-tunnel to the Android-owned TUN only after a SOCKS path check succeeds. If an established engine exits, repeated HTTPS renewal proves the routed path unhealthy, or the underlying network changes, Android retains the lease and a fail-closed TUN guard while it rebuilds the route against ranked alternatives with bounded cooldown and retries.

## Security invariants

- Access codes are exactly 32 ASCII alphanumeric characters. Only a lookup prefix and an Argon2 hash are persisted.
- Session bearer tokens use the same split lookup/hash pattern and are never persisted in plaintext.
- Data-plane tokens share only the public session lookup ID with the control token. Their independent high-entropy secrets use BLAKE2 hashes and constant-time verification on the handshake hot path.
- Client-side secret values redact diagnostics and zero their allocations on drop. Protocol secret options live inside the credential object rather than public transport metadata.
- The admin token must be independent, at least 32 characters, and is compared in constant time.
- State replacement is atomic and the state file is mode `0600` on Unix.
- Mutations are serialized and persisted before becoming visible in memory.
- Request bodies are capped at 16 KiB, requests time out, and Argon2 concurrency is bounded.
- Docker runs the control plane as an unprivileged user with all Linux capabilities dropped and a read-only root filesystem.
- The Hysteria2 container is isolated from persistent control state, runs as UID `10001`, has a read-only root filesystem, and receives no Linux capabilities.
- The Xray container has the same UID, read-only root, and capability restrictions. It can only read the mode-`0600` generated runtime configuration, while its gRPC control API remains private to the Compose network.
- Hysteria2 rejects loopback, link-local, private, multicast, carrier-grade NAT, and outbound SMTP destinations to reduce lateral-movement, metadata-service, and spam abuse.
- Xray rejects private, loopback, link-local, multicast, carrier-grade NAT, BitTorrent, and outbound SMTP destinations for the same abuse boundary.
- The client accepts plaintext control HTTP only on loopback by default, refuses redirects and system proxies, requires TLS 1.2 or newer for HTTPS, caps response bodies, and sanitizes server errors before UI delivery.
- Desktop transport credentials exist only in memory and an ephemeral mode-`0600` configuration that is deleted after engine and route readiness.
- Android rejects unknown transport options and downgrade-shaped TLS fields, resolves and excludes every data-plane endpoint before routing traffic, additionally protects Hysteria's QUIC socket, and deletes its mode-`0600` engine configuration after SOCKS readiness.
- Android native inputs are reproducible: Hysteria and Xray executables plus the complete hev source archive are version- and checksum-pinned, and CI verifies the expected payload matrix.
- Desktop broker connections require a fresh challenge response using a 256-bit per-installation token. Protocol frames are bounded, credential buffers are zeroized, and debug output redacts proofs and tunnel IDs.
- The privileged broker allows one full-route tunnel and stops orphaned engine processes when the desktop heartbeat lease expires.

## Transport strategy

Clients use a policy engine rather than a hard-coded default. The implemented selector ranks configured transports using operator priority, explicit user preference, recent latency, consecutive failures, and bounded exponential cooldown. Automatic fallback has bounded attempts and never silently downgrades certificate or server-key verification. Packet-loss sampling will be added with the desktop diagnostics surface.

`Shadowsocks2022` remains a discovery identifier rather than an executable adapter. The pinned Xray 26.3.27 engine warns that its Shadowsocks implementation is deprecated and may be removed, so Ket will not bind a new production transport to that lifecycle. A future implementation must use a maintained engine while preserving lease-scoped credentials, live revocation, per-session accounting, and Android/Desktop parity.

`XorScrambled` is represented only as an obfuscation layer. It is not encryption and must wrap an authenticated encrypted transport. Hysteria2 can retain HTTP/3 masquerading or enable Salamander/Gecko packet obfuscation; the client must receive the exact configured mode and password. CDN or reverse-proxy compatibility is transport-specific and cannot be assumed for UDP.

## Client parity

Desktop and Android consume the same versioned control contract. The Rust desktop core implements probing, bounded fallback, reconnecting, and the full lifecycle snapshot. Android implements disconnected, enrolling, connecting, reconnecting, connected, stopping, and failed states with ranked multi-transport startup fallback, bounded post-connect recovery, node, capacity, handshake latency, selected protocol, and local traffic metrics.
