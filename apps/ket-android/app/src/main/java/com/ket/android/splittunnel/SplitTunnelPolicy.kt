package com.ket.android.splittunnel

/** Which apps send their traffic through Ket. */
enum class SplitTunnelMode(val wireName: String, val displayName: String) {
    AllApps("all", "All apps"),
    OnlySelected("only", "Only chosen apps"),
    ExceptSelected("except", "All except chosen apps"),
    ;

    companion object {
        fun fromWireName(value: String?): SplitTunnelMode =
            entries.firstOrNull { it.wireName == value } ?: AllApps
    }
}

/** The user's choice: a mode and the apps it names. */
data class SplitTunnelPolicy(
    val mode: SplitTunnelMode = SplitTunnelMode.AllApps,
    val packages: Set<String> = emptySet(),
) {
    /** Drops anything that is not an Android package name. */
    fun normalized(): SplitTunnelPolicy = copy(packages = packages.filterTo(sortedSetOf(), ::isPackageName))

    companion object {
        val AllApps = SplitTunnelPolicy()

        // Android package names: two or more dot-separated Java identifiers.
        private val packageName = Regex("^[A-Za-z][A-Za-z0-9_]*(\\.[A-Za-z][A-Za-z0-9_]*)+$")

        fun isPackageName(value: String): Boolean = value.length <= 255 && packageName.matches(value)
    }
}

/** The per-app rules of an Android VPN interface builder. */
interface VpnAppRules {
    fun allow(packageName: String)

    fun disallow(packageName: String)
}

/** What a VPN interface was actually built with. */
data class AppliedSplitTunnel(
    val mode: SplitTunnelMode,
    val appCount: Int,
    /** Set when the chosen mode could not be honored and every app uses Ket. */
    val fallbackReason: String? = null,
)

/**
 * Turns a policy into VPN builder rules.
 *
 * Android allows either an allow list or a deny list on one interface, never
 * both. Ket itself is always inside the VPN: in "only chosen apps" it is added
 * to the allow list, and it is never put on the deny list, because Ket's own
 * route check and lease renewals must travel through the tunnel they verify.
 * Apps that are no longer installed are skipped. When nothing is left to
 * choose, every app uses Ket, the protective default.
 */
object SplitTunnelRules {
    fun apply(
        policy: SplitTunnelPolicy,
        ownPackage: String,
        isInstalled: (String) -> Boolean,
        rules: VpnAppRules,
    ): AppliedSplitTunnel {
        val chosen = policy.normalized().packages.filter { it != ownPackage && isInstalled(it) }
        return when (policy.mode) {
            SplitTunnelMode.AllApps -> AppliedSplitTunnel(SplitTunnelMode.AllApps, 0)
            SplitTunnelMode.OnlySelected -> {
                if (chosen.isEmpty()) {
                    return AppliedSplitTunnel(SplitTunnelMode.AllApps, 0, "no chosen app is installed")
                }
                rules.allow(ownPackage)
                chosen.forEach(rules::allow)
                AppliedSplitTunnel(SplitTunnelMode.OnlySelected, chosen.size)
            }
            SplitTunnelMode.ExceptSelected -> {
                if (chosen.isEmpty()) {
                    return AppliedSplitTunnel(SplitTunnelMode.AllApps, 0, "no chosen app is installed")
                }
                chosen.forEach(rules::disallow)
                AppliedSplitTunnel(SplitTunnelMode.ExceptSelected, chosen.size)
            }
        }
    }
}
