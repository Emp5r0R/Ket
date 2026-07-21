package com.ket.android

import androidx.activity.compose.BackHandler
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

private val KetInk = Color(0xFF071014)
internal val KetTeal = Color(0xFF28C7B7)
internal val KetMuted = Color(0xFF8EA5A7)
private val KetPanel = Color(0xFF11191D)
private val KetHealthy = Color(0xFF4BD29F)
private val KetDegraded = Color(0xFFF0B44D)
private val KetSaturated = Color(0xFFFF6B66)

@Composable
internal fun KetTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = KetTeal,
            onPrimary = KetInk,
            background = KetInk,
            surface = KetPanel,
            secondary = KetHealthy,
            error = KetSaturated,
            onBackground = Color.White,
            onSurface = Color.White,
        ),
    ) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background,
            content = content,
        )
    }
}

@Composable
internal fun KetApp(
    snapshot: TunnelSnapshot,
    onConnect: (String, String, KetProtocol?) -> Unit,
    onDisconnect: () -> Unit,
) {
    var serverUrl by rememberSaveable { mutableStateOf("") }
    var accessCode by rememberSaveable { mutableStateOf("") }
    var preferredProtocolWireName by rememberSaveable { mutableStateOf<String?>(null) }
    var guideProtocolWireName by rememberSaveable { mutableStateOf<String?>(null) }
    val preferredProtocol = preferredProtocolWireName?.let(KetProtocol::fromWireName)
    guideProtocolWireName?.let(KetProtocol::fromWireName)?.let { guideProtocol ->
        ProtocolLearnMorePage(
            initialProtocol = guideProtocol,
            onBack = { guideProtocolWireName = null },
        )
        return
    }
    val connected = snapshot.phase == TunnelPhase.Connected
    val busy = snapshot.phase in setOf(
        TunnelPhase.Enrolling,
        TunnelPhase.Connecting,
        TunnelPhase.Reconnecting,
        TunnelPhase.Stopping,
    )
    Scaffold(
        containerColor = KetInk,
        contentWindowInsets = WindowInsets.safeDrawing,
        bottomBar = {
            Surface(color = KetInk) {
                Button(
                    enabled = snapshot.phase != TunnelPhase.Stopping &&
                        (connected || busy || (serverUrl.isNotBlank() && accessCode.length == 32)),
                    onClick = {
                        if (connected || busy) {
                            onDisconnect()
                        } else {
                            onConnect(serverUrl, accessCode, preferredProtocol)
                        }
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .navigationBarsPadding()
                        .padding(horizontal = 20.dp, vertical = 12.dp)
                        .height(52.dp),
                ) {
                    if (busy && snapshot.phase != TunnelPhase.Stopping) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(18.dp),
                            color = MaterialTheme.colorScheme.onPrimary,
                            strokeWidth = 2.dp,
                        )
                        Spacer(Modifier.width(10.dp))
                    }
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
        },
    ) { contentPadding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(contentPadding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 18.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("KET", style = MaterialTheme.typography.labelLarge, color = KetTeal)
                PhaseStatus(snapshot.phase)
            }
            Text(phaseHeadline(snapshot.phase), style = MaterialTheme.typography.headlineMedium)
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
                ProtocolPreferenceSelector(
                    selected = preferredProtocol,
                    onSelect = { preferredProtocolWireName = it?.wireName },
                    onLearnMore = {
                        guideProtocolWireName = (preferredProtocol ?: KetProtocol.Stealth).wireName
                    },
                )
            }
            ServerMap(snapshot.node, connected)
            snapshot.node?.let { node ->
                NodeCapacity(node)
                Telemetry(snapshot, node)
            }
            if (snapshot.message.isNotBlank()) {
                Text(
                    snapshot.message,
                    color = if (snapshot.phase == TunnelPhase.Failed) KetSaturated else KetMuted,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}

@Composable
private fun ProtocolPreferenceSelector(
    selected: KetProtocol?,
    onSelect: (KetProtocol?) -> Unit,
    onLearnMore: () -> Unit,
) {
    var expanded by androidx.compose.runtime.remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("PREFERRED PROTOCOL", color = KetMuted, style = MaterialTheme.typography.labelSmall)
            TextButton(onClick = onLearnMore) { Text("Learn more") }
        }
        Box(Modifier.fillMaxWidth()) {
            OutlinedButton(
                onClick = { expanded = true },
                modifier = Modifier.fillMaxWidth().height(48.dp),
            ) {
                Text(selected?.displayName ?: "Automatic")
            }
            DropdownMenu(
                expanded = expanded,
                onDismissRequest = { expanded = false },
            ) {
                DropdownMenuItem(
                    text = { Text("Automatic") },
                    onClick = {
                        onSelect(null)
                        expanded = false
                    },
                )
                KetProtocol.entries.forEach { protocol ->
                    DropdownMenuItem(
                        text = { Text(protocol.displayName) },
                        onClick = {
                            onSelect(protocol)
                            expanded = false
                        },
                    )
                }
            }
        }
        Text(
            selected?.shortInstruction ?: AUTO_PROTOCOL_INSTRUCTION,
            color = KetMuted,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun ProtocolLearnMorePage(
    initialProtocol: KetProtocol,
    onBack: () -> Unit,
) {
    var selectedWireName by rememberSaveable { mutableStateOf(initialProtocol.wireName) }
    var menuExpanded by androidx.compose.runtime.remember { mutableStateOf(false) }
    val protocol = KetProtocol.fromWireName(selectedWireName) ?: initialProtocol
    BackHandler(onBack = onBack)
    Scaffold(
        containerColor = KetInk,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { contentPadding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(contentPadding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 14.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                TextButton(onClick = onBack) { Text("Back") }
                Text("PROTOCOL GUIDE", color = KetTeal, style = MaterialTheme.typography.labelLarge)
            }
            Box(Modifier.fillMaxWidth()) {
                OutlinedButton(
                    onClick = { menuExpanded = true },
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) {
                    Text(protocol.displayName)
                }
                DropdownMenu(
                    expanded = menuExpanded,
                    onDismissRequest = { menuExpanded = false },
                ) {
                    KetProtocol.entries.forEach { item ->
                        DropdownMenuItem(
                            text = { Text(item.displayName) },
                            onClick = {
                                selectedWireName = item.wireName
                                menuExpanded = false
                            },
                        )
                    }
                }
            }
            Text(protocol.shortInstruction, style = MaterialTheme.typography.titleMedium)
            ProtocolGuideSection("Best for", protocol.bestFor)
            ProtocolGuideSection("How it works", protocol.operation)
            ProtocolGuideList("Connect", protocol.steps)
            ProtocolGuideList("Limits", protocol.limitations)
        }
    }
}

@Composable
private fun ProtocolGuideSection(title: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
        HorizontalDivider(color = Color(0xFF243237))
        Text(title, color = KetTeal, style = MaterialTheme.typography.labelLarge)
        Text(body, color = KetMuted, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun ProtocolGuideList(title: String, items: List<String>) {
    Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
        HorizontalDivider(color = Color(0xFF243237))
        Text(title, color = KetTeal, style = MaterialTheme.typography.labelLarge)
        items.forEachIndexed { index, item ->
            Text("${index + 1}. $item", color = KetMuted, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

@Composable
private fun PhaseStatus(phase: TunnelPhase) {
    val tone by animateColorAsState(
        targetValue = when (phase) {
            TunnelPhase.Connected -> KetHealthy
            TunnelPhase.Failed -> KetSaturated
            TunnelPhase.Disconnected -> KetMuted
            else -> KetDegraded
        },
        label = "phase tone",
    )
    Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        Canvas(Modifier.size(9.dp)) { drawCircle(tone) }
        Text(
            when (phase) {
                TunnelPhase.Disconnected -> "Offline"
                TunnelPhase.Enrolling -> "Authorizing"
                TunnelPhase.Connecting -> "Connecting"
                TunnelPhase.Reconnecting -> "Recovering"
                TunnelPhase.Connected -> "Protected"
                TunnelPhase.Stopping -> "Stopping"
                TunnelPhase.Failed -> "Attention"
            },
            color = tone,
            style = MaterialTheme.typography.labelMedium,
        )
    }
}

private fun phaseHeadline(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Protected route"
    TunnelPhase.Enrolling, TunnelPhase.Connecting -> "Securing route"
    TunnelPhase.Reconnecting -> "Restoring route"
    TunnelPhase.Stopping -> "Closing route"
    TunnelPhase.Failed -> "Route needs attention"
    TunnelPhase.Disconnected -> "Choose your node"
}

@Composable
private fun ServerMap(node: AndroidNodeStatus?, connected: Boolean) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = KetPanel,
    ) {
        Column(
            Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("SERVER MAP", color = KetMuted, style = MaterialTheme.typography.labelSmall)
                Text(
                    node?.location?.countryCode ?: "--",
                    color = KetTeal,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
            KetMap(node?.location, connected)
            HorizontalDivider(color = Color(0xFF243237))
            Text(
                node?.location?.displayName ?: "No server selected",
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                node?.displayName ?: "Enter a server URL and access code",
                color = KetMuted,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun NodeCapacity(node: AndroidNodeStatus) {
    val tone by animateColorAsState(
        targetValue = when (node.health) {
            AndroidNodeHealth.Healthy -> KetHealthy
            AndroidNodeHealth.Degraded -> KetDegraded
            AndroidNodeHealth.Saturated -> KetSaturated
        },
        label = "node health tone",
    )
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(node.health.displayName, color = tone, style = MaterialTheme.typography.labelLarge)
            Text(
                "${node.activeSessions} / ${node.sessionCapacity} sessions",
                color = KetMuted,
                style = MaterialTheme.typography.labelMedium,
            )
        }
        LinearProgressIndicator(
            progress = { (node.capacityPercent / 100.0).toFloat() },
            modifier = Modifier.fillMaxWidth().height(5.dp),
            color = tone,
            trackColor = Color(0xFF263338),
        )
    }
}

@Composable
private fun Telemetry(snapshot: TunnelSnapshot, node: AndroidNodeStatus) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        MetricRow(
            "Download" to formatBytes(snapshot.receivedBytes),
            "Upload" to formatBytes(snapshot.sentBytes),
            "Handshake" to (snapshot.handshakeLatencyMs?.let { "$it ms" } ?: "--"),
        )
        HorizontalDivider(color = Color(0xFF1D292D))
        MetricRow(
            "Transport" to snapshot.transportName,
            "CPU" to (node.cpuLoadPercent?.let { "%.1f%%".format(it) } ?: "--"),
            "Uptime" to formatDuration(node.uptimeSeconds),
        )
        MetricRow(
            "Memory" to formatMemory(node),
            "Node load" to "${node.capacityPercent.toInt()}%",
            "Device flows" to snapshot.onlineConnections.toString(),
        )
    }
}

@Composable
private fun MetricRow(vararg metrics: Pair<String, String>) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        metrics.forEach { (label, value) ->
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(label, color = KetMuted, style = MaterialTheme.typography.labelSmall)
                Text(
                    value,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
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

private fun formatMemory(node: AndroidNodeStatus): String =
    if (node.memoryUsedBytes == null || node.memoryTotalBytes == null) {
        "--"
    } else {
        "${formatBytes(node.memoryUsedBytes)} / ${formatBytes(node.memoryTotalBytes)}"
    }

private fun formatDuration(seconds: Long?): String {
    if (seconds == null) return "--"
    val days = seconds / 86_400
    val hours = seconds % 86_400 / 3_600
    val minutes = seconds % 3_600 / 60
    return when {
        days > 0 -> "${days}d ${hours}h"
        hours > 0 -> "${hours}h ${minutes}m"
        else -> "${minutes}m"
    }
}
