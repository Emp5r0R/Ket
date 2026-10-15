package com.ket.android

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
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
import androidx.compose.material.icons.filled.ConfirmationNumber
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.QrCodeScanner
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.ket.android.qr.InviteQrScanner
import com.ket.android.qr.QrImageReader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private const val INVITE_MIN_LENGTH = 32

/**
 * New-server entry. An invite (pasted, scanned, or read from an image) is the default; a server
 * URL and access code remain available under Advanced. Invite text is held only in memory.
 */
@Composable
internal fun EnrollmentControls(
    serverUrl: String,
    accessCode: String,
    inviteIssue: String?,
    acceptsInvite: (String) -> Boolean,
    onServerUrlChange: (String) -> Unit,
    onAccessCodeChange: (String) -> Unit,
    onEnrollInvite: (String) -> Unit,
    onEnrollManual: () -> Unit,
) {
    var manual by rememberSaveable { mutableStateOf(false) }
    var invite by remember { mutableStateOf("") }
    var localIssue by remember { mutableStateOf<String?>(null) }
    var scanning by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val pickImage = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val decoded = withContext(Dispatchers.Default) {
                runCatching { QrImageReader.decode(context, uri) }.getOrNull()
            }
            if (decoded != null && acceptsInvite(decoded)) {
                invite = decoded
                localIssue = null
            } else {
                localIssue = "No valid Ket invite QR code was found in that image."
            }
        }
    }
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(7.dp),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(
                    "NEW CONNECTION",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelSmall,
                )
                Text("Add a Ket server", style = MaterialTheme.typography.titleLarge)
            }
            if (manual) {
                OutlinedTextField(
                    value = serverUrl,
                    onValueChange = onServerUrlChange,
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Server URL") },
                    leadingIcon = { Icon(Icons.Filled.Link, contentDescription = null) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = accessCode,
                    onValueChange = onAccessCodeChange,
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Access code") },
                    supportingText = { Text("${accessCode.length}/32") },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    singleLine = true,
                )
                Button(
                    onClick = onEnrollManual,
                    enabled = serverUrl.isNotBlank() && accessCode.matches(Regex("^[A-Za-z0-9]{32}$")),
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) {
                    Text("Add server")
                }
            } else {
                OutlinedTextField(
                    value = invite,
                    onValueChange = { value ->
                        invite = value.filterNot(Char::isWhitespace).take(2_048)
                        localIssue = null
                    },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Invite") },
                    placeholder = { Text("ket:invite:...") },
                    leadingIcon = { Icon(Icons.Filled.ConfirmationNumber, contentDescription = null) },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    singleLine = true,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
                    OutlinedButton(onClick = { scanning = true }, modifier = Modifier.weight(1f).height(44.dp)) {
                        Icon(Icons.Filled.QrCodeScanner, contentDescription = null, modifier = Modifier.size(17.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Scan QR")
                    }
                    OutlinedButton(
                        onClick = {
                            pickImage.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                        },
                        modifier = Modifier.weight(1f).height(44.dp),
                    ) {
                        Icon(Icons.Filled.Image, contentDescription = null, modifier = Modifier.size(17.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Open image")
                    }
                }
                Button(
                    onClick = {
                        val text = invite
                        invite = ""
                        onEnrollInvite(text)
                    },
                    enabled = invite.length >= INVITE_MIN_LENGTH,
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) {
                    Text("Add server")
                }
            }
            (localIssue ?: inviteIssue)?.let { message ->
                Text(message, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            }
            TextButton(onClick = { manual = !manual }, modifier = Modifier.fillMaxWidth()) {
                Text(if (manual) "Use an invite instead" else "Advanced: use a server URL and access code")
            }
        }
    }
    if (scanning) {
        InviteQrScanner(
            accept = acceptsInvite,
            onScanned = { value ->
                scanning = false
                invite = value
                localIssue = null
            },
            onDismiss = { scanning = false },
        )
    }
}
