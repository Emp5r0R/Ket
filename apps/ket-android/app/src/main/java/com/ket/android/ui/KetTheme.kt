package com.ket.android.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import com.ket.android.AndroidNodeHealth
import com.ket.android.KetThemeMode

private val KetDarkInk = Color(0xFF101311)
private val KetDarkLiberatedInk = Color(0xFF170F11)
private val KetLightInk = Color(0xFFF4F6F3)
private val KetLightLiberatedInk = Color(0xFFFFF7F8)
internal val KetTeal = Color(0xFF13A884)
internal val KetCoral = Color(0xFFF24F5E)
private val KetDarkPanel = Color(0xFF181D1A)
private val KetDarkLiberatedPanel = Color(0xFF21171A)
private val KetLightPanel = Color(0xFFFFFFFF)
private val KetLightText = Color(0xFF18201D)
private val KetDarkMuted = Color(0xFF9AAEAE)
private val KetLightMuted = Color(0xFF66716C)
internal val KetHealthy = Color(0xFF2FB683)
internal val KetDegraded = Color(0xFFE0A030)
internal val KetSaturated = Color(0xFFE5484D)

internal fun healthTone(health: AndroidNodeHealth): Color = when (health) {
    AndroidNodeHealth.Healthy -> KetHealthy
    AndroidNodeHealth.Degraded -> KetDegraded
    AndroidNodeHealth.Saturated -> KetSaturated
}

/**
 * Ket's colours: teal while disconnected and coral once liberated, in light
 * and dark. The accent drives every selected state, including the navigation
 * indicator, so nothing falls back to Material's default purple.
 */
@Composable
internal fun KetTheme(
    connected: Boolean = false,
    mode: KetThemeMode = KetThemeMode.System,
    content: @Composable () -> Unit,
) {
    val dark = when (mode) {
        KetThemeMode.System -> isSystemInDarkTheme()
        KetThemeMode.Light -> false
        KetThemeMode.Dark -> true
    }
    val accent by animateColorAsState(
        targetValue = when {
            connected && dark -> Color(0xFFFF5B6B)
            connected -> KetCoral
            dark -> Color(0xFF32C69E)
            else -> KetTeal
        },
        animationSpec = tween(360),
        label = "connection accent",
    )
    val background by animateColorAsState(
        targetValue = when {
            dark && connected -> KetDarkLiberatedInk
            dark -> KetDarkInk
            connected -> KetLightLiberatedInk
            else -> KetLightInk
        },
        animationSpec = tween(360),
        label = "connection background",
    )
    val panel by animateColorAsState(
        targetValue = when {
            dark && connected -> KetDarkLiberatedPanel
            dark -> KetDarkPanel
            else -> KetLightPanel
        },
        animationSpec = tween(360),
        label = "connection surface",
    )
    val onAccent by animateColorAsState(
        targetValue = if (dark && !connected) KetDarkInk else Color.White,
        animationSpec = tween(240),
        label = "connection on-accent",
    )
    val colorScheme = if (dark) {
        darkColorScheme(
            primary = accent,
            onPrimary = onAccent,
            primaryContainer = accent.copy(alpha = 0.18f),
            onPrimaryContainer = accent,
            secondary = accent,
            secondaryContainer = accent.copy(alpha = 0.18f),
            onSecondaryContainer = accent,
            background = background,
            surface = panel,
            surfaceVariant = Color(0xFF222926),
            surfaceContainer = panel,
            error = KetSaturated,
            errorContainer = Color(0xFF3A1D20),
            onErrorContainer = Color(0xFFFFD9DB),
            onBackground = Color.White,
            onSurface = Color.White,
            onSurfaceVariant = KetDarkMuted,
            outline = Color(0xFF344145),
            outlineVariant = Color(0xFF2A3335),
        )
    } else {
        lightColorScheme(
            primary = accent,
            onPrimary = if (connected) Color.White else KetDarkInk,
            primaryContainer = accent.copy(alpha = 0.14f),
            onPrimaryContainer = accent,
            secondary = accent,
            secondaryContainer = accent.copy(alpha = 0.14f),
            onSecondaryContainer = accent,
            background = background,
            surface = panel,
            surfaceVariant = Color(0xFFEDF1EE),
            surfaceContainer = panel,
            error = Color(0xFFB42331),
            errorContainer = Color(0xFFFDE7E9),
            onErrorContainer = Color(0xFF6B0F18),
            onBackground = KetLightText,
            onSurface = KetLightText,
            onSurfaceVariant = KetLightMuted,
            outline = Color(0xFFCCD4CF),
            outlineVariant = Color(0xFFE1E7E3),
        )
    }
    MaterialTheme(colorScheme = colorScheme) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background,
            content = content,
        )
    }
}
