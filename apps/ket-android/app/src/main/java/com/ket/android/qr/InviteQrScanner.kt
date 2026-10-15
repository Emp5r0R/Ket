package com.ket.android.qr

import android.Manifest
import android.content.pm.PackageManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Size
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.view.CameraController
import androidx.camera.view.LifecycleCameraController
import androidx.camera.view.PreviewView
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.SecureFlagPolicy
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import kotlinx.coroutines.delay
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

private const val SCAN_TIMEOUT_MS = 60_000L
private const val FRAME_INTERVAL_MS = 250L

/**
 * Camera invite scanner (adapted from Abyssal). Only text that [accept] verifies is delivered;
 * the camera stops on delivery, dismissal, timeout, or when the app leaves the foreground.
 */
@Composable
internal fun InviteQrScanner(
    accept: (String) -> Boolean,
    onScanned: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    var granted by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    var requested by remember { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        granted = it
        requested = true
    }
    LaunchedEffect(Unit) {
        if (!granted) permission.launch(Manifest.permission.CAMERA)
    }
    val dismiss by rememberUpdatedState(onDismiss)
    LaunchedEffect(Unit) {
        delay(SCAN_TIMEOUT_MS)
        dismiss()
    }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(securePolicy = SecureFlagPolicy.SecureOn)) {
        Surface(
            shape = RoundedCornerShape(7.dp),
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        ) {
            Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Scan a Ket invite", style = MaterialTheme.typography.titleMedium)
                if (granted) {
                    CameraPreview(accept, onScanned, onDismiss)
                } else {
                    Text(
                        if (requested) {
                            "Camera unavailable. Paste the invite or open a QR image instead."
                        } else {
                            "Waiting for camera permission..."
                        },
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                OutlinedButton(onClick = onDismiss, modifier = Modifier.fillMaxWidth().height(44.dp)) {
                    Text("Close camera")
                }
            }
        }
    }
}

@Composable
private fun CameraPreview(
    accept: (String) -> Boolean,
    onScanned: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current
    val scanned by rememberUpdatedState(onScanned)
    val dismiss by rememberUpdatedState(onDismiss)
    val accepted by rememberUpdatedState(accept)
    val preview = remember(context) {
        PreviewView(context).apply { implementationMode = PreviewView.ImplementationMode.COMPATIBLE }
    }
    var status by remember { mutableStateOf("Point the camera at the invite QR code.") }
    DisposableEffect(context, lifecycle, preview) {
        val active = AtomicBoolean(true)
        val delivered = AtomicBoolean(false)
        val handler = Handler(Looper.getMainLooper())
        val executor = Executors.newSingleThreadExecutor()
        var lastFrame = 0L
        var controller: LifecycleCameraController? = null
        fun stop() {
            if (!active.getAndSet(false)) return
            controller?.clearImageAnalysisAnalyzer()
            controller?.unbind()
            preview.controller = null
            handler.removeCallbacksAndMessages(null)
            executor.shutdownNow()
        }
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_PAUSE || event == Lifecycle.Event.ON_STOP) {
                stop()
                dismiss()
            }
        }
        lifecycle.lifecycle.addObserver(observer)
        try {
            controller = LifecycleCameraController(context).apply {
                setEnabledUseCases(CameraController.IMAGE_ANALYSIS)
                cameraSelector = CameraSelector.DEFAULT_BACK_CAMERA
                imageAnalysisResolutionSelector = ResolutionSelector.Builder()
                    .setResolutionStrategy(
                        ResolutionStrategy(Size(960, 720), ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER),
                    )
                    .build()
                imageAnalysisBackpressureStrategy = ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST
                setImageAnalysisAnalyzer(executor) { frame ->
                    var bytes: ByteArray? = null
                    try {
                        val now = SystemClock.elapsedRealtime()
                        if (!active.get() || delivered.get() || now - lastFrame < FRAME_INTERVAL_MS) return@setImageAnalysisAnalyzer
                        lastFrame = now
                        val plane = frame.planes.firstOrNull() ?: return@setImageAnalysisAnalyzer
                        bytes = InviteQrDecoder.copyPlane(plane.buffer, frame.width, frame.height, plane.rowStride, plane.pixelStride)
                        val value = InviteQrDecoder.decodeLuminance(bytes, frame.width, frame.height)
                            ?: return@setImageAnalysisAnalyzer
                        val valid = accepted(value)
                        handler.post {
                            if (!active.get()) return@post
                            if (valid && delivered.compareAndSet(false, true)) {
                                stop()
                                scanned(value)
                            } else {
                                status = "That QR code is not a valid Ket invite."
                            }
                        }
                    } catch (_: Exception) {
                        handler.post { if (active.get()) status = "That QR code is not a valid Ket invite." }
                    } finally {
                        bytes?.fill(0)
                        frame.close()
                    }
                }
                preview.controller = this
                bindToLifecycle(lifecycle)
            }
        } catch (_: Exception) {
            stop()
            status = "Camera unavailable. Paste the invite or open a QR image instead."
        }
        onDispose {
            lifecycle.lifecycle.removeObserver(observer)
            stop()
        }
    }
    AndroidView(
        factory = { preview },
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(4f / 3f)
            .clip(RoundedCornerShape(7.dp))
            .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(7.dp)),
    )
    Text(status, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
}
