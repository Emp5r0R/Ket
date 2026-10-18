package com.ket.android

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import androidx.core.content.edit
import org.json.JSONObject
import java.security.MessageDigest
import java.security.SecureRandom

/**
 * Remembers which transports worked on each network, so Automatic starts with the one that
 * worked there last time: the campus Wi-Fi that only lets REALITY through, the mobile network
 * where Hysteria2 is fastest. Networks are told apart by a salted hash of their kind, interface,
 * and gateway. The stored data holds only those hashes, a random salt, and transport outcomes:
 * no network name and no address.
 */
internal class NetworkTransportMemory(
    private val read: () -> String?,
    private val write: (String) -> Unit,
    private val clock: () -> Long = System::currentTimeMillis,
    private val random: SecureRandom = SecureRandom(),
) {
    @Synchronized
    fun load(network: String?): JSONObject? {
        network ?: return null
        val file = parse() ?: return null
        return file.optJSONObject(NETWORKS)
            ?.optJSONObject(key(file.getString(SALT), network))
            ?.optJSONObject(HISTORY)
    }

    @Synchronized
    fun save(network: String?, history: JSONObject) {
        network ?: return
        val file = parse() ?: JSONObject().put(SALT, newSalt()).put(NETWORKS, JSONObject())
        val networks = file.getJSONObject(NETWORKS)
        networks.put(
            key(file.getString(SALT), network),
            JSONObject().put(SEEN, clock()).put(HISTORY, history),
        )
        while (networks.length() > MAX_NETWORKS) {
            val oldest = networks.keys().asSequence().minByOrNull {
                networks.optJSONObject(it)?.optLong(SEEN, 0) ?: 0
            } ?: break
            networks.remove(oldest)
        }
        write(file.toString())
    }

    private fun parse(): JSONObject? = runCatching {
        JSONObject(read() ?: return null).takeIf {
            it.optString(SALT).length == SALT_HEX_LENGTH && it.optJSONObject(NETWORKS) != null
        }
    }.getOrNull()

    private fun newSalt(): String = ByteArray(SALT_HEX_LENGTH / 2).also(random::nextBytes).hex()

    private fun key(salt: String, network: String): String =
        MessageDigest.getInstance("SHA-256")
            .digest("$salt\u0000$network".toByteArray(Charsets.UTF_8))
            .copyOf(16)
            .hex()

    private fun ByteArray.hex(): String = joinToString("") { "%02x".format(it) }

    companion object {
        private const val SALT = "salt"
        private const val NETWORKS = "networks"
        private const val SEEN = "seen"
        private const val HISTORY = "history"
        private const val SALT_HEX_LENGTH = 64
        internal const val MAX_NETWORKS = 16
        private const val PREFERENCES = "ket_transport_memory"
        private const val KEY = "memory"

        fun forContext(context: Context): NetworkTransportMemory {
            val preferences = context.applicationContext.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            return NetworkTransportMemory(
                read = { preferences.getString(KEY, null) },
                write = { preferences.edit { putString(KEY, it) } },
            )
        }

        /**
         * The physical network under the VPN, described by its kind, interface, and gateway; the
         * caller hashes it. Null when the phone has no such network.
         */
        fun describe(connectivity: ConnectivityManager, network: Network?): String? {
            network ?: return null
            val capabilities = connectivity.getNetworkCapabilities(network) ?: return null
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) return null
            val kind = when {
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
                else -> "other"
            }
            val link: LinkProperties = connectivity.getLinkProperties(network) ?: return null
            val gateway = link.routes.firstOrNull { it.isDefaultRoute && it.gateway != null }
                ?.gateway?.hostAddress.orEmpty()
            return "$kind\n${link.interfaceName.orEmpty()}\n$gateway"
        }
    }
}
