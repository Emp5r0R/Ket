package com.ket.android.connection

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp

/** What to tell someone whose entry was refused. */
internal fun BypassRefusal.message(): String = when (this) {
    BypassRefusal.TooWide -> "That network is too large. Use a /8 or narrower for IPv4, /16 or narrower for IPv6."
    BypassRefusal.Special -> "Loopback, multicast, and broadcast addresses never use the tunnel."
    BypassRefusal.TooMany -> "Keep at most ${BypassEntries.MAX_ENTRIES} entries."
    BypassRefusal.Invalid -> "Enter an IP address, such as 203.0.113.5, or a network, such as 192.168.10.0/24."
}

/** Adds [text] to [entries] in its stored form, or says why it cannot be added. */
internal fun addBypassEntry(entries: List<String>, text: String): Result<List<String>> =
    when (val checked = BypassEntries.check(text)) {
        is BypassCheck.Refused -> Result.failure(IllegalArgumentException(checked.refusal.message()))
        is BypassCheck.Accepted -> {
            val entry = checked.prefix.toString()
            when {
                entry in entries -> Result.success(entries)
                entries.size >= BypassEntries.MAX_ENTRIES ->
                    Result.failure(IllegalArgumentException(BypassRefusal.TooMany.message()))
                else -> Result.success(entries + entry)
            }
        }
    }

/** Settings for addresses and networks reached directly, outside the tunnel. */
@Composable
internal fun BypassListSection(
    options: ConnectionOptions,
    onChange: (ConnectionOptions) -> Unit,
) {
    var draft by rememberSaveable { mutableStateOf("") }
    var refusal by rememberSaveable { mutableStateOf<String?>(null) }
    val add = {
        addBypassEntry(options.bypass, draft).fold(
            onSuccess = {
                onChange(options.copy(bypass = it))
                draft = ""
                refusal = null
            },
            onFailure = { refusal = it.message },
        )
    }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text("Keep outside the tunnel", style = MaterialTheme.typography.bodyLarge)
        Text(
            "Addresses and networks listed here are reached directly, with every protocol: your home " +
                "network's printer, a work network, or a site that refuses VPN traffic. The network you " +
                "are on, and anyone watching it, can see that traffic and where it goes.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = draft,
                onValueChange = {
                    draft = it
                    refusal = null
                },
                modifier = Modifier.weight(1f),
                singleLine = true,
                label = { Text("Address or network") },
                placeholder = { Text("192.168.1.0/24") },
                isError = refusal != null,
                supportingText = refusal?.let { { Text(it) } },
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.None,
                    autoCorrect = false,
                    keyboardType = KeyboardType.Uri,
                    imeAction = ImeAction.Done,
                ),
                keyboardActions = KeyboardActions(onDone = { if (draft.isNotBlank()) add() }),
            )
            FilledTonalButton(onClick = add, enabled = draft.isNotBlank()) { Text("Add") }
        }
        options.bypass.forEach { entry ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    entry,
                    modifier = Modifier.weight(1f),
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodyMedium,
                )
                IconButton(onClick = { onChange(options.copy(bypass = options.bypass - entry)) }) {
                    Icon(Icons.Filled.Close, contentDescription = "Stop keeping $entry outside the tunnel")
                }
            }
        }
    }
}
