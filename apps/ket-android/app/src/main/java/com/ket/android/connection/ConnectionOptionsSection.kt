package com.ket.android.connection

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.ket.android.KetProtocol

/**
 * The connection settings, for a Settings card. Changes apply from the next connection.
 * [offered] are the server's protocols, which Automatic may be told to leave out.
 */
@Composable
fun ConnectionOptionsSection(
    options: ConnectionOptions,
    onChange: (ConnectionOptions) -> Unit,
    offered: List<KetProtocol> = emptyList(),
) {
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        SplitTlsHelloRow(options, onChange)
        if (offered.size > 1) AutomaticProtocolsList(options, onChange, offered)
        BypassListSection(options, onChange)
    }
}

@Composable
private fun AutomaticProtocolsList(
    options: ConnectionOptions,
    onChange: (ConnectionOptions) -> Unit,
    offered: List<KetProtocol>,
) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text("Protocols Automatic tries", style = MaterialTheme.typography.bodyLarge)
        Text(
            "Leave out ones that never work for you. Choosing a protocol by hand still uses it, " +
                "and if you leave out every one, Automatic tries them all.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
        offered.forEach { protocol ->
            val included = protocol !in options.automaticExcluded
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .toggleable(
                        value = included,
                        role = Role.Checkbox,
                        onValueChange = { include ->
                            onChange(
                                options.copy(
                                    automaticExcluded = if (include) {
                                        options.automaticExcluded - protocol
                                    } else {
                                        options.automaticExcluded + protocol
                                    },
                                ),
                            )
                        },
                    ),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Checkbox(checked = included, onCheckedChange = null)
                Text(protocol.displayName, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun SplitTlsHelloRow(
    options: ConnectionOptions,
    onChange: (ConnectionOptions) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .toggleable(
                value = options.splitTlsHello,
                role = Role.Switch,
                onValueChange = { onChange(options.copy(splitTlsHello = it)) },
            ),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text("Split the TLS greeting", style = MaterialTheme.typography.bodyLarge)
            Text(
                "Sends the first message of VLESS + REALITY and HTTPS Stealth in small pieces, " +
                    "so a filter that reads the server name from one packet misses it. Try it if " +
                    "those two are blocked.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        Switch(checked = options.splitTlsHello, onCheckedChange = null)
    }
}
