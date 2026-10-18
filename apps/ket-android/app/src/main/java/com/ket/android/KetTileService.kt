package com.ket.android

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import androidx.core.content.ContextCompat

/** What tapping the Quick Settings tile does in each situation. */
internal enum class TileAction { Connect, Disconnect, OpenApp, Wait }

internal fun tileAction(phase: TunnelPhase, vpnPermitted: Boolean, profileSaved: Boolean): TileAction = when (phase) {
    TunnelPhase.Connected, TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting -> TileAction.Disconnect
    TunnelPhase.Stopping -> TileAction.Wait
    TunnelPhase.Disconnected, TunnelPhase.Failed ->
        if (vpnPermitted && profileSaved) TileAction.Connect else TileAction.OpenApp
}

internal fun tileSubtitle(snapshot: TunnelSnapshot): String = when (snapshot.phase) {
    TunnelPhase.Connected -> snapshot.transportName
    TunnelPhase.Enrolling, TunnelPhase.Connecting -> "Connecting"
    TunnelPhase.Reconnecting -> "Reconnecting"
    TunnelPhase.Stopping -> "Disconnecting"
    TunnelPhase.Failed -> "Not connected"
    TunnelPhase.Disconnected -> "Off"
}

/**
 * Connects and disconnects Ket from Quick Settings. Connecting uses the saved server, the same
 * way Android's always-on VPN starts Ket; without a saved server or VPN permission, the tile
 * opens the app instead.
 */
class KetTileService : TileService() {
    private var subscription: AutoCloseable? = null

    override fun onStartListening() {
        subscription = KetTunnelRuntime.subscribe { snapshot -> qsTile?.let { show(it, snapshot) } }
    }

    override fun onStopListening() {
        subscription?.close()
        subscription = null
    }

    override fun onClick() {
        val profileSaved = runCatching { AndroidTunnelCredentialStore.get(this).load() != null }.getOrDefault(false)
        when (tileAction(KetTunnelRuntime.snapshot().phase, VpnService.prepare(this) == null, profileSaved)) {
            TileAction.Disconnect -> KetTunnelController.disconnect(this)
            TileAction.Connect -> ContextCompat.startForegroundService(
                this,
                Intent(this, KetVpnService::class.java).setAction(KetVpnService.ACTION_START),
            )
            TileAction.OpenApp -> openApp()
            TileAction.Wait -> Unit
        }
    }

    private fun show(tile: Tile, snapshot: TunnelSnapshot) {
        tile.state = if (snapshot.phase == TunnelPhase.Connected) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.label = "Ket"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) tile.subtitle = tileSubtitle(snapshot)
        tile.updateTile()
    }

    private fun openApp() {
        val intent = Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(
                PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
            )
        } else {
            openAppBeforeAndroid14(intent)
        }
    }

    /** Before Android 14 the PendingIntent form does not exist; this one is safe there. */
    @SuppressLint("StartActivityAndCollapseDeprecated")
    @Suppress("DEPRECATION")
    private fun openAppBeforeAndroid14(intent: Intent) = startActivityAndCollapse(intent)
}
