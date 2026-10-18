package com.ket.android.connection

import java.net.Inet6Address
import java.net.InetAddress

/**
 * An address or network kept outside the tunnel: [address] holds the network's bytes (host
 * bits cleared) and [length] its prefix length.
 */
class BypassPrefix private constructor(val address: ByteArray, val length: Int) {
    val isIpv4: Boolean get() = address.size == 4

    /** The network's address; IPv6 stays IPv6 even where it looks like IPv4 (`::ffff:0:0/96`). */
    fun inetAddress(): InetAddress = inetAddressOf(address)

    /** The stored form, the same as the desktop app's: `203.0.113.0/24`, `2001:db8::1`. */
    override fun toString(): String {
        val text = if (isIpv4) address.joinToString(".") { (it.toInt() and 0xff).toString() } else ipv6Text(address)
        return if (length == address.size * 8) text else "$text/$length"
    }

    override fun equals(other: Any?): Boolean =
        other is BypassPrefix && length == other.length && address.contentEquals(other.address)

    override fun hashCode(): Int = 31 * address.contentHashCode() + length

    companion object {
        /** The address alone: a /32 or /128. */
        fun host(address: InetAddress): BypassPrefix = of(address.address, address.address.size * 8)

        internal fun of(address: ByteArray, length: Int): BypassPrefix {
            val masked = address.copyOf()
            for (bit in length until masked.size * 8) {
                masked[bit / 8] = (masked[bit / 8].toInt() and (0x80 ushr (bit % 8)).inv()).toByte()
            }
            return BypassPrefix(masked, length)
        }
    }
}

/** Why an entry cannot be kept outside the tunnel; [code] matches the desktop app's. */
enum class BypassRefusal(val code: String) {
    Invalid("invalid"),
    TooWide("too_wide"),
    Special("special"),
    TooMany("too_many"),
}

sealed interface BypassCheck {
    data class Accepted(val prefix: BypassPrefix) : BypassCheck
    data class Refused(val refusal: BypassRefusal) : BypassCheck
}

/**
 * The rules for the bypass list, the same as the Rust contract the desktop app and tunnel service
 * use (`crates/ket-tunnel-protocol/src/bypass.rs`); both replay `bypass-entries.json`. Addresses
 * are parsed here, never handed to the system resolver, so checking an entry sends no DNS query.
 */
object BypassEntries {
    const val MAX_ENTRIES = 64
    private const val MIN_IPV4_PREFIX = 8
    private const val MIN_IPV6_PREFIX = 16

    fun check(text: String): BypassCheck {
        val trimmed = text.trim()
        val slash = trimmed.indexOf('/')
        val addressText = if (slash < 0) trimmed else trimmed.substring(0, slash)
        val address = parseIpv4(addressText) ?: parseIpv6(addressText)
            ?: return BypassCheck.Refused(BypassRefusal.Invalid)
        val bits = address.size * 8
        val length = if (slash < 0) {
            bits
        } else {
            val digits = trimmed.substring(slash + 1)
            if (digits.isEmpty() || !digits.all(Char::isAsciiDigit)) {
                return BypassCheck.Refused(BypassRefusal.Invalid)
            }
            digits.toIntOrNull()?.takeIf { it <= bits } ?: return BypassCheck.Refused(BypassRefusal.Invalid)
        }
        val prefix = BypassPrefix.of(address, length)
        return when {
            length < (if (prefix.isIpv4) MIN_IPV4_PREFIX else MIN_IPV6_PREFIX) ->
                BypassCheck.Refused(BypassRefusal.TooWide)
            isSpecial(prefix.address) -> BypassCheck.Refused(BypassRefusal.Special)
            else -> BypassCheck.Accepted(prefix)
        }
    }

    /** Saved entries that still pass, each once, at most [MAX_ENTRIES]. */
    fun valid(entries: Collection<String>): List<BypassPrefix> = entries
        .mapNotNull { (check(it) as? BypassCheck.Accepted)?.prefix }
        .distinct()
        .take(MAX_ENTRIES)

    private fun isSpecial(address: ByteArray): Boolean {
        val bytes = address.map { it.toInt() and 0xff }
        return if (address.size == 4) {
            bytes[0] == 127 || bytes[0] in 224..239 || bytes.all { it == 0 } || bytes.all { it == 255 }
        } else {
            bytes.all { it == 0 } ||
                (bytes.subList(0, 15).all { it == 0 } && bytes[15] == 1) ||
                bytes[0] == 0xff
        }
    }

    /** Four decimal parts of 0-255 without leading zeros, as Rust accepts them. */
    private fun parseIpv4(text: String): ByteArray? {
        val parts = text.split('.')
        if (parts.size != 4) return null
        return ByteArray(4) { index ->
            val part = parts[index]
            if (part.isEmpty() || part.length > 3 || !part.all(Char::isAsciiDigit)) return null
            if (part.length > 1 && part[0] == '0') return null
            part.toInt().takeIf { it <= 255 }?.toByte() ?: return null
        }
    }

    /** RFC 4291 text without a zone; an IPv4 address written as IPv6 is refused. */
    private fun parseIpv6(text: String): ByteArray? {
        if (text.isEmpty() || text.count { it == ':' } < 2 || text.contains(":::")) return null
        if (!text.all { it.isAsciiHexDigit() || it == ':' || it == '.' }) return null
        val halves = text.split("::")
        if (halves.size > 2) return null
        fun groups(part: String): List<Int>? {
            if (part.isEmpty()) return emptyList()
            val pieces = part.split(':')
            val result = mutableListOf<Int>()
            pieces.forEachIndexed { index, piece ->
                if (piece.contains('.')) {
                    if (index != pieces.lastIndex) return null
                    val v4 = parseIpv4(piece) ?: return null
                    result += ((v4[0].toInt() and 0xff) shl 8) or (v4[1].toInt() and 0xff)
                    result += ((v4[2].toInt() and 0xff) shl 8) or (v4[3].toInt() and 0xff)
                } else {
                    if (piece.isEmpty() || piece.length > 4) return null
                    result += piece.toInt(16)
                }
            }
            return result
        }
        val head = groups(halves[0]) ?: return null
        val tail = if (halves.size == 2) groups(halves[1]) ?: return null else emptyList()
        val all = if (halves.size == 2) {
            if (head.size + tail.size > 7) return null
            head + List(8 - head.size - tail.size) { 0 } + tail
        } else {
            head.takeIf { it.size == 8 } ?: return null
        }
        val bytes = ByteArray(16)
        all.forEachIndexed { index, group ->
            bytes[index * 2] = (group shr 8).toByte()
            bytes[index * 2 + 1] = group.toByte()
        }
        val mapped = bytes.copyOfRange(0, 10).all { it.toInt() == 0 } &&
            (bytes[10].toInt() and 0xff) == 0xff && (bytes[11].toInt() and 0xff) == 0xff
        return if (mapped) null else bytes
    }
}

/** RFC 5952: lowercase, no leading zeros, the longest run of two or more zero groups as `::`. */
private fun ipv6Text(address: ByteArray): String {
    val groups = List(8) { ((address[it * 2].toInt() and 0xff) shl 8) or (address[it * 2 + 1].toInt() and 0xff) }
    var bestStart = -1
    var bestLength = 0
    var index = 0
    while (index < 8) {
        if (groups[index] == 0) {
            val start = index
            while (index < 8 && groups[index] == 0) index++
            if (index - start > bestLength && index - start >= 2) {
                bestStart = start
                bestLength = index - start
            }
        } else {
            index++
        }
    }
    fun join(range: IntRange) = range.joinToString(":") { Integer.toHexString(groups[it]) }
    if (bestStart < 0) return join(0..7)
    return join(0 until bestStart) + "::" + join(bestStart + bestLength..7)
}

private fun Char.isAsciiDigit(): Boolean = this in '0'..'9'

private fun Char.isAsciiHexDigit(): Boolean = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'

internal fun inetAddressOf(bytes: ByteArray): InetAddress =
    if (bytes.size == 16) Inet6Address.getByAddress(null, bytes, null) else InetAddress.getByAddress(bytes)
