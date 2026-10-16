package com.ket.android.ui.connection

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.ket.android.TunnelPhase
import com.ket.android.TunnelSnapshot
import com.ket.android.phaseStatusLabel
import com.ket.android.ui.KetDegraded
import com.ket.android.ui.KetHealthy
import com.ket.android.ui.KetSaturated
import kotlinx.coroutines.delay

private val IslandInk = Color(0xFF0B0D0C)

/** On a dark screen the capsule is lifted a little and outlined so it still stands out. */
private val IslandInkOnDark = Color(0xFF1B1F1D)
private val IslandText = Color(0xFFF2F4F3)
private val IslandMuted = Color(0xFFA9B2AE)
private val WatchedRed = Color(0xFFFF4D5E)

/** How long the island stays open after the connection state changes. */
private const val EXPANDED_MILLIS = 3_500L

/**
 * The connection state as a dark capsule at the top of the screen, like a
 * phone's dynamic island. It opens with details whenever the state changes,
 * then folds back to a short status; a tap opens it again. While Ket is off,
 * a red dot pulses like a recording light: you are being watched.
 */
@Composable
internal fun StatusIsland(snapshot: TunnelSnapshot, disconnectedLabel: String, modifier: Modifier = Modifier) {
    val phase = snapshot.phase
    var expanded by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(phase) {
        expanded = true
        delay(EXPANDED_MILLIS)
        expanded = false
    }
    val title = if (phase == TunnelPhase.Disconnected) disconnectedLabel else phaseStatusLabel(phase)
    val detail = islandDetail(snapshot)
    val onDarkScreen = MaterialTheme.colorScheme.background.luminance() < 0.5f
    Surface(
        onClick = { expanded = !expanded },
        modifier = modifier
            .widthIn(max = 340.dp)
            .semantics {
                liveRegion = LiveRegionMode.Polite
                contentDescription = "$title. $detail"
            },
        color = if (onDarkScreen) IslandInkOnDark else IslandInk,
        contentColor = IslandText,
        shape = RoundedCornerShape(if (expanded) 22.dp else 50.dp),
        border = BorderStroke(1.dp, Color.White.copy(alpha = if (onDarkScreen) 0.22f else 0.06f)),
        shadowElevation = 6.dp,
    ) {
        Column(
            Modifier
                .animateContentSize(spring(dampingRatio = 0.72f, stiffness = Spring.StiffnessMediumLow))
                .padding(horizontal = 14.dp, vertical = if (expanded) 12.dp else 8.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(9.dp), verticalAlignment = Alignment.CenterVertically) {
                IslandIndicator(phase)
                AnimatedContent(
                    targetState = title,
                    transitionSpec = { fadeIn(tween(200)) togetherWith fadeOut(tween(120)) },
                    label = "island title",
                ) { text ->
                    Text(text, style = MaterialTheme.typography.labelLarge, maxLines = 1)
                }
            }
            AnimatedVisibility(
                visible = expanded,
                enter = fadeIn(tween(220, delayMillis = 80)) + expandVertically(),
                exit = fadeOut(tween(120)) + shrinkVertically(),
            ) {
                Text(
                    detail,
                    modifier = Modifier.padding(top = 6.dp, start = 2.dp),
                    color = IslandMuted,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun IslandIndicator(phase: TunnelPhase) {
    Box(Modifier.size(16.dp), contentAlignment = Alignment.Center) {
        when (phase) {
            TunnelPhase.Disconnected -> {
                val pulse = rememberInfiniteTransition(label = "watched light")
                val glow by pulse.animateFloat(
                    initialValue = 0.25f,
                    targetValue = 1f,
                    animationSpec = infiniteRepeatable(tween(900), RepeatMode.Reverse),
                    label = "watched light glow",
                )
                Canvas(Modifier.size(16.dp)) {
                    drawCircle(WatchedRed.copy(alpha = 0.30f * glow), radius = size.minDimension / 2f)
                    drawCircle(WatchedRed, radius = size.minDimension / 4f)
                }
            }
            TunnelPhase.Connected -> Icon(Icons.Filled.Shield, contentDescription = null, tint = KetHealthy, modifier = Modifier.size(16.dp))
            TunnelPhase.Failed -> Icon(Icons.Filled.Warning, contentDescription = null, tint = KetSaturated, modifier = Modifier.size(16.dp))
            else -> CircularProgressIndicator(Modifier.size(14.dp), color = KetDegraded, strokeWidth = 2.dp)
        }
    }
}

/** The second line the island shows when open. */
internal fun islandDetail(snapshot: TunnelSnapshot): String = when (snapshot.phase) {
    TunnelPhase.Disconnected -> "Ket is off. Your network can see what you do."
    TunnelPhase.Connected -> "Protected with ${snapshot.transportName}"
    TunnelPhase.Enrolling -> "Adding your server"
    TunnelPhase.Connecting, TunnelPhase.Reconnecting -> snapshot.message.ifBlank { "Finding a route that works here" }
    TunnelPhase.Stopping -> "Closing the tunnel and restoring your network"
    TunnelPhase.Failed -> snapshot.message.ifBlank { "Could not connect. Open diagnostics for details." }
}
