package com.ket.android

import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor

/** Owns the Android VPN permission and TUN lifecycle; the Rust transport engine plugs in here. */
class KetVpnService : VpnService() {
    private var tun: ParcelFileDescriptor? = null
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        tun?.close()
        tun = Builder().setSession("Ket").addAddress("10.8.0.2", 32).addRoute("0.0.0.0", 0).establish()
        return if (tun == null) START_NOT_STICKY else START_STICKY
    }
    override fun onRevoke() { tun?.close(); tun = null; stopSelf(); super.onRevoke() }
    override fun onDestroy() { tun?.close(); tun = null; super.onDestroy() }
}
