package com.ket.android.ui.connection

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.ket.android.R
import com.ket.android.extractInvite
import com.ket.android.qr.InviteQrScanner
import com.ket.android.qr.QrImageReader
import com.ket.android.ui.common.SectionCard
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private val ACCESS_CODE = Regex("^[A-Za-z0-9]{32}$")

/**
 * First run: add the server someone shared. Every way in (paste, scan, an
 * image, or Share to Ket from another app) hands the invite straight to
 * [onInvite]; there is nothing to type. A server address and access code
 * stay under Advanced for older servers.
 */
@Composable
internal fun AddServerScreen(
    issue: String?,
    sharedInvite: String?,
    onSharedInviteConsumed: () -> Unit,
    onInvite: (String) -> Unit,
    onManual: (serverUrl: String, accessCode: String) -> Unit,
    onIssueChange: (String?) -> Unit,
) {
    var manual by rememberSaveable { mutableStateOf(false) }
    var scanning by remember { mutableStateOf(false) }
    val clipboard = LocalClipboardManager.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val pickImage = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val decoded = withContext(Dispatchers.Default) {
                runCatching { QrImageReader.decode(context, uri) }.getOrNull()
            }
            val invite = extractInvite(decoded)
            if (invite != null) onInvite(invite) else onIssueChange("No Ket invite QR code was found in that image.")
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(20.dp)) {
        Column(
            Modifier.fillMaxWidth().padding(top = 12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Image(painterResource(R.drawable.ket_mark), contentDescription = null, modifier = Modifier.size(64.dp))
            Text("Add your server", style = MaterialTheme.typography.headlineSmall)
            Text(
                "Use the invite you were given. It starts with ket:invite: and often comes as a QR code.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
        }
        if (sharedInvite != null) {
            SectionCard(title = null) {
                Text("Invite received", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Another app shared a Ket invite. Add its server to this phone?",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedButton(onClick = onSharedInviteConsumed, modifier = Modifier.weight(1f)) { Text("Not now") }
                    Button(
                        onClick = {
                            onSharedInviteConsumed()
                            onInvite(sharedInvite)
                        },
                        modifier = Modifier.weight(1f),
                    ) { Text("Add server") }
                }
            }
        }
        if (!manual && sharedInvite == null) {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Button(
                    onClick = {
                        val invite = extractInvite(clipboard.getText()?.text)
                        if (invite != null) {
                            onInvite(invite)
                        } else {
                            onIssueChange("There is no Ket invite on the clipboard. Copy the whole invite first, then tap Paste.")
                        }
                    },
                    modifier = Modifier.fillMaxWidth().height(56.dp),
                    shape = RoundedCornerShape(14.dp),
                ) {
                    Icon(Icons.Filled.ContentPaste, contentDescription = null, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(10.dp))
                    Text("Paste invite", style = MaterialTheme.typography.titleMedium)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedButton(
                        onClick = { scanning = true },
                        modifier = Modifier.weight(1f).height(52.dp),
                        shape = RoundedCornerShape(14.dp),
                    ) {
                        Icon(Icons.Filled.QrCodeScanner, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Scan QR")
                    }
                    OutlinedButton(
                        onClick = { pickImage.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                        modifier = Modifier.weight(1f).height(52.dp),
                        shape = RoundedCornerShape(14.dp),
                    ) {
                        Icon(Icons.Filled.Image, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("From image")
                    }
                }
            }
        }
        AnimatedVisibility(visible = issue != null) {
            issue?.let { IssueBanner(it) }
        }
        if (manual) {
            ManualEntry(
                onAdd = { url, code ->
                    onIssueChange(null)
                    onManual(url, code)
                },
            )
        }
        TextButton(
            onClick = {
                manual = !manual
                onIssueChange(null)
            },
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(if (manual) "Use an invite instead" else "Advanced: server address and access code")
        }
        Text(
            "When you connect, Android asks you to allow Ket's status notification and to set up a VPN. Both are needed.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
    if (scanning) {
        InviteQrScanner(
            accept = { extractInvite(it) != null },
            onScanned = { value ->
                scanning = false
                extractInvite(value)?.let(onInvite)
            },
            onDismiss = { scanning = false },
        )
    }
}

@Composable
private fun IssueBanner(message: String) {
    Surface(
        modifier = Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite },
        color = MaterialTheme.colorScheme.errorContainer,
        shape = RoundedCornerShape(12.dp),
    ) {
        Row(Modifier.padding(14.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.colorScheme.error, modifier = Modifier.size(20.dp))
            Text(message, color = MaterialTheme.colorScheme.onErrorContainer, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

@Composable
private fun ManualEntry(onAdd: (String, String) -> Unit) {
    var serverUrl by rememberSaveable { mutableStateOf("") }
    var accessCode by remember { mutableStateOf("") }
    SectionCard(title = "Server address and access code") {
        OutlinedTextField(
            value = serverUrl,
            onValueChange = { serverUrl = it.trim() },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Server address") },
            placeholder = { Text("https://example.com") },
            leadingIcon = { Icon(Icons.Filled.Link, contentDescription = null) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            singleLine = true,
        )
        OutlinedTextField(
            value = accessCode,
            onValueChange = { value -> accessCode = value.filter(Char::isLetterOrDigit).take(32) },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Access code") },
            supportingText = { Text("${accessCode.length} of 32 characters") },
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            singleLine = true,
        )
        Button(
            onClick = { onAdd(serverUrl, accessCode) },
            enabled = serverUrl.isNotBlank() && accessCode.matches(ACCESS_CODE),
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(14.dp),
        ) {
            Text("Add server")
        }
    }
}
