package com.ket.android.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.MenuBook
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.DarkMode
import androidx.compose.material.icons.filled.DeleteOutline
import androidx.compose.material.icons.filled.LightMode
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ket.android.KetThemeMode
import com.ket.android.KetProtocol
import com.ket.android.connection.ConnectionOptions
import com.ket.android.connection.ConnectionOptionsSection
import com.ket.android.formatDuration
import com.ket.android.splittunnel.InstalledApp
import com.ket.android.splittunnel.SplitTunnelPolicy
import com.ket.android.splittunnel.SplitTunnelSection
import com.ket.android.ui.common.InfoRow
import com.ket.android.ui.common.ScreenTitle
import com.ket.android.ui.common.SectionCard
import com.ket.android.ui.connection.serverHost

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SettingsScreen(
    themeMode: KetThemeMode,
    onThemeModeChange: (KetThemeMode) -> Unit,
    splitTunnel: SplitTunnelPolicy,
    connected: Boolean,
    onSplitTunnelChange: (SplitTunnelPolicy) -> Unit,
    loadInstalledApps: () -> List<InstalledApp>,
    connectionOptions: ConnectionOptions,
    onConnectionOptionsChange: (ConnectionOptions) -> Unit,
    offeredProtocols: List<KetProtocol>,
    savedServerUrl: String?,
    accessRemainingSeconds: Long?,
    onForgetServer: () -> Unit,
    onOpenDiagnostics: () -> Unit,
    onOpenProtocolGuide: () -> Unit,
    appVersion: String,
    serverVersion: String?,
) {
    var confirmingForget by rememberSaveable { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(20.dp)) {
        ScreenTitle("Settings")
        SectionCard(title = "Appearance") {
            val icons = listOf(Icons.Filled.Computer, Icons.Filled.LightMode, Icons.Filled.DarkMode)
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                KetThemeMode.entries.forEachIndexed { index, mode ->
                    SegmentedButton(
                        selected = themeMode == mode,
                        onClick = { onThemeModeChange(mode) },
                        shape = SegmentedButtonDefaults.itemShape(index, KetThemeMode.entries.size),
                        icon = { Icon(icons[index], contentDescription = null, Modifier.size(16.dp)) },
                    ) {
                        Text(mode.displayName)
                    }
                }
            }
        }
        SectionCard(title = "Apps that use Ket") {
            SplitTunnelSection(
                policy = splitTunnel,
                connected = connected,
                onChange = onSplitTunnelChange,
                loadApps = loadInstalledApps,
            )
        }
        SectionCard(title = "Connection", subtitle = "Applies from the next connection.") {
            ConnectionOptionsSection(connectionOptions, onConnectionOptionsChange, offeredProtocols)
        }
        if (savedServerUrl != null) {
            SectionCard(title = "Your server") {
                InfoRow("Address", serverHost(savedServerUrl))
                InfoRow("Access left", formatDuration(accessRemainingSeconds))
                serverVersion?.let { InfoRow("Server version", "Ket $it") }
                OutlinedButton(
                    onClick = { confirmingForget = true },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Filled.DeleteOutline, contentDescription = null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.error)
                    Spacer(Modifier.width(8.dp))
                    Text("Forget this server", color = MaterialTheme.colorScheme.error)
                }
            }
        }
        SectionCard(title = "Help") {
            HelpButton(Icons.AutoMirrored.Filled.MenuBook, "How the protocols work", onOpenProtocolGuide)
            HelpButton(Icons.Filled.BugReport, "Connection diagnostics", onOpenDiagnostics)
        }
        Text(
            "Ket $appVersion",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.align(Alignment.CenterHorizontally),
        )
    }
    if (confirmingForget) {
        AlertDialog(
            onDismissRequest = { confirmingForget = false },
            title = { Text("Forget this server?") },
            text = {
                Text(
                    if (connected) {
                        "Ket disconnects and removes the saved invite from this phone. You will need the invite again to come back."
                    } else {
                        "Ket removes the saved invite from this phone. You will need the invite again to come back."
                    },
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmingForget = false
                    onForgetServer()
                }) { Text("Forget", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmingForget = false }) { Text("Keep") } },
        )
    }
}

@Composable
private fun HelpButton(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, onClick: () -> Unit) {
    TextButton(onClick = onClick, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, contentDescription = null, Modifier.size(20.dp))
            Spacer(Modifier.width(12.dp))
            Text(label, style = MaterialTheme.typography.bodyLarge)
        }
    }
}
