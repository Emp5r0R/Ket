package com.ket.android.connection

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

/** The connection settings, for a Settings card. Changes apply from the next connection. */
@Composable
fun ConnectionOptionsSection(
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
