package com.ket.android.ui.connection

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.SwapVert
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.ket.android.AndroidNodeStatus
import com.ket.android.KetMap
import com.ket.android.KetProtocol
import com.ket.android.TunnelPhase
import com.ket.android.TunnelSnapshot
import com.ket.android.connection.FallbackNotes
import com.ket.android.connectionLocationHeading
import com.ket.android.connectionPowerDetail
import com.ket.android.formatBytes
import com.ket.android.formatDuration
import com.ket.android.ui.common.CardShape
import com.ket.android.ui.common.Pill
import com.ket.android.ui.healthTone
import com.ket.android.KetUpdateBanner
import com.ket.android.share.ShareBanner
import com.ket.android.share.ShareState

/**
 * The home screen once a server is saved: where the server is, the power
 * button within the first screenful, then the route details.
 */
@Composable
internal fun ConnectionScreen(
    snapshot: TunnelSnapshot,
    lastNode: AndroidNodeStatus?,
    serverUrl: String,
    accessRemainingSeconds: Long?,
    preferredProtocol: KetProtocol?,
    offeredProtocols: List<KetProtocol>,
    canStartConnection: Boolean,
    onPreferredProtocolChange: (KetProtocol?) -> Unit,
    onOpenProtocolGuide: () -> Unit,
    onToggle: () -> Unit,
    onOpenDiagnostics: () -> Unit,
    share: ShareState = ShareState(),
    onOpenShare: () -> Unit = {},
    onStopShare: () -> Unit = {},
) {
    val connected = snapshot.phase == TunnelPhase.Connected
    val busy = snapshot.phase in setOf(
        TunnelPhase.Enrolling,
        TunnelPhase.Connecting,
        TunnelPhase.Reconnecting,
        TunnelPhase.Stopping,
    )
    var choosingProtocol by remember { mutableStateOf(false) }
    val node = snapshot.node ?: lastNode
    Column(verticalArrangement = Arrangement.spacedBy(18.dp)) {
        Column(Modifier.padding(horizontal = 4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(connectionLocationHeading(node), style = MaterialTheme.typography.headlineMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    node?.displayName ?: serverHost(serverUrl),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                node?.let { Pill(it.health.displayName, healthTone(it.health)) }
            }
        }
        Surface(
            shape = CardShape,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
            color = MaterialTheme.colorScheme.surface,
        ) {
            KetMap(node?.location, connected)
        }
        PowerButton(
            phase = snapshot.phase,
            detail = powerDetail(snapshot),
            enabled = snapshot.phase != TunnelPhase.Stopping && (connected || busy || canStartConnection),
            onClick = onToggle,
        )
        FallbackNotes(
            notes = snapshot.fallbacks,
            activeTransportName = snapshot.transportName.takeIf { connected },
        )
        if (snapshot.phase == TunnelPhase.Failed && snapshot.message.isNotBlank()) {
            FailureNotice(snapshot.message, onOpenDiagnostics)
        }
        KetUpdateBanner(snapshot.clientUpdate)
        ShareBanner(share, onOpenDetails = onOpenShare, onStop = onStopShare)
        Surface(
            shape = CardShape,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
            color = MaterialTheme.colorScheme.surface,
        ) {
            Column {
                RouteRow(
                    icon = Icons.Filled.Tune,
                    label = "Protocol",
                    value = preferredProtocol?.displayName ?: "Automatic",
                    hint = when {
                        connected -> "Now using ${snapshot.transportName}. Disconnect to change."
                        busy -> null
                        else -> "Tap to choose"
                    },
                    onClick = if (connected || busy) null else ({ choosingProtocol = true }),
                )
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                RouteRow(Icons.Filled.Schedule, "Access left", formatDuration(accessRemainingSeconds))
                if (connected) {
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    RouteRow(
                        Icons.Filled.SwapVert,
                        "Traffic",
                        "↓ ${formatBytes(snapshot.receivedBytes)}   ↑ ${formatBytes(snapshot.sentBytes)}",
                    )
                }
            }
        }
    }
    if (choosingProtocol) {
        ProtocolPicker(
            offered = offeredProtocols,
            selected = preferredProtocol,
            onSelect = {
                onPreferredProtocolChange(it)
                choosingProtocol = false
            },
            onLearnMore = {
                choosingProtocol = false
                onOpenProtocolGuide()
            },
            onDismiss = { choosingProtocol = false },
        )
    }
}

private fun powerDetail(snapshot: TunnelSnapshot): String = when (snapshot.phase) {
    TunnelPhase.Connected -> "Protected with ${snapshot.transportName}"
    TunnelPhase.Connecting, TunnelPhase.Reconnecting, TunnelPhase.Enrolling ->
        snapshot.message.ifBlank { connectionPowerDetail(snapshot.phase) }
    else -> connectionPowerDetail(snapshot.phase)
}

internal fun serverHost(serverUrl: String): String =
    serverUrl.removePrefix("https://").removePrefix("http://").substringBefore('/').ifBlank { "Saved server" }

@Composable
private fun RouteRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    value: String,
    hint: String? = null,
    onClick: (() -> Unit)? = null,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, contentDescription = null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
            Text(value, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            hint?.let {
                Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            }
        }
        if (onClick != null) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun FailureNotice(message: String, onOpenDiagnostics: () -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.errorContainer,
        shape = RoundedCornerShape(12.dp),
    ) {
        Column(Modifier.padding(start = 14.dp, end = 6.dp, top = 12.dp, bottom = 4.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.colorScheme.error, modifier = Modifier.size(20.dp))
                Text(message, color = MaterialTheme.colorScheme.onErrorContainer, style = MaterialTheme.typography.bodyMedium)
            }
            TextButton(onClick = onOpenDiagnostics, modifier = Modifier.align(Alignment.End)) {
                Icon(Icons.Filled.BugReport, contentDescription = null, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text("Troubleshoot")
            }
        }
    }
}
