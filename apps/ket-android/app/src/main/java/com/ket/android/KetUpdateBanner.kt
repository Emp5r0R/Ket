package com.ket.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date

enum class UpdateTone { Info, Warning, Danger }

/** What the app shows for the server's update advice. */
data class UpdateNotice(
    val tone: UpdateTone,
    val title: String,
    val detail: String?,
    /** Optional and recommended updates can be hidden until the next version. */
    val dismissible: Boolean,
    val downloadUrl: String,
    val latestVersion: String,
)

internal object UpdateNotices {
    fun describe(
        update: AndroidClientUpdate?,
        formatDay: (Long) -> String = ::formatDay,
    ): UpdateNotice? {
        if (update == null) return null
        val (tone, title, dismissible) = when (update.status) {
            ClientUpdateStatus.Current -> return null
            ClientUpdateStatus.Optional -> Triple(UpdateTone.Info, "Ket ${update.latestVersion} is available", true)
            ClientUpdateStatus.Recommended -> Triple(UpdateTone.Info, "Update recommended: Ket ${update.latestVersion}", true)
            ClientUpdateStatus.Security -> Triple(
                UpdateTone.Warning,
                update.requiredAfterEpochSeconds
                    ?.let { "Security update: install Ket ${update.latestVersion} before ${formatDay(it)}" }
                    ?: "Security update: install Ket ${update.latestVersion} soon",
                false,
            )
            ClientUpdateStatus.Required -> Triple(
                UpdateTone.Danger,
                "Update required: this server needs Ket ${update.latestVersion}",
                false,
            )
        }
        return UpdateNotice(tone, title, update.summary, dismissible, update.downloadUrl, update.latestVersion)
    }

    private fun formatDay(epochSeconds: Long): String =
        DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(epochSeconds * 1_000))
}

@Composable
internal fun KetUpdateBanner(update: AndroidClientUpdate?) {
    val notice = UpdateNotices.describe(update) ?: return
    var dismissedVersion by rememberSaveable { mutableStateOf<String?>(null) }
    if (notice.dismissible && dismissedVersion == notice.latestVersion) return
    val uriHandler = LocalUriHandler.current
    val (container, content) = when (notice.tone) {
        UpdateTone.Info -> MaterialTheme.colorScheme.secondaryContainer to MaterialTheme.colorScheme.onSecondaryContainer
        UpdateTone.Warning -> MaterialTheme.colorScheme.tertiaryContainer to MaterialTheme.colorScheme.onTertiaryContainer
        UpdateTone.Danger -> MaterialTheme.colorScheme.errorContainer to MaterialTheme.colorScheme.onErrorContainer
    }
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .semantics {
                liveRegion = if (notice.tone == UpdateTone.Danger) LiveRegionMode.Assertive else LiveRegionMode.Polite
            },
        color = container,
        shape = RoundedCornerShape(6.dp),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(9.dp)) {
                Icon(
                    if (notice.tone == UpdateTone.Info) Icons.Filled.SystemUpdate else Icons.Filled.Warning,
                    contentDescription = null,
                    tint = content,
                )
                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(notice.title, color = content, style = MaterialTheme.typography.titleSmall)
                    notice.detail?.let {
                        Text(it, color = content, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
            Row {
                TextButton(onClick = { runCatching { uriHandler.openUri(notice.downloadUrl) } }) {
                    Icon(Icons.Filled.SystemUpdate, contentDescription = null, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Get the update")
                }
                if (notice.dismissible) {
                    TextButton(onClick = { dismissedVersion = notice.latestVersion }) {
                        Text("Not now")
                    }
                }
            }
        }
    }
}
