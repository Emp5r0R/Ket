package com.ket.android.connection

import android.content.Context
import androidx.core.content.edit
import com.ket.android.KetProtocol

/**
 * Keeps the connection settings on this device. They are not secrets, so they
 * live in ordinary private preferences, and the VPN service reads them each
 * time it starts a transport.
 */
class ConnectionOptionsStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun load(): ConnectionOptions = ConnectionOptions(
        splitTlsHello = preferences.getBoolean(SPLIT_TLS_HELLO_KEY, false),
        automaticExcluded = preferences.getStringSet(AUTOMATIC_EXCLUDED_KEY, emptySet())
            .orEmpty()
            .mapNotNull(KetProtocol::fromWireName)
            .toSet(),
        bypass = BypassEntries.valid(
            preferences.getString(BYPASS_KEY, null).orEmpty().lines().filter(String::isNotBlank),
        ).map(BypassPrefix::toString),
    )

    fun save(options: ConnectionOptions) {
        preferences.edit {
            putBoolean(SPLIT_TLS_HELLO_KEY, options.splitTlsHello)
            putStringSet(AUTOMATIC_EXCLUDED_KEY, options.automaticExcluded.map { it.wireName }.toSet())
            // One entry per line keeps the order the person built the list in.
            if (options.bypass.isEmpty()) remove(BYPASS_KEY) else putString(BYPASS_KEY, options.bypass.joinToString("\n"))
        }
    }

    private companion object {
        const val PREFERENCES = "ket_connection_options"
        const val SPLIT_TLS_HELLO_KEY = "split_tls_hello"
        const val AUTOMATIC_EXCLUDED_KEY = "automatic_excluded"
        const val BYPASS_KEY = "bypass"
    }
}
