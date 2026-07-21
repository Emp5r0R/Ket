package com.ket.android

import android.Manifest
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.mutableStateOf
import androidx.core.content.ContextCompat

private data class PendingConnectionRequest(
    val serverUrl: String,
    val accessCode: String,
    val preferredProtocol: KetProtocol?,
)

private object PendingPermissionRequest {
    var serverUrl: String? = null
    var accessCode: String? = null
    var preferredProtocol: KetProtocol? = null

    fun clear(): PendingConnectionRequest? {
        val server = serverUrl
        val code = accessCode
        val protocol = preferredProtocol
        serverUrl = null
        accessCode = null
        preferredProtocol = null
        return if (server != null && code != null) PendingConnectionRequest(server, code, protocol) else null
    }
}

class MainActivity : ComponentActivity() {
    private val tunnelState = mutableStateOf(KetTunnelRuntime.snapshot())
    private var subscription: AutoCloseable? = null
    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val request = PendingPermissionRequest.clear()
        if (result.resultCode == RESULT_OK && request != null) {
            KetTunnelController.connect(
                this,
                request.serverUrl,
                request.accessCode,
                request.preferredProtocol,
            )
        } else if (request != null) {
            KetTunnelRuntime.publish(
                TunnelSnapshot(phase = TunnelPhase.Failed, message = "Android VPN permission was not granted"),
            )
        }
    }
    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        requestVpnPermission()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        subscription = KetTunnelRuntime.subscribe { snapshot ->
            runOnUiThread { tunnelState.value = snapshot }
        }
        setContent {
            KetTheme {
                KetApp(
                    snapshot = tunnelState.value,
                    onConnect = ::requestConnection,
                    onDisconnect = { KetTunnelController.disconnect(this) },
                )
            }
        }
    }

    override fun onDestroy() {
        subscription?.close()
        subscription = null
        super.onDestroy()
    }

    private fun requestConnection(
        serverUrl: String,
        accessCode: String,
        preferredProtocol: KetProtocol?,
    ) {
        PendingPermissionRequest.serverUrl = serverUrl
        PendingPermissionRequest.accessCode = accessCode
        PendingPermissionRequest.preferredProtocol = preferredProtocol
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else {
            requestVpnPermission()
        }
    }

    private fun requestVpnPermission() {
        val permission = VpnService.prepare(this)
        if (permission == null) {
            PendingPermissionRequest.clear()?.let {
                KetTunnelController.connect(this, it.serverUrl, it.accessCode, it.preferredProtocol)
            }
        } else {
            vpnPermission.launch(permission)
        }
    }
}
