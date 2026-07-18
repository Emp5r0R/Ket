package com.ket.android

import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.Canvas
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.StrokeCap
import java.util.concurrent.Executors

private val Ink = Color(0xFF071014); private val Teal = Color(0xFF28C7B7); private val Muted = Color(0xFF8EA5A7)

class MainActivity : ComponentActivity() {
    private val executor = Executors.newSingleThreadExecutor()
    private val handler = Handler(Looper.getMainLooper()); private var endpoint = ""; private var token = ""
    @Volatile private var telemetryActive = false
    private var telemetrySink: ((String) -> Unit)? = null
    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        // Only start the tunnel after the system approval callback succeeds.
        if (it.resultCode == RESULT_OK && telemetryActive) {
            startService(Intent(this, KetVpnService::class.java))
        } else if (telemetryActive) {
            disconnect()
        }
    }
    override fun onCreate(savedInstanceState: Bundle?) { super.onCreate(savedInstanceState); setContent { KetTheme { KetApp(::enrollAndStart, ::disconnect) { telemetrySink = it } } } }
    private fun enrollAndStart(serverUrl: String, accessCode: String, done: (String, String, String) -> Unit) {
        endpoint = serverUrl
        executor.execute { try { val result = KetControlApi.enroll(serverUrl, accessCode, "Ket Android"); token = result.token; telemetryActive = true; runOnUiThread { done(result.node, result.country, "Connected - awaiting telemetry"); startVpn(); scheduleTelemetry() } } catch (error: Exception) { runOnUiThread { done("Enrollment failed", error.message ?: "Check the server URL", "") } } }
    }
    private fun scheduleTelemetry() {
        if (!telemetryActive) return
        handler.removeCallbacksAndMessages(null)
        handler.postDelayed({
            if (telemetryActive) {
                executor.execute {
                    try {
                        KetControlApi.renew(endpoint, token)
                        val telemetry = KetControlApi.status(endpoint, token)
                        runOnUiThread {
                            telemetrySink?.invoke(
                                "Capacity ${telemetry.capacity.toInt()}% | up ${telemetry.sent} B | down ${telemetry.received} B | ${telemetry.online} online",
                            )
                        }
                    } catch (_: Exception) {
                        // The next poll retries transient control-plane failures.
                    }
                    scheduleTelemetry()
                }
            }
        }, 10_000)
    }
    private fun disconnect() { telemetryActive = false; handler.removeCallbacksAndMessages(null); stopService(Intent(this, KetVpnService::class.java)); val oldToken = token; executor.execute { if (oldToken.isNotEmpty()) runCatching { KetControlApi.release(endpoint, oldToken) }; token = "" } }
    override fun onDestroy() { disconnect(); executor.shutdownNow(); super.onDestroy() }
    private fun startVpn() {
        val intent = VpnService.prepare(this)
        if (intent != null) vpnPermission.launch(intent)
        else startService(Intent(this, KetVpnService::class.java))
    }
}

@Composable private fun KetTheme(content: @Composable () -> Unit) { MaterialTheme(colorScheme = darkColorScheme(primary = Teal, background = Ink, surface = Color(0xFF101D21), onBackground = Color.White, onSurface = Color.White), content = content) }

@Composable private fun KetApp(onConnect: (String, String, (String, String, String) -> Unit) -> Unit, onDisconnect: () -> Unit, onTelemetry: (((String) -> Unit) -> Unit)) {
    var connected by remember { mutableStateOf(false) }
    var serverUrl by remember { mutableStateOf("") }
    var accessCode by remember { mutableStateOf("") }
    var nodeName by remember { mutableStateOf("No server selected") }
    var country by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("") }
    LaunchedEffect(Unit) { onTelemetry { status = it } }
    Column(Modifier.fillMaxSize().background(Ink).padding(24.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
        Text("KET", style = MaterialTheme.typography.labelLarge, color = Teal)
        Text(if (connected) "Protected route" else "Choose your node", style = MaterialTheme.typography.headlineMedium)
        if (!connected) {
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
        Surface(Modifier.fillMaxWidth().height(240.dp), shape = RoundedCornerShape(24.dp), color = Color(0xFF10262A)) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.SpaceBetween) {
                Text("SERVER MAP", color = Muted, style = MaterialTheme.typography.labelSmall)
                KetMap(connected)
                Text(if (connected) "$country · $nodeName" else nodeName, style = MaterialTheme.typography.titleLarge)
                Text(if (status.isBlank()) "Health  --     Capacity  --" else status, color = Muted)
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) { Text("Hysteria 2", color = Muted); Text("-- ms", color = Muted) }
        Button(
            enabled = connected || (serverUrl.isNotBlank() && accessCode.length == 32),
            onClick = {
                if (connected) {
                    connected = false
                    onDisconnect()
                } else {
                    status = "Enrolling..."
                    onConnect(serverUrl, accessCode) { node, location, message ->
                        nodeName = node
                        country = location
                        connected = node != "Enrollment failed"
                        status = message
                    }
                }
            },
            modifier = Modifier.fillMaxWidth().height(56.dp),
        ) { Text(if (connected) "Disconnect" else "Connect") }
    }
}

@Composable private fun KetMap(connected: Boolean) {
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
