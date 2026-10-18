package com.ket.android.connection

import com.ket.android.KetProtocol

/**
 * Connection settings chosen on this device. They apply from the next
 * connection, including always-on starts.
 */
data class ConnectionOptions(
    /**
     * Send the TLS ClientHello of VLESS + REALITY and HTTPS Stealth in small
     * pieces, so a filter that reads the server name from one packet misses it.
     */
    val splitTlsHello: Boolean = false,
    /** Protocols Automatic leaves out; choosing one by hand still uses it. */
    val automaticExcluded: Set<KetProtocol> = emptySet(),
    /** Addresses and networks reached directly, outside the tunnel, in their stored form. */
    val bypass: List<String> = emptyList(),
)
