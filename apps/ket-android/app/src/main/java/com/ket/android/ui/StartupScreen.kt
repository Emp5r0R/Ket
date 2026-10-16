package com.ket.android.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.ket.android.KetStartupPolicy
import com.ket.android.R

/** Shown while a saved server reconnects at launch. */
@Composable
internal fun KetStartupScreen() {
    val presentation = KetStartupPolicy.presentation()
    val transition = rememberInfiniteTransition(label = "Ket startup")
    val pulse by transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1_800), RepeatMode.Restart),
        label = "Ket startup pulse",
    )
    val breathe by transition.animateFloat(
        initialValue = 0.96f,
        targetValue = 1.04f,
        animationSpec = infiniteRepeatable(tween(1_100), RepeatMode.Reverse),
        label = "Ket startup mark",
    )
    Box(
        modifier = Modifier
            .fillMaxSize()
            .semantics {
                contentDescription = "Ket is restoring the saved server"
                liveRegion = LiveRegionMode.Polite
            },
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Box(Modifier.size(132.dp), contentAlignment = Alignment.Center) {
                Canvas(Modifier.fillMaxSize()) {
                    repeat(2) { index ->
                        val progress = (pulse + index * 0.5f) % 1f
                        drawCircle(
                            color = KetCoral.copy(alpha = (1f - progress) * 0.20f),
                            radius = size.minDimension * (0.20f + progress * 0.30f),
                            style = Stroke(width = 1.2.dp.toPx()),
                        )
                    }
                }
                Image(
                    painter = painterResource(R.drawable.ket_mark),
                    contentDescription = null,
                    modifier = Modifier
                        .size(72.dp)
                        .graphicsLayer {
                            scaleX = breathe
                            scaleY = breathe
                        },
                )
            }
            Text(presentation.title, style = MaterialTheme.typography.titleMedium)
            Text(
                presentation.detail,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}
