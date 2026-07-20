package com.ket.android

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
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
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

private val MapBackground = Color(0xFF0B1519)
private val MapLand = Color(0xFF29474B)

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
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(2f)
            .background(MapBackground, RoundedCornerShape(4.dp))
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
                    onDrawBehind {
                        val grid = Color(0xFF1A2A2F)
                        for (x in 0..6) {
                            val position = size.width * x / 6f
                            drawLine(
                                grid,
                                Offset(position, 0f),
                                Offset(position, size.height),
                                0.7.dp.toPx(),
                            )
                        }
                        for (y in 0..3) {
                            val position = size.height * y / 3f
                            drawLine(
                                grid,
                                Offset(0f, position),
                                Offset(size.width, position),
                                0.7.dp.toPx(),
                            )
                        }
                        if (geometry != null) {
                            drawPath(landPath, MapLand)
                            drawPath(
                                landPath,
                                Color(0xFF3A5D61),
                                style = Stroke(0.6.dp.toPx()),
                            )
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
                if (connected) {
                    drawCircle(
                        KetTeal.copy(alpha = 0.16f * (1f - pulse.value / 2f)),
                        radius = (10 + pulse.value * 8).dp.toPx(),
                        center = center,
                    )
                }
                drawCircle(
                    if (connected) KetTeal else KetMuted,
                    radius = 4.5.dp.toPx(),
                    center = center,
                )
                drawCircle(
                    Color.White.copy(alpha = 0.85f),
                    radius = 1.5.dp.toPx(),
                    center = center,
                )
            }
        }
    }
}
