package com.ket.android.ui.connection

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.ket.android.TunnelPhase
import com.ket.android.connectionActionLabel
import com.ket.android.connectionPowerHeadline

private val workingPhases = setOf(TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting)

/**
 * The one button that matters: connect, cancel while connecting, or
 * disconnect. A ring pulses while Ket works, so it reads as busy, not stuck.
 */
@Composable
internal fun PowerButton(phase: TunnelPhase, detail: String, enabled: Boolean, onClick: () -> Unit) {
    val connected = phase == TunnelPhase.Connected
    val working = phase in workingPhases
    val container by animateColorAsState(
        when {
            connected -> MaterialTheme.colorScheme.primary
            working -> MaterialTheme.colorScheme.surface
            else -> MaterialTheme.colorScheme.primary
        },
        tween(300),
        label = "power container",
    )
    val content by animateColorAsState(
        when {
            connected -> MaterialTheme.colorScheme.onPrimary
            working -> MaterialTheme.colorScheme.primary
            else -> MaterialTheme.colorScheme.onPrimary
        },
        tween(300),
        label = "power content",
    )
    val ring = rememberInfiniteTransition(label = "power ring")
    val ringProgress by ring.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1_600), RepeatMode.Restart),
        label = "power ring progress",
    )
    val accent = MaterialTheme.colorScheme.primary
    Column(
        Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(Modifier.size(132.dp), contentAlignment = Alignment.Center) {
            if (working || connected) {
                Canvas(Modifier.size(132.dp)) {
                    val progress = if (working) ringProgress else 0.35f
                    drawCircle(
                        color = accent.copy(alpha = (if (working) 0.28f else 0.14f) * (1f - progress)),
                        radius = size.minDimension / 2f * (0.72f + 0.28f * progress),
                    )
                }
            }
            Button(
                onClick = onClick,
                enabled = enabled,
                modifier = Modifier
                    .size(96.dp)
                    .semantics { contentDescription = connectionActionLabel(phase) },
                shape = CircleShape,
                contentPadding = PaddingValues(0.dp),
                border = if (working) BorderStroke(2.dp, accent) else null,
                elevation = ButtonDefaults.buttonElevation(defaultElevation = if (working) 0.dp else 4.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = container,
                    contentColor = content,
                    disabledContainerColor = container.copy(alpha = 0.45f),
                    disabledContentColor = content.copy(alpha = 0.6f),
                ),
            ) {
                AnimatedContent(
                    targetState = phase,
                    transitionSpec = { fadeIn(tween(160)) togetherWith fadeOut(tween(100)) },
                    label = "power icon",
                ) { current ->
                    when {
                        current == TunnelPhase.Stopping ->
                            CircularProgressIndicator(Modifier.size(30.dp), color = content, strokeWidth = 2.5.dp)
                        current in workingPhases -> Icon(Icons.Filled.Close, null, Modifier.size(34.dp))
                        else -> Icon(Icons.Filled.PowerSettingsNew, null, Modifier.size(38.dp))
                    }
                }
            }
        }
        Text(connectionPowerHeadline(phase), style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
        Text(
            detail,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
        )
    }
}
