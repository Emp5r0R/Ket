# Ket architecture

## Product boundary

Ket separates orchestration from packet transport. The Rust control plane owns node identity, access grants, bounded session leases, location data, health telemetry, and transport discovery. Protocol-specific data planes are replaceable adapters and must use maintained upstream implementations; Ket will not invent cryptography or reimplement mature tunnel protocols.

This separation keeps the API and user experience consistent while allowing a node to offer different transports for different network conditions.

## Target components

| Component | Responsibility | Technology | Current state |
| --- | --- | --- | --- |
| `ket-core` | Shared API models, protocol identifiers, secret primitives | Rust | Implemented |
| `ket-server` | Control API, access grants, sessions, telemetry | Rust/Axum | Implemented |
| Data-plane control | Scoped credentials, engine configuration, auth, traffic, health, kicks | Rust | Hysteria2 implemented |
| Transport engines | VLESS Reality, Hysteria2, Shadowsocks 2022, OpenVPN/stunnel, IKEv2 | Maintained upstream engines | Hysteria2 2.10 integrated |
| Desktop client core | Node enrollment, strategy selection, tunnel lifecycle, metrics | Rust | Implemented for Hysteria2 |
| Desktop privilege broker | Authenticated TUN/route ownership and engine supervision | Rust system service | Implemented for Linux/Windows |
| Linux/Windows desktop | Map-first connection UI and native packaging | Tauri 2 plus shared Rust core | UI and service installers implemented; signed bundles pending |
| Android | `VpnService`, map-first Compose UI, shared contracts | Kotlin/Compose, Hysteria2, hev-socks5-tunnel | Multi-ABI Hysteria2 data plane implemented; physical-device verification pending |

## Control flow

1. An operator creates one or more 32-character access grants through the admin API.
2. A client sends its server URL, access code, and local device label to `POST /v1/sessions`.
3. The server validates the Argon2 hash, global node capacity, grant expiry, and per-grant connection limit.
4. The client receives a short-lived control bearer, node location/health, and configured transport profiles. Implemented transports also include a separate data-plane credential.
5. Hysteria2 submits that scoped credential to Ket's HTTP authentication backend. Ket returns the session ID used by Hysteria's traffic and online APIs.
6. Clients renew the lease while connected and release it on disconnect. Release, grant revocation, and the expiry reaper reject future authentication and ask Hysteria to kick the active session ID.
7. On desktop, the unprivileged Tauri process sends validated transport requests to a loopback-only system service. The service authenticates each connection before it owns the Hysteria process, TUN interface, and routes.
8. On Android, `VpnService` starts Hysteria in local SOCKS5 mode, protects its QUIC descriptor through Hysteria's FD-control protocol, and attaches hev-socks5-tunnel to the Android-owned TUN only after transport readiness.

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
- Hysteria2 rejects loopback, link-local, private, multicast, carrier-grade NAT, and outbound SMTP destinations to reduce lateral-movement, metadata-service, and spam abuse.
- The client accepts plaintext control HTTP only on loopback by default, refuses redirects and system proxies, requires TLS 1.2 or newer for HTTPS, caps response bodies, and sanitizes server errors before UI delivery.
- Desktop Hysteria credentials exist only in memory and an ephemeral mode-`0600` configuration that is deleted after the engine reports both server connection and TUN readiness.
- Android rejects unknown Hysteria profile options and downgrade-shaped TLS fields, resolves the server before routing traffic, protects only the QUIC socket from the VPN, and deletes its mode-`0600` engine configuration after SOCKS readiness.
- Android native inputs are reproducible: Hysteria executables and the complete hev source archive are version- and checksum-pinned, and CI verifies all three native payloads for four ABIs.
- Desktop broker connections require a fresh challenge response using a 256-bit per-installation token. Protocol frames are bounded, credential buffers are zeroized, and debug output redacts proofs and tunnel IDs.
- The privileged broker allows one full-route tunnel and stops orphaned engine processes when the desktop heartbeat lease expires.

## Transport strategy

Clients use a policy engine rather than a hard-coded default. The implemented selector ranks configured transports using operator priority, explicit user preference, recent latency, consecutive failures, and bounded exponential cooldown. Automatic fallback has bounded attempts and never silently downgrades certificate or server-key verification. Packet-loss sampling will be added with the desktop diagnostics surface.

`XorScrambled` is represented only as an obfuscation layer. It is not encryption and must wrap an authenticated encrypted transport. Hysteria2 can retain HTTP/3 masquerading or enable Salamander/Gecko packet obfuscation; the client must receive the exact configured mode and password. CDN or reverse-proxy compatibility is transport-specific and cannot be assumed for UDP.

## Client parity

Desktop and Android consume the same versioned control contract. The Rust desktop core implements probing, bounded fallback, reconnecting, and the full lifecycle snapshot. Android currently implements disconnected, enrolling, connecting, connected, stopping, and failed states with node, capacity, handshake latency, and local traffic metrics. Android automatic multi-transport fallback and reconnect policy remain pending behind its platform adapter.
