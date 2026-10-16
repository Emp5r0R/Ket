package com.ket.android.connection

import android.content.Context
import androidx.core.content.edit

/**
 * Keeps the connection settings on this device. They are not secrets, so they
 * live in ordinary private preferences, and the VPN service reads them each
 * time it starts a transport.
 */
class ConnectionOptionsStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun load(): ConnectionOptions = ConnectionOptions(
        splitTlsHello = preferences.getBoolean(SPLIT_TLS_HELLO_KEY, false),
    )

    fun save(options: ConnectionOptions) {
        preferences.edit { putBoolean(SPLIT_TLS_HELLO_KEY, options.splitTlsHello) }
    }

    private companion object {
        const val PREFERENCES = "ket_connection_options"
        const val SPLIT_TLS_HELLO_KEY = "split_tls_hello"
    }
}
