package com.ket.android.share

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel

/** What the screens can ask of the share. */
data class ShareActions(
    val networks: () -> List<ShareNetwork>,
    val start: (String) -> Unit,
    val stop: () -> Unit,
) {
    companion object {
        val None = ShareActions(networks = { emptyList() }, start = {}, stop = {})
    }
}

internal fun devicesText(count: Int): String = when (count) {
    0 -> "No devices connected"
    1 -> "1 device connected"
    else -> "$count devices connected"
}

internal fun refusedText(count: Long): String =
    if (count == 1L) "1 sign-in with a wrong password was refused." else "$count sign-ins with a wrong password were refused."

/**
 * Settings for sharing the phone's Ket connection with nearby devices, such as a laptop joined
 * to the phone's hotspot. Off until started; every start draws a new password.
 */
@Composable
internal fun ShareSection(state: ShareState, actions: ShareActions, connected: Boolean) {
    var networks by remember { mutableStateOf(actions.networks()) }
    var chosen by rememberSaveable { mutableStateOf<String?>(null) }
    LaunchedEffect(state.active) { if (state.active == null) networks = actions.networks() }
    val active = state.active
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (active == null) {
            Text(
                "Ket opens a proxy on the network you choose, protected by a new password each time. " +
                    "Devices on that network set it as their proxy and browse through your Ket connection. " +
                    "Use it on networks you trust, such as this phone's own hotspot: the link between them " +
                    "and this phone is not encrypted by Ket, though HTTPS sites stay encrypted.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
            if (networks.isEmpty()) {
                Text(
                    "This phone is not on a network it can share on. Turn on its hotspot or join a Wi-Fi network.",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            val selected = networks.firstOrNull { it.address == chosen } ?: networks.firstOrNull()
            networks.forEach { network ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .selectable(
                            selected = network == selected,
                            onClick = { chosen = network.address },
                            role = Role.RadioButton,
                        ),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = network == selected, onClick = null)
                    Spacer(Modifier.width(8.dp))
                    Text(network.label, style = MaterialTheme.typography.bodyMedium)
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Button(
                    onClick = { selected?.let { actions.start(it.address) } },
                    enabled = connected && selected != null,
                ) {
                    Icon(Icons.Filled.Share, contentDescription = null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("Start sharing")
                }
                IconButton(onClick = { networks = actions.networks() }) {
                    Icon(Icons.Filled.Refresh, contentDescription = "Look for networks again")
                }
            }
            if (!connected) {
                Text(
                    "Connect Ket first; sharing stops when you disconnect.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        } else {
            ActiveShareDetails(state, active, actions)
        }
        state.notice?.let {
            Text(it.message, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun ColumnScope.ActiveShareDetails(state: ShareState, active: ActiveShare, actions: ShareActions) {
    val clipboard = LocalClipboardManager.current
    val qr = remember(active.proxyUrl) { qrBitmap(active.proxyUrl) }
    Text(
        devicesText(state.devices) + if (!state.tunnel) " · Paused while Ket reconnects" else "",
        style = MaterialTheme.typography.bodyLarge,
    )
    qr?.let {
        Image(
            bitmap = it.asImageBitmap(),
            contentDescription = "QR code with the proxy address and password",
            modifier = Modifier
                .size(200.dp)
                .background(Color.White, RoundedCornerShape(8.dp))
                .padding(8.dp)
                .align(Alignment.CenterHorizontally),
        )
    }
    DetailRow("Proxy (HTTP or SOCKS5)", active.network.address)
    DetailRow("Port", active.port.toString())
    DetailRow("User name", active.username)
    Row(verticalAlignment = Alignment.CenterVertically) {
        DetailRow("Password", active.password, Modifier.weight(1f))
        IconButton(onClick = { clipboard.setText(AnnotatedString(active.password)) }) {
            Icon(Icons.Filled.ContentCopy, contentDescription = "Copy the password")
        }
    }
    Text(
        "On the other device, open its Wi-Fi settings, set the proxy to Manual with this address and " +
            "port, and sign in when the browser asks. Proxy apps can scan the code.",
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        style = MaterialTheme.typography.bodySmall,
    )
    TextButton(onClick = { clipboard.setText(AnnotatedString(active.proxyUrl)) }) { Text("Copy the proxy link") }
    if (state.refusedSignIns > 0) {
        Text(
            refusedText(state.refusedSignIns) + " Stop and start sharing again for a new password if that was not you.",
            color = MaterialTheme.colorScheme.error,
            style = MaterialTheme.typography.bodySmall,
        )
    }
    OutlinedButton(onClick = actions.stop, modifier = Modifier.fillMaxWidth()) {
        Text("Stop sharing", color = MaterialTheme.colorScheme.error)
    }
}

@Composable
private fun DetailRow(label: String, value: String, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        Text(value, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodyMedium)
    }
}

/** Shown on the connection screen while the phone shares its connection. */
@Composable
internal fun ShareBanner(state: ShareState, onOpenDetails: () -> Unit, onStop: () -> Unit) {
    val active = state.active ?: return
    Surface(
        shape = RoundedCornerShape(16.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Filled.Share, contentDescription = null, Modifier.size(20.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text("Sharing on ${active.network.address}:${active.port}", style = MaterialTheme.typography.bodyMedium)
                Text(
                    devicesText(state.devices) + if (!state.tunnel) ". Paused while Ket reconnects." else ".",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            TextButton(onClick = onOpenDetails) { Text("Details") }
            TextButton(onClick = onStop) { Text("Stop") }
        }
    }
}

/** The proxy link as a QR code, black on white, or null if it cannot be drawn. */
internal fun qrBitmap(text: String, size: Int = 512): Bitmap? = runCatching {
    val matrix = QRCodeWriter().encode(
        text,
        BarcodeFormat.QR_CODE,
        size,
        size,
        mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M, EncodeHintType.MARGIN to 1),
    )
    val pixels = IntArray(matrix.width * matrix.height) { index ->
        if (matrix.get(index % matrix.width, index / matrix.width)) android.graphics.Color.BLACK else android.graphics.Color.WHITE
    }
    Bitmap.createBitmap(pixels, matrix.width, matrix.height, Bitmap.Config.ARGB_8888)
}.getOrNull()
