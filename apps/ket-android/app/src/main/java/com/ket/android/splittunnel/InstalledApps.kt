package com.ket.android.splittunnel

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.content.pm.ResolveInfo
import android.os.Build

/** An app the user can choose for split tunneling. */
data class InstalledApp(val packageName: String, val label: String)

/**
 * Lists apps that have a launcher entry, which is what people recognize and
 * what the manifest's <queries> entry makes visible without broad package
 * visibility. Ket itself is left out: it always uses the VPN.
 */
object InstalledApps {
    fun launchable(context: Context): List<InstalledApp> {
        val packageManager = context.packageManager
        val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        return queryActivities(packageManager, launcher)
            .map { it.activityInfo.applicationInfo }
            .filter { it.packageName != context.packageName }
            .distinctBy { it.packageName }
            .map { InstalledApp(it.packageName, packageManager.getApplicationLabel(it).toString()) }
            .sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.label })
    }

    fun isInstalled(context: Context, packageName: String): Boolean = try {
        applicationInfo(context.packageManager, packageName)
        true
    } catch (_: PackageManager.NameNotFoundException) {
        false
    }

    private fun queryActivities(packageManager: PackageManager, intent: Intent): List<ResolveInfo> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            packageManager.queryIntentActivities(intent, PackageManager.ResolveInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            packageManager.queryIntentActivities(intent, 0)
        }

    private fun applicationInfo(packageManager: PackageManager, packageName: String): ApplicationInfo =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            packageManager.getApplicationInfo(packageName, PackageManager.ApplicationInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            packageManager.getApplicationInfo(packageName, 0)
        }
}
