package com.ket.android.splittunnel

import android.content.Context
import androidx.core.content.edit

/**
 * Keeps the split-tunnel choice on this device. App choices are not secrets,
 * so they live in ordinary private preferences, and the VPN service reads them
 * whenever it builds an interface (including always-on starts after a reboot).
 */
class SplitTunnelStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun load(): SplitTunnelPolicy = SplitTunnelPolicy(
        mode = SplitTunnelMode.fromWireName(preferences.getString(MODE_KEY, null)),
        packages = preferences.getStringSet(PACKAGES_KEY, emptySet()).orEmpty().toSet(),
    ).normalized()

    fun save(policy: SplitTunnelPolicy) {
        val normalized = policy.normalized()
        preferences.edit {
            putString(MODE_KEY, normalized.mode.wireName)
            putStringSet(PACKAGES_KEY, normalized.packages.toSet())
        }
    }

    private companion object {
        const val PREFERENCES = "ket_split_tunnel"
        const val MODE_KEY = "mode"
        const val PACKAGES_KEY = "packages"
    }
}
