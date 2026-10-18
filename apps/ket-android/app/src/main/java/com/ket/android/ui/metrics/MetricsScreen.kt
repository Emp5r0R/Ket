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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.ket.android.AndroidNodeStatus
import com.ket.android.DataAllowanceStatus
import com.ket.android.TunnelPhase
import com.ket.android.TunnelSnapshot
import com.ket.android.formatAllowanceReset
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
internal fun MetricsScreen(
    snapshot: TunnelSnapshot,
    accessRemainingSeconds: Long?,
    trafficHistory: List<TrafficSample> = emptyList(),
) {
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
        val latest = if (connected) Throughput.latest(trafficHistory) else null
        val download = MaterialTheme.colorScheme.primary
        val upload = MaterialTheme.colorScheme.onSurfaceVariant
        SectionCard(title = "This connection") {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                TrafficFigure(
                    Icons.Filled.ArrowDownward,
                    "Download",
                    formatRate(latest?.received),
                    "${formatBytes(snapshot.receivedBytes)} in total",
                    download,
                    Modifier.weight(1f),
                )
                TrafficFigure(
                    Icons.Filled.ArrowUpward,
                    "Upload",
                    formatRate(latest?.sent),
                    "${formatBytes(snapshot.sentBytes)} in total",
                    upload,
                    Modifier.weight(1f),
                )
            }
            if (connected) {
                SpeedChart(
                    Throughput.rates(trafficHistory),
                    download = download,
                    upload = upload,
                    grid = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f),
                )
            }
            InfoRow("Protocol", if (connected) snapshot.transportName else "Not connected")
            InfoRow("Set up in", formatSetupTime(snapshot.handshakeLatencyMs))
            InfoRow("Access left", formatDuration(accessRemainingSeconds))
        }
        snapshot.allowance?.let { AllowanceCard(it) }
        ServerCard(node)
    }
}

/** How much of the invite's data allowance its devices have used this period. */
@Composable
private fun AllowanceCard(allowance: DataAllowanceStatus) {
    val tone = if (allowance.usedUp) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary
    SectionCard(title = "Data allowance") {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth()) {
                Text(
                    "Used ${allowance.period.current}",
                    Modifier.weight(1f),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text("${formatBytes(allowance.usedBytes)} of ${formatBytes(allowance.bytes)}", style = MaterialTheme.typography.bodyMedium)
            }
            LinearProgressIndicator(
                progress = { allowance.fraction },
                modifier = Modifier.fillMaxWidth().height(6.dp),
                color = tone,
                trackColor = MaterialTheme.colorScheme.outlineVariant,
            )
        }
        Text(allowanceText(allowance), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
    }
}

internal fun allowanceText(allowance: DataAllowanceStatus): String {
    val reset = formatAllowanceReset(allowance.resetsAtEpochSeconds)
    return if (allowance.usedUp) {
        "Used up. Ket can connect again from $reset."
    } else {
        "${formatBytes(allowance.bytes)} ${allowance.period.each}, shared by every device using this invite. Starts again $reset."
    }
}

@Composable
private fun TrafficFigure(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    speed: String,
    total: String,
    tone: Color,
    modifier: Modifier,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, contentDescription = null, tint = tone, modifier = Modifier.height(16.dp))
            Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
        }
        Text(speed, style = MaterialTheme.typography.headlineSmall)
        Text(total, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
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
