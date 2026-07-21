package com.ket.android

enum class KetProtocol(
    val wireName: String,
    val displayName: String,
    val shortInstruction: String,
    val bestFor: String,
    val operation: String,
    val steps: List<String>,
    val limitations: List<String>,
) {
    Stealth(
        "stealth",
        "HTTPS Stealth",
        "Use on strict networks that still allow normal HTTPS. Ket falls back if this path is unavailable.",
        "HTTP-only networks, filtered Wi-Fi, and Cloudflare-carried servers.",
        "Carries VLESS through XHTTP packet-up inside certificate-verified browser-shaped TLS.",
        listOf("Select HTTPS Stealth.", "Connect normally.", "Use Auto if the server CDN path is blocked."),
        listOf("Requires the server HTTPS route.", "Adds more overhead than a direct transport."),
    ),
    WireGuardTls(
        "wire_guard",
        "WireGuard TLS",
        "Use for a fast tunnel that resembles WebSocket HTTPS traffic. Automatic fallback remains active.",
        "Networks that allow WebSockets but identify native VPN handshakes.",
        "Carries WireGuard packets through a certificate-verified WebSocket/TLS transport.",
        listOf("Select WireGuard TLS.", "Connect and allow the initial route test.", "Use HTTPS Stealth if WebSockets are blocked."),
        listOf("Requires WebSocket support.", "Requires a compatible 64-bit Android build."),
    ),
    Hysteria2(
        "hysteria2",
        "Hysteria 2",
        "Use when UDP is available, especially on lossy or high-latency links.",
        "Mobile links, long-distance servers, and unstable networks that permit UDP.",
        "Uses QUIC congestion control with authenticated leases and optional packet obfuscation.",
        listOf("Select Hysteria 2.", "Connect while UDP 443 is reachable.", "Choose a TLS transport if UDP is blocked."),
        listOf("Cannot cross networks that block UDP.", "Obfuscation is not separate encryption."),
    ),
    Reality(
        "vless_xtls_reality",
        "VLESS + REALITY",
        "Use for a direct, low-overhead TCP route with a TLS-like handshake.",
        "Networks that allow direct TCP but probe ordinary proxy handshakes.",
        "Uses Xray VLESS with REALITY authentication and a browser TLS fingerprint.",
        listOf("Select VLESS + REALITY.", "Connect through the direct server endpoint.", "Use HTTPS Stealth when direct TCP is filtered."),
        listOf("Ordinary HTTP CDNs cannot carry it.", "The REALITY target must remain reachable."),
    ),
    Shadowsocks2022(
        "shadowsocks2022",
        "Shadowsocks 2022",
        "Use for a lightweight direct route when its assigned TCP/UDP port is reachable.",
        "Lower-overhead direct connections without aggressive active probing.",
        "Uses SIP022 AEAD-2022 keys and a lease-specific TCP/UDP server port.",
        listOf("Select Shadowsocks 2022.", "Connect through the assigned direct port.", "Use a TLS-shaped protocol under stronger filtering."),
        listOf("Requires a dedicated public port.", "Its wire shape can be easier to classify than HTTPS."),
    ),
    OpenVpnTls(
        "open_vpn_stunnel",
        "OpenVPN TLS",
        "Use for compatibility when modern transports fail; expect lower TCP performance.",
        "Compatibility-focused devices and networks that permit generic direct TLS.",
        "Runs authenticated OpenVPN inside a second certificate-pinned TLS layer.",
        listOf("Select OpenVPN TLS.", "Wait for both TLS handshakes.", "Prefer HTTPS Stealth on HTTP-only networks."),
        listOf("TCP inside TCP can reduce throughput.", "Ordinary HTTP CDNs cannot carry it."),
    );

    companion object {
        fun fromWireName(value: String): KetProtocol? = entries.firstOrNull { it.wireName == value }
    }
}

internal const val AUTO_PROTOCOL_INSTRUCTION =
    "Tests healthy server routes and changes protocol when the current network blocks one."
