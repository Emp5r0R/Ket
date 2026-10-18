package com.ket.android

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import com.ket.android.connection.ConnectionOptions
import com.ket.android.invite.NodePublicKey

/** What a diagnostic report says about the phone and its server, never which network it is on. */
internal data class DiagnosticEnvironment(
    /** Such as `Android 14 (API 34)`. */
    val system: String = "unknown",
    /** `wifi`, `cellular`, `ethernet`, `other`, `none`, or `unknown`. */
    val network: String = "unknown",
    /** `sealed`, `sealed_if_offered`, or `plain`; see [controlMode]. */
    val control: String = "unknown",
    val automaticSkips: List<String> = emptyList(),
) {
    companion object {
        fun current(context: Context, options: ConnectionOptions): DiagnosticEnvironment {
            val connectivity = context.getSystemService(ConnectivityManager::class.java)
            val profile = runCatching { AndroidTunnelCredentialStore.get(context).load()?.profile }.getOrNull()
            return DiagnosticEnvironment(
                system = "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
                network = connectivity?.let(AndroidNetworkKind::current) ?: "unknown",
                control = profile?.let { controlMode(it.nodeKey) } ?: "unknown",
                automaticSkips = options.automaticExcluded.map(KetProtocol::wireName).sorted(),
            )
        }

        /** How control requests travel for a profile pinned to [nodeKey] (or none). */
        fun controlMode(nodeKey: NodePublicKey?): String = when {
            nodeKey == null -> "plain"
            nodeKey.sealedControl -> "sealed"
            else -> "sealed_if_offered"
        }
    }
}

internal object AndroidNetworkKind {
    fun of(capabilities: NetworkCapabilities): String = when {
        capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
        capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
        capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
        else -> "other"
    }

    /** The kind of physical network the phone uses now, under the VPN, preferring a validated one. */
    fun current(connectivity: ConnectivityManager): String {
        // Every network, not only the default, because while Ket is connected the default is Ket's own VPN.
        @Suppress("DEPRECATION")
        val physical = connectivity.allNetworks
            .mapNotNull(connectivity::getNetworkCapabilities)
            .filter {
                !it.hasTransport(NetworkCapabilities.TRANSPORT_VPN) &&
                    it.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            }
        val chosen = physical.firstOrNull { it.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) }
            ?: physical.firstOrNull()
            ?: return "none"
        return of(chosen)
    }
}
