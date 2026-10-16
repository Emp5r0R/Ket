package com.ket.android.connection

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp

/**
 * Says why Ket is not using the protocols it tried first. Shows nothing when
 * the first protocol worked.
 */
@Composable
fun FallbackNotes(
    notes: List<TransportFallbackNote>,
    activeTransportName: String?,
    modifier: Modifier = Modifier,
) {
    if (notes.isEmpty()) return
    Surface(
        modifier = modifier
            .fillMaxWidth()
            .semantics { contentDescription = "Why Ket switched protocols" },
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(6.dp),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                fallbackHeading(activeTransportName),
                style = MaterialTheme.typography.labelMedium,
            )
            notes.forEach { note ->
                Text(
                    buildAnnotatedString {
                        withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(note.transportName) }
                        append(": ")
                        append(note.reason)
                    },
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}

internal fun fallbackHeading(activeTransportName: String?): String =
    if (activeTransportName.isNullOrBlank()) "Tried:" else "Using $activeTransportName because:"
