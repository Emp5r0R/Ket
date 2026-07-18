package com.ket.android

import org.json.JSONObject

internal object EngineConfig {
    fun hysteria(
        transport: HysteriaTransport,
        resolvedAddress: String,
        fdControlSocket: String,
        socksPort: Int,
    ): String {
        require(socksPort in 1..65535) { "SOCKS port is invalid" }
        require(resolvedAddress.isNotBlank()) { "Resolved server address is missing" }
        val serverHost = if (resolvedAddress.contains(':')) "[$resolvedAddress]" else resolvedAddress
        val quic = JSONObject()
            .put("maxIdleTimeout", "30s")
            .put("keepAlivePeriod", "10s")
            .put("sockopts", JSONObject().put("fdControlUnixSocket", "@$fdControlSocket"))
        val document = JSONObject()
            .put("server", "$serverHost:${transport.port}")
            .put("auth", transport.auth)
            .put("tls", JSONObject().put("sni", transport.tlsServerName))
            .put("quic", quic)
            .put("fastOpen", false)
            .put("lazy", false)
            .put("socks5", JSONObject().put("listen", "127.0.0.1:$socksPort").put("disableUDP", false))
        when (val obfs = transport.obfuscation) {
            HysteriaObfuscation.None -> Unit
            is HysteriaObfuscation.Salamander -> document.put(
                "obfs",
                JSONObject()
                    .put("type", "salamander")
                    .put("salamander", JSONObject().put("password", obfs.password)),
            )
            is HysteriaObfuscation.Gecko -> document.put(
                "obfs",
                JSONObject()
                    .put("type", "gecko")
                    .put(
                        "gecko",
                        JSONObject()
                            .put("password", obfs.password)
                            .put("minPacketSize", obfs.minimumPacketSize)
                            .put("maxPacketSize", obfs.maximumPacketSize),
                    ),
            )
        }
        return document.toString(2)
    }

    fun tunToSocks(socksPort: Int): String {
        require(socksPort in 1..65535) { "SOCKS port is invalid" }
        return """
            tunnel:
              mtu: 1400
              ipv4: 198.18.0.1
              ipv6: 'fc00::1'
              icmp: 'reply'
            socks5:
              address: '127.0.0.1'
              port: $socksPort
              udp: 'udp'
            misc:
              log-level: error
              connect-timeout: 10000
              tcp-read-write-timeout: 300000
              udp-read-write-timeout: 60000
        """.trimIndent() + "\n"
    }
}
