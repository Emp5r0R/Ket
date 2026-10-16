package com.ket.android.ui.metrics

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Analytics
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ket.android.AndroidNodeStatus
import com.ket.android.TunnelPhase
import com.ket.android.TunnelSnapshot
import com.ket.android.formatBytes
import com.ket.android.formatDuration
import com.ket.android.formatSetupTime
import com.ket.android.ui.common.EmptyState
import com.ket.android.ui.common.InfoRow
import com.ket.android.ui.common.Pill
import com.ket.android.ui.common.ScreenTitle
import com.ket.android.ui.common.SectionCard
import com.ket.android.ui.healthTone

@Composable
internal fun MetricsScreen(snapshot: TunnelSnapshot, accessRemainingSeconds: Long?) {
    Column(verticalArrangement = Arrangement.spacedBy(18.dp)) {
        ScreenTitle("Metrics", "This connection and the server behind it")
        val node = snapshot.node
        if (node == null) {
            EmptyState(
                Icons.Filled.Analytics,
                "Nothing to measure yet",
                "Connect to your server to see traffic, speed of setup, and how busy the server is.",
            )
            return@Column
        }
        val connected = snapshot.phase == TunnelPhase.Connected
        SectionCard(title = "This connection") {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                TrafficFigure(Icons.Filled.ArrowDownward, "Downloaded", formatBytes(snapshot.receivedBytes), Modifier.weight(1f))
                TrafficFigure(Icons.Filled.ArrowUpward, "Uploaded", formatBytes(snapshot.sentBytes), Modifier.weight(1f))
            }
            InfoRow("Protocol", if (connected) snapshot.transportName else "Not connected")
            InfoRow("Set up in", formatSetupTime(snapshot.handshakeLatencyMs))
            InfoRow("Access left", formatDuration(accessRemainingSeconds))
        }
        ServerCard(node)
    }
}

@Composable
private fun TrafficFigure(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, value: String, modifier: Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.height(16.dp))
            Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
        }
        Text(value, style = MaterialTheme.typography.headlineSmall)
    }
}

@Composable
private fun ServerCard(node: AndroidNodeStatus) {
    val tone = healthTone(node.health)
    SectionCard(title = "Server") {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(node.displayName, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
            Pill(node.health.displayName, tone)
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth()) {
                Text("Load", Modifier.weight(1f), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
                Text("${node.activeSessions} of ${node.sessionCapacity} devices", style = MaterialTheme.typography.bodyMedium)
            }
            LinearProgressIndicator(
                progress = { (node.capacityPercent / 100.0).toFloat().coerceIn(0f, 1f) },
                modifier = Modifier.fillMaxWidth().height(6.dp),
                color = tone,
                trackColor = MaterialTheme.colorScheme.outlineVariant,
            )
        }
        InfoRow("Processor", node.cpuLoadPercent?.let { "%.0f%%".format(it) } ?: "--")
        InfoRow(
            "Memory",
            if (node.memoryUsedBytes == null || node.memoryTotalBytes == null) {
                "--"
            } else {
                "${formatBytes(node.memoryUsedBytes)} of ${formatBytes(node.memoryTotalBytes)}"
            },
        )
        InfoRow("Running for", formatDuration(node.uptimeSeconds))
    }
}
