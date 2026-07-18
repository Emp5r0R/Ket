package com.ket.android

import android.Manifest
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp

private val Ink = Color(0xFF071014)
private val Teal = Color(0xFF28C7B7)
private val Muted = Color(0xFF8EA5A7)

private object PendingPermissionRequest {
    var serverUrl: String? = null
    var accessCode: String? = null

    fun clear(): Pair<String, String>? {
        val server = serverUrl
        val code = accessCode
        serverUrl = null
        accessCode = null
        return if (server != null && code != null) server to code else null
    }
}

class MainActivity : ComponentActivity() {
    private val tunnelState = mutableStateOf(KetTunnelRuntime.snapshot())
    private var subscription: AutoCloseable? = null
    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val request = PendingPermissionRequest.clear()
        if (result.resultCode == RESULT_OK && request != null) {
            KetTunnelController.connect(this, request.first, request.second)
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

    private fun requestConnection(serverUrl: String, accessCode: String) {
        PendingPermissionRequest.serverUrl = serverUrl
        PendingPermissionRequest.accessCode = accessCode
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
            PendingPermissionRequest.clear()?.let { KetTunnelController.connect(this, it.first, it.second) }
        } else {
            vpnPermission.launch(permission)
        }
    }
}

@Composable
private fun KetTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Teal,
            background = Ink,
            surface = Color(0xFF101D21),
            onBackground = Color.White,
            onSurface = Color.White,
        ),
        content = content,
    )
}

@Composable
private fun KetApp(
    snapshot: TunnelSnapshot,
    onConnect: (String, String) -> Unit,
    onDisconnect: () -> Unit,
) {
    var serverUrl by rememberSaveable { mutableStateOf("") }
    var accessCode by rememberSaveable { mutableStateOf("") }
    val connected = snapshot.phase == TunnelPhase.Connected
    val busy = snapshot.phase in setOf(TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Stopping)
    val hasNode = snapshot.node != "No server selected"
    Column(
        Modifier.fillMaxSize().background(Ink).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        Text("KET", style = MaterialTheme.typography.labelLarge, color = Teal)
        Text(
            when (snapshot.phase) {
                TunnelPhase.Connected -> "Protected route"
                TunnelPhase.Enrolling, TunnelPhase.Connecting -> "Securing route"
                TunnelPhase.Stopping -> "Closing route"
                else -> "Choose your node"
            },
            style = MaterialTheme.typography.headlineMedium,
        )
        if (!connected && !busy) {
            OutlinedTextField(
                value = serverUrl,
                onValueChange = { serverUrl = it },
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Server URL") },
                singleLine = true,
            )
            OutlinedTextField(
                value = accessCode,
                onValueChange = { accessCode = it },
                modifier = Modifier.fillMaxWidth(),
                label = { Text("32-character access code") },
                visualTransformation = PasswordVisualTransformation(),
                singleLine = true,
            )
        }
        Surface(
            Modifier.fillMaxWidth().height(240.dp),
            shape = RoundedCornerShape(8.dp),
            color = Color(0xFF10262A),
        ) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.SpaceBetween) {
                Text("SERVER MAP", color = Muted, style = MaterialTheme.typography.labelSmall)
                KetMap(connected)
                Text(
                    if (hasNode) listOf(snapshot.country, snapshot.node).filter(String::isNotBlank).joinToString(" · ") else snapshot.node,
                    style = MaterialTheme.typography.titleLarge,
                )
                Text(snapshot.message.ifBlank { "Health  --     Capacity  --" }, color = Muted)
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("Hysteria 2", color = Muted)
            Text(snapshot.handshakeLatencyMs?.let { "$it ms" } ?: "-- ms", color = Muted)
        }
        if (connected) {
            Text(
                "Capacity ${snapshot.capacityPercent.toInt()}% | up ${formatBytes(snapshot.sentBytes)} | down ${formatBytes(snapshot.receivedBytes)} | ${snapshot.onlineConnections} online",
                color = Muted,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        Button(
            enabled = snapshot.phase != TunnelPhase.Stopping &&
                (connected || busy || (serverUrl.isNotBlank() && accessCode.length == 32)),
            onClick = {
                if (connected || busy) onDisconnect() else onConnect(serverUrl, accessCode)
            },
            modifier = Modifier.fillMaxWidth().height(56.dp),
        ) {
            Text(
                when {
                    snapshot.phase == TunnelPhase.Stopping -> "Disconnecting"
                    connected -> "Disconnect"
                    busy -> "Cancel"
                    else -> "Connect"
                },
            )
        }
    }
}

private fun formatBytes(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val units = arrayOf("KB", "MB", "GB", "TB")
    var value = bytes.toDouble()
    var unit = -1
    while (value >= 1024 && unit < units.lastIndex) {
        value /= 1024
        unit += 1
    }
    return "%.1f %s".format(value, units[unit])
}

@Composable
private fun KetMap(connected: Boolean) {
    Canvas(Modifier.fillMaxWidth().height(100.dp)) {
        val grid = Color(0xFF214047)
        for (x in 0..8) drawLine(grid, Offset(size.width * x / 8f, 0f), Offset(size.width * x / 8f, size.height), 1f)
        for (y in 0..4) drawLine(grid, Offset(0f, size.height * y / 4f), Offset(size.width, size.height * y / 4f), 1f)
        val land = Color(0xFF2B6263)
        drawLine(land, Offset(size.width * .12f, size.height * .55f), Offset(size.width * .35f, size.height * .32f), 12f, StrokeCap.Round)
        drawLine(land, Offset(size.width * .35f, size.height * .32f), Offset(size.width * .58f, size.height * .52f), 16f, StrokeCap.Round)
        drawLine(land, Offset(size.width * .58f, size.height * .52f), Offset(size.width * .82f, size.height * .35f), 11f, StrokeCap.Round)
        drawCircle(Teal, radius = if (connected) 9f else 6f, center = Offset(size.width * .58f, size.height * .52f))
        if (connected) drawCircle(Teal.copy(alpha = .25f), radius = 22f, center = Offset(size.width * .58f, size.height * .52f))
    }
}
