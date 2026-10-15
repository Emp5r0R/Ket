package com.ket.android.invite

private const val TAG_HTTPS = 1UL
private const val TAG_LOOPBACK_DEVELOPMENT = 2UL
private const val LOCATOR_FIELDS = 3
private const val MAX_HOST_BYTES = 253
internal const val MAX_LOCATORS = 4

/** Suffixes that never name a public control endpoint. */
private val nonPublicSuffixes = listOf(
    "local", "localhost", "internal", "onion", "i2p", "home.arpa", "test", "example", "invalid",
)
private val labelPattern = Regex("^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$")

/** Where a client reaches the node's control API. Mirrors `ket-invite` locator.rs. */
sealed class ControlLocator {
    abstract val port: Int

    /** The control API base URL, always ending in `/`. */
    abstract val baseUrl: String

    data class Https(val host: String, override val port: Int) : ControlLocator() {
        override val baseUrl: String
            get() = if (port == 443) "https://$host/" else "https://$host:$port/"
    }

    data class LoopbackDevelopment(val host: LoopbackHost, override val port: Int) : ControlLocator() {
        override val baseUrl: String
            get() = if (host == LoopbackHost.Ipv6) "http://[::1]:$port/" else "http://${host.address}:$port/"
    }

    internal fun encode(encoder: CborEncoder) {
        encoder.array(LOCATOR_FIELDS)
        when (this) {
            is Https -> {
                encoder.uint(TAG_HTTPS)
                encoder.text(host)
            }
            is LoopbackDevelopment -> {
                encoder.uint(TAG_LOOPBACK_DEVELOPMENT)
                encoder.uint(host.tag)
            }
        }
        encoder.uint(port.toULong())
    }

    internal fun validate() {
        val valid = when (this) {
            is Https -> port in 1..65535 && isPublicDnsName(host)
            is LoopbackDevelopment -> port in 1..65535
        }
        ensure(valid, InviteFailure.UnsafeLocator)
    }

    internal companion object {
        fun decode(decoder: CborDecoder): ControlLocator {
            decoder.array(LOCATOR_FIELDS)
            val locator = when (decoder.uint()) {
                TAG_HTTPS -> Https(decoder.text(MAX_HOST_BYTES), decodePort(decoder))
                TAG_LOOPBACK_DEVELOPMENT -> LoopbackDevelopment(LoopbackHost.fromTag(decoder.uint()), decodePort(decoder))
                else -> fail(InviteFailure.UnsupportedLocator)
            }
            locator.validate()
            return locator
        }

        fun validateSet(locators: List<ControlLocator>) {
            ensure(locators.size in 1..MAX_LOCATORS, InviteFailure.Invalid)
            locators.forEach(ControlLocator::validate)
            ensure(locators.toSet().size == locators.size, InviteFailure.Invalid)
        }

        private fun decodePort(decoder: CborDecoder): Int {
            val port = decoder.uint()
            ensure(port <= 0xffffUL, InviteFailure.UnsafeLocator)
            return port.toInt()
        }

        /**
         * A canonical lowercase public DNS name: two or more labels of `a-z`, `0-9`, and inner
         * hyphens, an alphabetic top-level label (which excludes IPv4 literals), and no reserved
         * or private-use suffix.
         */
        fun isPublicDnsName(host: String): Boolean {
            if (host.isEmpty() || host.length > MAX_HOST_BYTES) return false
            val labels = host.split('.')
            return labels.size >= 2 &&
                labels.all(labelPattern::matches) &&
                labels.last().any { it in 'a'..'z' } &&
                nonPublicSuffixes.none { host == it || host.endsWith(".$it") }
        }
    }
}

enum class LoopbackHost(val address: String, internal val tag: ULong) {
    Localhost("localhost", 1UL),
    Ipv4("127.0.0.1", 2UL),
    Ipv6("::1", 3UL),
    AndroidEmulator("10.0.2.2", 4UL),
    ;

    internal companion object {
        fun fromTag(tag: ULong): LoopbackHost =
            entries.firstOrNull { it.tag == tag } ?: fail(InviteFailure.UnsafeLocator)
    }
}
