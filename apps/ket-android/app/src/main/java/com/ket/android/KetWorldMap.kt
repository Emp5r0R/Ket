package com.ket.android

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

@Composable
internal fun KetMap(location: AndroidNodeLocation?, connected: Boolean) {
    val context = LocalContext.current
    val geometry = remember {
        runCatching {
            context.resources.openRawResource(R.raw.world_land_110m).bufferedReader().use { reader ->
                WorldMapTopologyParser.parse(reader.readText())
            }
        }.getOrNull()
    }
    val transition = rememberInfiniteTransition(label = "map marker")
    val pulse = transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1_600), RepeatMode.Reverse),
        label = "map marker pulse",
    )
    val accent = MaterialTheme.colorScheme.primary
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val zoom by animateFloatAsState(
        targetValue = if (connected && location != null) 2.45f else 1f,
        animationSpec = tween(720),
        label = "map region zoom",
    )
    val dark = MaterialTheme.colorScheme.background.luminance() < 0.5f
    val mapBackground by animateColorAsState(
        targetValue = when {
            dark && connected -> Color(0xFF1B1416)
            dark -> Color(0xFF141916)
            else -> Color(0xFFF8FAF8)
        },
        animationSpec = tween(360),
        label = "map background",
    )
    val mapLand = when {
        dark && connected -> Color(0xFF3B3033)
        dark -> Color(0xFF303934)
        else -> Color(0xFFDDE3DF)
    }
    val mapBorder = if (dark) Color(0xFF4B5355) else Color(0xFFFFFFFF)
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(900f / 432f)
            .background(mapBackground, RoundedCornerShape(4.dp))
            .semantics {
                contentDescription = location?.let {
                    "Server location: ${it.displayName}"
                } ?: "Available server regions"
            },
    ) {
        Canvas(
            Modifier
                .fillMaxSize()
                .drawWithCache {
                    val horizontalPadding = 6.dp.toPx()
                    val verticalPadding = 5.dp.toPx()
                    val landPath = Path().apply {
                        fillType = PathFillType.EvenOdd
                        geometry?.polygons?.forEach { polygon ->
                            polygon.rings.forEach { ring ->
                                ring.forEachIndexed { index, point ->
                                    val projected = WorldMapProjection.project(
                                        point.longitude,
                                        point.latitude,
                                        size.width,
                                        size.height,
                                        horizontalPadding,
                                        verticalPadding,
                                    )
                                    if (index == 0) moveTo(projected.x, projected.y)
                                    else lineTo(projected.x, projected.y)
                                }
                                close()
                            }
                        }
                    }
                    val marker = location?.let {
                        WorldMapProjection.project(
                            it.longitude,
                            it.latitude,
                            size.width,
                            size.height,
                            horizontalPadding,
                            verticalPadding,
                        )
                    }
                    val progress = ((zoom - 1f) / 1.45f).coerceIn(0f, 1f)
                    val markerX = marker?.x ?: size.width / 2f
                    val markerY = marker?.y ?: size.height / 2f
                    val focusedX = markerX + (size.width / 2f - markerX) * progress
                    val focusedY = markerY + (size.height / 2f - markerY) * progress
                    val translateX = focusedX - markerX * zoom
                    val translateY = focusedY - markerY * zoom
                    onDrawBehind {
                        if (geometry != null) {
                            withTransform({
                                translate(translateX, translateY)
                                scale(zoom, zoom, Offset.Zero)
                            }) {
                                drawPath(landPath, mapLand)
                                drawPath(
                                    landPath,
                                    mapBorder,
                                    style = Stroke(0.6.dp.toPx() / zoom),
                                )
                            }
                        }
                    }
                },
        ) {}
        Canvas(Modifier.fillMaxSize()) {
            location?.let {
                val marker = WorldMapProjection.project(
                    it.longitude,
                    it.latitude,
                    size.width,
                    size.height,
                    6.dp.toPx(),
                    5.dp.toPx(),
                )
                val center = Offset(marker.x, marker.y)
                val progress = ((zoom - 1f) / 1.45f).coerceIn(0f, 1f)
                val focusedCenter = Offset(
                    center.x + (size.width / 2f - center.x) * progress,
                    center.y + (size.height / 2f - center.y) * progress,
                )
                if (connected) {
                    drawCircle(
                        accent.copy(alpha = 0.16f * (1f - pulse.value / 2f)),
                        radius = (10 + pulse.value * 8).dp.toPx(),
                        center = focusedCenter,
                    )
                }
                drawCircle(
                    if (connected) accent else muted,
                    radius = 4.5.dp.toPx(),
                    center = focusedCenter,
                )
                drawCircle(
                    Color.White.copy(alpha = 0.85f),
                    radius = 1.5.dp.toPx(),
                    center = focusedCenter,
                )
            }
        }
    }
}
