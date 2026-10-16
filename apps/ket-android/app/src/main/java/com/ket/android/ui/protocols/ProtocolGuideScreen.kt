package com.ket.android.ui.protocols

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.ket.android.AUTO_PROTOCOL_INSTRUCTION
import com.ket.android.KetProtocol
import com.ket.android.ui.common.CardShape
import com.ket.android.ui.common.Pill
import com.ket.android.ui.common.ScreenTitle
import com.ket.android.ui.common.SectionCard

/**
 * Every protocol Ket knows, explained. Protocols the server offers come
 * first and can be chosen here; [expanded] opens one, such as the one the
 * person came from.
 */
@Composable
internal fun ProtocolGuideScreen(
    offered: List<KetProtocol>,
    selected: KetProtocol?,
    expanded: KetProtocol?,
    canChoose: Boolean,
    onChoose: (KetProtocol?) -> Unit,
) {
    val ordered = KetProtocol.entries.sortedBy { if (offered.isEmpty() || it in offered) 0 else 1 }
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        ScreenTitle("Protocols", "Different ways to reach your server. Ket picks for you unless you choose.")
        SectionCard(title = null) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Automatic", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                if (selected == null) Pill("In use", MaterialTheme.colorScheme.primary)
            }
            Text(AUTO_PROTOCOL_INSTRUCTION, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            if (selected != null && canChoose) {
                FilledTonalButton(onClick = { onChoose(null) }) { Text("Use Automatic") }
            }
        }
        ordered.forEach { protocol ->
            ProtocolCard(
                protocol = protocol,
                offered = offered.isEmpty() || protocol in offered,
                inUse = selected == protocol,
                initiallyExpanded = expanded == protocol,
                canChoose = canChoose,
                onChoose = { onChoose(protocol) },
            )
        }
        if (!canChoose) {
            Text(
                "Disconnect to change the protocol.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
    }
}

@Composable
private fun ProtocolCard(
    protocol: KetProtocol,
    offered: Boolean,
    inUse: Boolean,
    initiallyExpanded: Boolean,
    canChoose: Boolean,
    onChoose: () -> Unit,
) {
    var open by rememberSaveable(protocol.wireName) { mutableStateOf(initiallyExpanded) }
    Surface(
        shape = CardShape,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, if (inUse) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column {
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable(role = Role.Button, onClick = { open = !open })
                    .semantics { stateDescription = if (open) "Expanded" else "Collapsed" }
                    .padding(16.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(protocol.displayName, style = MaterialTheme.typography.titleMedium)
                        when {
                            inUse -> Pill("In use", MaterialTheme.colorScheme.primary)
                            !offered -> Pill("Not on your server", MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    Text(protocol.bestFor, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
                }
                Icon(if (open) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore, contentDescription = null)
            }
            AnimatedVisibility(open) {
                Column(
                    Modifier.padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    GuideText("When to use it", protocol.shortInstruction)
                    GuideText("How it works", protocol.operation)
                    GuideList("To connect", protocol.steps)
                    GuideList("Limits", protocol.limitations)
                    if (offered && !inUse && canChoose) {
                        FilledTonalButton(onClick = onChoose) { Text("Use ${protocol.displayName}") }
                    }
                }
            }
        }
    }
}

@Composable
private fun GuideText(title: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title, color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge)
        Text(body, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun GuideList(title: String, items: List<String>) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title, color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge)
        items.forEachIndexed { index, item ->
            Text("${index + 1}. $item", style = MaterialTheme.typography.bodyMedium)
        }
    }
}
