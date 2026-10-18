package com.ket.android.ui.metrics

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/**
 * Download speed as a filled line and upload speed as a thinner one, both scaled from zero to
 * the highest speed shown.
 */
@Composable
internal fun SpeedChart(rates: List<TrafficRate>, download: Color, upload: Color, grid: Color, modifier: Modifier = Modifier) {
    val top = rates.maxOfOrNull { maxOf(it.received, it.sent) }?.coerceAtLeast(1.0) ?: 1.0
    Canvas(
        modifier
            .fillMaxWidth()
            .height(120.dp)
            .semantics { contentDescription = "Download and upload speed over the last minute" },
    ) {
        val inset = 4.dp.toPx()
        for (line in 1..3) {
            val y = size.height * line / 4f
            drawLine(grid, Offset(0f, y), Offset(size.width, y), strokeWidth = 1.dp.toPx())
        }
        if (rates.size < 2) return@Canvas
        fun point(index: Int, value: Double) = Offset(
            x = inset + (size.width - inset * 2) * index / (rates.size - 1),
            y = size.height - inset - ((value / top) * (size.height - inset * 2)).toFloat(),
        )
        fun line(values: List<Double>) = Path().apply {
            values.forEachIndexed { index, value ->
                val at = point(index, value)
                if (index == 0) moveTo(at.x, at.y) else lineTo(at.x, at.y)
            }
        }
        val received = rates.map(TrafficRate::received)
        val receivedLine = line(received)
        val area = Path().apply {
            addPath(receivedLine)
            lineTo(point(received.lastIndex, 0.0).x, size.height)
            lineTo(point(0, 0.0).x, size.height)
            close()
        }
        drawPath(area, Brush.verticalGradient(listOf(download.copy(alpha = 0.32f), download.copy(alpha = 0f))))
        drawPath(receivedLine, download, style = Stroke(2.5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
        drawPath(line(rates.map(TrafficRate::sent)), upload, style = Stroke(1.5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}
