package com.ket.android.connection

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

/** Settings section for how Ket connects. Changes apply from the next connection. */
@Composable
fun ConnectionOptionsSection(
    options: ConnectionOptions,
    onChange: (ConnectionOptions) -> Unit,
) {
    Surface(
        color = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(6.dp),
        tonalElevation = 1.dp,
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 12.dp)) {
            Text("Connection", style = MaterialTheme.typography.labelMedium)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 8.dp)
                    .toggleable(
                        value = options.splitTlsHello,
                        role = Role.Switch,
                        onValueChange = { onChange(options.copy(splitTlsHello = it)) },
                    ),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text("Split the TLS greeting", style = MaterialTheme.typography.bodyMedium)
                    Text(
                        "Sends the first message of VLESS + REALITY and HTTPS Stealth in small pieces, " +
                            "so a filter that reads the server name from one packet misses it. Try it if " +
                            "those two are blocked. Applies from the next connection.",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                Switch(checked = options.splitTlsHello, onCheckedChange = null)
            }
        }
    }
}
