package com.ket.android.splittunnel

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Settings content that chooses which apps use Ket. Changes are saved at once
 * and take effect the next time the VPN interface is built.
 */
@Composable
fun SplitTunnelSection(
    policy: SplitTunnelPolicy,
    connected: Boolean,
    onChange: (SplitTunnelPolicy) -> Unit,
    loadApps: () -> List<InstalledApp>,
) {
    var picking by rememberSaveable { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth()) {
        Column(Modifier.selectableGroup()) {
            SplitTunnelMode.entries.forEach { mode ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(40.dp)
                        .selectable(
                            selected = policy.mode == mode,
                            onClick = { onChange(policy.copy(mode = mode)) },
                            role = Role.RadioButton,
                        ),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = policy.mode == mode, onClick = null)
                    Text(mode.displayName, Modifier.padding(start = 10.dp), style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
        Text(
            SplitTunnelChoices.summary(policy),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
        if (connected) {
            Text(
                "Changes apply the next time you connect.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        if (policy.mode != SplitTunnelMode.AllApps) {
            OutlinedButton(
                onClick = { picking = true },
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp).height(40.dp),
            ) {
                Text("Choose apps (${policy.packages.size})")
            }
        }
    }
    if (picking) {
        SplitTunnelAppPicker(
            policy = policy,
            loadApps = loadApps,
            onChange = onChange,
            onDismissRequest = { picking = false },
        )
    }
}

@Composable
private fun SplitTunnelAppPicker(
    policy: SplitTunnelPolicy,
    loadApps: () -> List<InstalledApp>,
    onChange: (SplitTunnelPolicy) -> Unit,
    onDismissRequest: () -> Unit,
) {
    var query by rememberSaveable { mutableStateOf("") }
    val apps by produceState<List<InstalledApp>?>(initialValue = null, loadApps) {
        value = withContext(Dispatchers.Default) { loadApps() }
    }
    val title = remember(policy.mode) {
        if (policy.mode == SplitTunnelMode.OnlySelected) "Apps that use Ket" else "Apps that skip Ket"
    }
    Dialog(
        onDismissRequest = onDismissRequest,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        BoxWithConstraints(
            modifier = Modifier.fillMaxSize().safeDrawingPadding().padding(16.dp),
            contentAlignment = Alignment.Center,
        ) {
            Surface(
                modifier = Modifier.fillMaxWidth().widthIn(max = 560.dp).height(minOf(maxHeight, 640.dp)),
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.surface,
                tonalElevation = 6.dp,
            ) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(title, style = MaterialTheme.typography.titleMedium)
                    OutlinedTextField(
                        value = query,
                        onValueChange = { query = it },
                        label = { Text("Search apps") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Box(Modifier.weight(1f).fillMaxWidth()) {
                        val loaded = apps
                        if (loaded == null) {
                            CircularProgressIndicator(Modifier.align(Alignment.Center))
                        } else {
                            LazyColumn(Modifier.fillMaxSize()) {
                                items(SplitTunnelChoices.search(loaded, query), key = { it.packageName }) { app ->
                                    AppChoiceRow(
                                        app = app,
                                        checked = app.packageName in policy.packages,
                                        onToggle = { onChange(SplitTunnelChoices.toggle(policy, app.packageName)) },
                                    )
                                }
                            }
                        }
                    }
                    Text(
                        SplitTunnelChoices.summary(policy),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                    )
                    TextButton(onClick = onDismissRequest, modifier = Modifier.align(Alignment.End)) {
                        Text("Done")
                    }
                }
            }
        }
    }
}

@Composable
private fun AppChoiceRow(app: InstalledApp, checked: Boolean, onToggle: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().clickable(role = Role.Checkbox, onClick = onToggle).padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = checked, onCheckedChange = null)
        Column(Modifier.padding(start = 10.dp)) {
            Text(app.label, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                app.packageName,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
