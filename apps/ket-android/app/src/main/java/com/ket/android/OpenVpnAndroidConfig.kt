package com.ket.android

internal object OpenVpnAndroidConfig {
    fun render(
        transport: OpenVpnStunnelTransport,
        carrierPort: Int,
        managementSocketPath: String,
    ): ByteArray {
        require(carrierPort in 1..65535) { "OpenVPN carrier port is invalid" }
        require(
                managementSocketPath.isNotEmpty() &&
                managementSocketPath.length <= 96 &&
                managementSocketPath.none { it.isWhitespace() || it == '\u0000' },
        ) { "OpenVPN management socket path is invalid" }
        val document = buildString {
            appendLine("client")
            appendLine("dev tun")
            appendLine("proto tcp-client")
            appendLine("remote 127.0.0.1 $carrierPort")
            appendLine("nobind")
            appendLine("connect-retry 2 5")
            appendLine("connect-retry-max 3")
            appendLine("connect-timeout 10")
            appendLine("resolv-retry 0")
            appendLine("remote-cert-tls server")
            appendLine("verify-x509-name ${transport.tlsServerName} name")
            appendLine("tls-version-min 1.2")
            appendLine("tls-cert-profile preferred")
            appendLine("data-ciphers AES-256-GCM:AES-128-GCM:CHACHA20-POLY1305")
            appendLine("data-ciphers-fallback AES-256-GCM")
            appendLine("auth SHA256")
            appendLine("allow-compression no")
            appendLine("auth-retry none")
            appendLine("redirect-gateway def1 bypass-dhcp")
            appendLine("block-ipv6")
            appendLine("persist-key")
            appendLine("management $managementSocketPath unix")
            appendLine("management-client")
            appendLine("management-query-passwords")
            appendLine("management-hold")
            appendLine("management-log-cache 50")
            appendLine("auth-user-pass")
            appendLine("verb 3")
            appendLine("mute 10")
            appendLine("<ca>")
            append(transport.caCertificate.trim())
            appendLine()
            appendLine("</ca>")
            appendLine("<tls-crypt>")
            append(transport.tlsCryptKey.trim())
            appendLine()
            appendLine("</tls-crypt>")
        }
        return document.toByteArray(Charsets.UTF_8)
    }
}
