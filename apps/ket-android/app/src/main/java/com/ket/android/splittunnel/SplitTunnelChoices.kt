package com.ket.android.splittunnel

/** Pure helpers behind the split-tunnel settings screen. */
object SplitTunnelChoices {
    fun search(apps: List<InstalledApp>, query: String): List<InstalledApp> {
        val needle = query.trim()
        if (needle.isEmpty()) return apps
        return apps.filter {
            it.label.contains(needle, ignoreCase = true) || it.packageName.contains(needle, ignoreCase = true)
        }
    }

    fun toggle(policy: SplitTunnelPolicy, packageName: String): SplitTunnelPolicy = policy.copy(
        packages = if (packageName in policy.packages) policy.packages - packageName else policy.packages + packageName,
    )

    /** One line that says plainly which traffic does not go through Ket. */
    fun summary(policy: SplitTunnelPolicy): String {
        val count = policy.normalized().packages.size
        val apps = if (count == 1) "1 chosen app" else "$count chosen apps"
        return when {
            policy.mode == SplitTunnelMode.AllApps -> "Every app uses Ket."
            count == 0 -> "Choose at least one app. Until then, every app uses Ket."
            policy.mode == SplitTunnelMode.OnlySelected -> "Only $apps use${if (count == 1) "s" else ""} Ket. Other apps connect directly."
            else -> "$apps connect${if (count == 1) "s" else ""} directly, without Ket."
        }
    }
}
