package com.ket.android.ui.connection

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.ket.android.AUTO_PROTOCOL_INSTRUCTION
import com.ket.android.KetProtocol

/**
 * The protocols this server offers for this invite, plus Automatic. A saved
 * choice the server no longer offers stays visible, marked, so it is clear
 * why connecting with it fails.
 */
internal fun pickerChoices(offered: List<KetProtocol>, selected: KetProtocol?): List<KetProtocol> {
    val known = offered.ifEmpty { KetProtocol.entries }
    return if (selected != null && selected !in known) known + selected else known
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ProtocolPicker(
    offered: List<KetProtocol>,
    selected: KetProtocol?,
    onSelect: (KetProtocol?) -> Unit,
    onLearnMore: () -> Unit,
    onDismiss: () -> Unit,
) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        Column(
            Modifier
                .fillMaxWidth()
                .navigationBarsPadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 16.dp)
                .selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text("Protocol", style = MaterialTheme.typography.titleLarge)
            Text(
                "Automatic is best for most people. Pick one only if your network blocks the others.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
            ProtocolChoice("Automatic", AUTO_PROTOCOL_INSTRUCTION, selected == null, offeredByServer = true) {
                onSelect(null)
            }
            pickerChoices(offered, selected).forEach { protocol ->
                val isOffered = offered.isEmpty() || protocol in offered
                ProtocolChoice(protocol.displayName, protocol.bestFor, selected == protocol, isOffered) {
                    onSelect(protocol)
                }
            }
            TextButton(onClick = onLearnMore, modifier = Modifier.align(Alignment.End)) {
                Text("How each protocol works")
            }
        }
    }
}

@Composable
private fun ProtocolChoice(
    title: String,
    detail: String,
    selected: Boolean,
    offeredByServer: Boolean,
    onClick: () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .selectable(selected = selected, enabled = offeredByServer, role = Role.RadioButton, onClick = onClick)
            .padding(vertical = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null, enabled = offeredByServer)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            Text(
                if (offeredByServer) detail else "Not available for this invite",
                color = if (offeredByServer) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}
