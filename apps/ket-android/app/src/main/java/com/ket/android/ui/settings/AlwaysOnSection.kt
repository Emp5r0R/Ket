package com.ket.android.ui.settings

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.ket.android.AlwaysOnState

/**
 * Android's own protection for VPN apps: start Ket after a restart, and block every app's
 * internet whenever Ket is not connected, so nothing leaves the phone while Ket starts or
 * reconnects. Only the system can turn it on; this opens its VPN settings.
 */
@Composable
internal fun AlwaysOnSection(state: AlwaysOnState?, onOpenVpnSettings: () -> Unit) {
    Text(
        "Android can start Ket by itself after a restart and block every app's internet until " +
            "Ket is connected, so nothing slips out while Ket starts or reconnects. In Android's " +
            "VPN settings, open Ket and turn on Always-on VPN and Block connections without VPN.",
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        style = MaterialTheme.typography.bodySmall,
    )
    Text(alwaysOnText(state), style = MaterialTheme.typography.bodyMedium)
    OutlinedButton(onClick = onOpenVpnSettings, modifier = Modifier.fillMaxWidth()) {
        Icon(Icons.Filled.Settings, contentDescription = null, Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text("Open VPN settings")
    }
}

internal fun alwaysOnText(state: AlwaysOnState?): String = when {
    state == null -> "Connect once to see whether it is on."
    state.alwaysOn && state.lockdown -> "On: Ket starts by itself, and apps have no internet without it."
    state.alwaysOn -> "Ket starts by itself, but apps can still reach the internet without it."
    else -> "Off: apps use the phone's own connection whenever Ket is not connected."
}
