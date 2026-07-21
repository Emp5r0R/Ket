package com.ket.android

import org.json.JSONObject

internal object EngineConfig {
    fun wireGuard(transport: WireGuardTlsTransport, wireGuardPort: Int, socksPort: Int): String {
        require(wireGuardPort in 1..65535) { "WireGuard port is invalid" }
        require(socksPort in 1..65535) { "SOCKS port is invalid" }
        return JSONObject()
            .put("log", JSONObject().put("loglevel", "warning"))
            .put(
                "inbounds",
                org.json.JSONArray().put(
                    JSONObject()
                        .put("tag", "ket-socks")
                        .put("listen", "127.0.0.1")
                        .put("port", socksPort)
                        .put("protocol", "socks")
                        .put("settings", JSONObject().put("auth", "noauth").put("udp", true))
                        .put(
                            "sniffing",
                            JSONObject()
                                .put("enabled", true)
                                .put("destOverride", org.json.JSONArray(listOf("http", "tls", "quic")))
                                .put("routeOnly", true),
                        ),
                ),
            )
            .put(
                "outbounds",
                org.json.JSONArray().put(
                    JSONObject()
                        .put("tag", "ket-wireguard")
                        .put("protocol", "wireguard")
                        .put(
                            "settings",
                            JSONObject()
                                .put("secretKey", transport.privateKey)
                                .put("address", org.json.JSONArray(listOf("${transport.clientAddress}/32")))
                                .put("noKernelTun", true)
                                .put("mtu", 1280)
                                .put("domainStrategy", "ForceIP")
                                .put(
                                    "peers",
                                    org.json.JSONArray().put(
                                        JSONObject()
                                            .put("publicKey", transport.serverPublicKey)
                                            .put("preSharedKey", transport.presharedKey)
                                            .put("endpoint", "127.0.0.1:$wireGuardPort")
                                            .put("allowedIPs", org.json.JSONArray(listOf("0.0.0.0/0")))
                                            .put("keepAlive", 25),
                                    ),
                                ),
                        ),
                ),
            )
            .toString(2)
    }

    fun shadowsocks(
        transport: ShadowsocksTransport,
        resolvedAddress: String,
        socksPort: Int,
    ): String {
        require(socksPort in 1..65535) { "SOCKS port is invalid" }
        require(resolvedAddress.isNotBlank()) { "Resolved server address is missing" }
        return JSONObject()
            .put("server", resolvedAddress)
            .put("server_port", transport.port)
            .put("local_address", "127.0.0.1")
            .put("local_port", socksPort)
            .put("password", transport.key)
            .put("method", SHADOWSOCKS_METHOD)
            .put("mode", "tcp_and_udp")
            .put("timeout", 300)
            .put("udp_timeout", 300)
            .put("no_delay", true)
            .put("keep_alive", 30)
            .toString(2)
    }

    fun xray(transport: AndroidXrayTransport, resolvedAddress: String, socksPort: Int): String {
        require(socksPort in 1..65535) { "SOCKS port is invalid" }
        require(resolvedAddress.isNotBlank()) { "Resolved server address is missing" }
        val user = JSONObject()
            .put("id", transport.userId)
            .put("encryption", "none")
        val tag: String
        val streamSettings: JSONObject
        when (transport) {
            is RealityTransport -> {
                tag = "ket-reality"
                user.put("flow", "xtls-rprx-vision")
                streamSettings = JSONObject()
                    .put("network", "raw")
                    .put("security", "reality")
                    .put(
                        "realitySettings",
                        JSONObject()
                            .put("show", false)
                            .put("fingerprint", transport.fingerprint)
                            .put("serverName", transport.tlsServerName)
                            .put("password", transport.password)
                            .put("shortId", transport.shortId)
                            .put("spiderX", "/"),
                    )
            }
            is StealthTransport -> {
                tag = "ket-stealth"
                streamSettings = JSONObject()
                    .put("network", "xhttp")
                    .put("security", "tls")
                    .put(
                        "tlsSettings",
                        JSONObject()
                            .put("fingerprint", transport.fingerprint)
                            .put("serverName", transport.tlsServerName)
                            .put("alpn", org.json.JSONArray(listOf("h2", "http/1.1"))),
                    )
                    .put(
                        "xhttpSettings",
                        JSONObject()
                            .put("host", transport.tlsServerName)
                            .put("path", transport.path)
                            .put("mode", "packet-up"),
                    )
            }
            else -> throw IllegalArgumentException("Unsupported Xray transport")
        }
        val document = JSONObject()
            .put("log", JSONObject().put("loglevel", "warning"))
            .put(
                "inbounds",
                org.json.JSONArray().put(
                    JSONObject()
                        .put("tag", "ket-socks")
                        .put("listen", "127.0.0.1")
                        .put("port", socksPort)
                        .put("protocol", "socks")
                        .put("settings", JSONObject().put("auth", "noauth").put("udp", true))
                        .put(
                            "sniffing",
                            JSONObject()
                                .put("enabled", true)
                                .put("destOverride", org.json.JSONArray(listOf("http", "tls", "quic")))
                                .put("routeOnly", true),
                        ),
                ),
            )
            .put(
                "outbounds",
                org.json.JSONArray().put(
                    JSONObject()
                        .put("tag", tag)
                        .put("protocol", "vless")
                        .put(
                            "settings",
                            JSONObject().put(
                                "vnext",
                                org.json.JSONArray().put(
                                    JSONObject()
                                        .put("address", resolvedAddress)
                                        .put("port", transport.port)
                                        .put(
                                            "users",
                                            org.json.JSONArray().put(user),
                                        ),
                                ),
                            ),
                        )
                        .put("streamSettings", streamSettings),
                ),
            )
        return document.toString(2)
    }

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
