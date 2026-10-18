package com.ket.android.connection

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.outlined.Circle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
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
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text("Protocols Automatic may use", style = MaterialTheme.typography.bodyLarge)
            Text(
                "Leave out ones that never work for you. Choosing a protocol by hand still uses it, " +
                    "and if you leave out every one, Automatic tries them all.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        offered.chunked(2).forEach { pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                pair.forEach { protocol ->
                    ProtocolChip(
                        protocol = protocol,
                        included = protocol !in options.automaticExcluded,
                        modifier = Modifier.weight(1f),
                        onIncludedChange = { include ->
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
                    )
                }
                if (pair.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

/** One protocol Automatic may use: filled when it is in, outlined when it is left out. */
@Composable
private fun ProtocolChip(
    protocol: KetProtocol,
    included: Boolean,
    modifier: Modifier,
    onIncludedChange: (Boolean) -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Surface(
        modifier = modifier
            .heightIn(min = 44.dp)
            .toggleable(value = included, role = Role.Checkbox, onValueChange = onIncludedChange),
        shape = RoundedCornerShape(12.dp),
        color = if (included) colors.primary.copy(alpha = 0.14f) else Color.Transparent,
        border = BorderStroke(1.dp, if (included) colors.primary else colors.outlineVariant),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                if (included) Icons.Filled.CheckCircle else Icons.Outlined.Circle,
                contentDescription = null,
                tint = if (included) colors.primary else colors.onSurfaceVariant,
                modifier = Modifier.size(18.dp),
            )
            Text(
                protocol.displayName,
                style = MaterialTheme.typography.bodyMedium,
                color = if (included) colors.onSurface else colors.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
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
