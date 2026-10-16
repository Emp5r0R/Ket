package com.ket.android

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Analytics
import androidx.compose.material.icons.filled.Book
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.CloudDownload
import androidx.compose.material.icons.filled.CloudUpload
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.DarkMode
import androidx.compose.material.icons.filled.DeleteOutline
import androidx.compose.material.icons.filled.LightMode
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material.icons.filled.NetworkCheck
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.ket.android.invite.InviteReader
import com.ket.android.invite.NodePublicKey
import kotlinx.coroutines.delay
import com.ket.android.splittunnel.InstalledApp
import com.ket.android.splittunnel.SplitTunnelPolicy
import com.ket.android.connection.ConnectionOptions
import com.ket.android.connection.ConnectionOptionsSection
import com.ket.android.connection.FallbackNotes
import com.ket.android.splittunnel.SplitTunnelSection

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
private val KetHealthy = Color(0xFF4BD29F)
private val KetDegraded = Color(0xFFF0B44D)
private val KetSaturated = Color(0xFFFF6B66)

internal enum class KetThemeMode(val storedName: String, val displayName: String) {
    System("system", "System"),
    Light("light", "Light"),
    Dark("dark", "Dark");

    companion object {
        fun fromStored(value: String?): KetThemeMode = entries.firstOrNull { it.storedName == value } ?: System
    }
}

internal enum class KetAppDestination(val storedName: String, val displayName: String) {
    Connection("connection", "Connection"),
    Metrics("metrics", "Metrics"),
    Protocols("protocols", "Protocols"),
    Settings("settings", "Settings");

    companion object {
        fun fromStored(value: String?): KetAppDestination =
            entries.firstOrNull { it.storedName == value } ?: Connection
    }
}

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
        contentAlignment = androidx.compose.ui.Alignment.Center,
    ) {
        Column(
            horizontalAlignment = androidx.compose.ui.Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Box(Modifier.size(132.dp), contentAlignment = androidx.compose.ui.Alignment.Center) {
                Canvas(Modifier.fillMaxSize()) {
                    repeat(2) { index ->
                        val progress = (pulse + index * 0.5f) % 1f
                        drawCircle(
                            color = KetCoral.copy(alpha = (1f - progress) * 0.20f),
                            radius = size.minDimension * (0.20f + progress * 0.30f),
                            style = androidx.compose.ui.graphics.drawscope.Stroke(width = 1.2.dp.toPx()),
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
            background = background,
            surface = panel,
            secondary = accent,
            error = KetSaturated,
            onBackground = Color.White,
            onSurface = Color.White,
            onSurfaceVariant = KetDarkMuted,
            outline = Color(0xFF344145),
        )
    } else {
        lightColorScheme(
            primary = accent,
            onPrimary = if (connected) Color.White else KetDarkInk,
            background = background,
            surface = panel,
            secondary = accent,
            error = Color(0xFFB42331),
            onBackground = KetLightText,
            onSurface = KetLightText,
            onSurfaceVariant = KetLightMuted,
            outline = Color(0xFFCCD4CF),
        )
    }
    MaterialTheme(
        colorScheme = colorScheme,
    ) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background,
            content = content,
        )
    }
}

@Composable
internal fun KetApp(
    snapshot: TunnelSnapshot,
    initialServerUrl: String = "",
    initialAccessCode: String = "",
    initialPreferredProtocol: KetProtocol? = null,
    initialNodeKey: NodePublicKey? = null,
    initialAccessExpiresAtEpochSeconds: Long? = null,
    onAccessExpired: () -> Unit = {},
    themeMode: KetThemeMode = KetThemeMode.System,
    onThemeModeChange: (KetThemeMode) -> Unit = {},
    splitTunnel: SplitTunnelPolicy = SplitTunnelPolicy.AllApps,
    onSplitTunnelChange: (SplitTunnelPolicy) -> Unit = {},
    connectionOptions: ConnectionOptions = ConnectionOptions(),
    onConnectionOptionsChange: (ConnectionOptions) -> Unit = {},
    loadInstalledApps: () -> List<InstalledApp> = { emptyList() },
    onForgetProfile: () -> Unit = {},
    diagnosticReport: String = "Ket Android connection diagnostics\nevents:\nnone\n",
    inviteReader: InviteReader = appInviteReader,
    onConnect: (String, String, KetProtocol?, NodePublicKey?) -> Unit,
    onDisconnect: () -> Unit,
) {
    var serverUrl by rememberSaveable { mutableStateOf(initialServerUrl) }
    var accessCode by rememberSaveable { mutableStateOf(initialAccessCode) }
    // The node key is public; it pins every enrollment of an invite-based profile.
    var nodeKeyText by rememberSaveable { mutableStateOf(initialNodeKey?.base64Url) }
    var inviteIssue by remember { mutableStateOf<String?>(null) }
    var preferredProtocolWireName by rememberSaveable {
        mutableStateOf(initialPreferredProtocol?.wireName)
    }
    var accessExpiresAtEpochSeconds by rememberSaveable {
        mutableStateOf(initialAccessExpiresAtEpochSeconds)
    }
    var accessRemainingSeconds by rememberSaveable { mutableStateOf<Long?>(null) }
    var profileSaved by rememberSaveable {
        mutableStateOf(
            initialServerUrl.isNotBlank() &&
                initialAccessCode.length == 32,
        )
    }
    var disconnectedVariant by rememberSaveable { mutableIntStateOf(0) }
    var diagnosticsVisible by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(snapshot.accessExpiresAtEpochSeconds) {
        snapshot.accessExpiresAtEpochSeconds?.let {
            accessExpiresAtEpochSeconds = it
            profileSaved = true
        }
    }
    LaunchedEffect(accessExpiresAtEpochSeconds) {
        val expiry = accessExpiresAtEpochSeconds ?: run {
            accessRemainingSeconds = null
            return@LaunchedEffect
        }
        while (true) {
            val remaining = (expiry - System.currentTimeMillis() / 1_000).coerceAtLeast(0)
            accessRemainingSeconds = remaining
            if (remaining == 0L) {
                serverUrl = ""
                accessCode = ""
                nodeKeyText = null
                accessExpiresAtEpochSeconds = null
                profileSaved = false
                onAccessExpired()
                break
            }
            delay(minOf(remaining * 1_000, 30_000))
        }
    }
    var destinationWireName by rememberSaveable {
        mutableStateOf(KetAppDestination.Connection.storedName)
    }
    var guideProtocolWireName by rememberSaveable {
        mutableStateOf((initialPreferredProtocol ?: KetProtocol.Stealth).wireName)
    }
    val preferredProtocol = preferredProtocolWireName?.let(KetProtocol::fromWireName)
    val destination = KetAppDestination.fromStored(destinationWireName)
    val contentScrollState = remember(destination) { ScrollState(0) }
    val guideProtocol = KetProtocol.fromWireName(guideProtocolWireName) ?: KetProtocol.Stealth
    val connected = snapshot.phase == TunnelPhase.Connected
    val busy = snapshot.phase in setOf(
        TunnelPhase.Enrolling,
        TunnelPhase.Connecting,
        TunnelPhase.Reconnecting,
        TunnelPhase.Stopping,
    )
    LaunchedEffect(connected, busy) {
        disconnectedVariant = 0
        if (!connected && !busy) {
            while (true) {
                delay(5_200)
                disconnectedVariant = (disconnectedVariant + 1) % 2
            }
        }
    }
    LaunchedEffect(snapshot.phase, snapshot.message) {
        if (snapshot.phase == TunnelPhase.Failed) {
            diagnosticsVisible = true
        }
    }
    BackHandler(enabled = destination != KetAppDestination.Connection) {
        destinationWireName = KetAppDestination.Connection.storedName
    }
    val forgetProfile = {
        serverUrl = ""
        accessCode = ""
        nodeKeyText = null
        accessExpiresAtEpochSeconds = null
        accessRemainingSeconds = null
        profileSaved = false
        onForgetProfile()
    }
    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
        bottomBar = {
            NavigationBar(containerColor = MaterialTheme.colorScheme.surface) {
                KetAppDestination.entries.forEach { item ->
                    NavigationBarItem(
                        selected = destination == item,
                        onClick = { destinationWireName = item.storedName },
                        icon = {
                            Icon(
                                when (item) {
                                    KetAppDestination.Connection -> Icons.Filled.Shield
                                    KetAppDestination.Metrics -> Icons.Filled.Analytics
                                    KetAppDestination.Protocols -> Icons.Filled.Book
                                    KetAppDestination.Settings -> Icons.Filled.Settings
                                },
                                contentDescription = null,
                            )
                        },
                        label = { Text(item.displayName) },
                    )
                }
            }
        },
    ) { contentPadding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(contentPadding)
                .animateContentSize(animationSpec = tween(260))
                .verticalScroll(contentScrollState)
                .padding(horizontal = 20.dp, vertical = 18.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                    Image(
                        painter = painterResource(R.drawable.ket_mark),
                        contentDescription = null,
                        modifier = Modifier.size(34.dp),
                    )
                    Spacer(Modifier.width(8.dp))
                    Text("Ket", style = MaterialTheme.typography.titleMedium)
                }
                Text(
                    destination.displayName,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelLarge,
                )
            }
            AnimatedContent(
                targetState = destination,
                transitionSpec = {
                    (fadeIn(tween(220)) + scaleIn(tween(220), initialScale = 0.98f)) togetherWith
                        fadeOut(tween(120))
                },
                label = "app destination",
            ) { currentDestination ->
                Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    when (currentDestination) {
                        KetAppDestination.Connection -> {
                            ConnectionRouteHeader(snapshot, disconnectedVariant)
                            ServerMap(snapshot, connected)
                            if (!profileSaved && !connected && !busy) {
                                EnrollmentControls(
                                    serverUrl = serverUrl,
                                    accessCode = accessCode,
                                    inviteIssue = inviteIssue,
                                    acceptsInvite = { text ->
                                        runCatching { inviteReader.read(text, System.currentTimeMillis() / 1_000) }.isSuccess
                                    },
                                    onServerUrlChange = { serverUrl = it },
                                    onAccessCodeChange = { value ->
                                        accessCode = value.filter(Char::isLetterOrDigit).take(32)
                                    },
                                    onEnrollInvite = { text ->
                                        runCatching { inviteReader.read(text, System.currentTimeMillis() / 1_000) }
                                            .onSuccess { target ->
                                                inviteIssue = null
                                                serverUrl = target.serverUrl
                                                accessCode = target.accessCode
                                                nodeKeyText = target.nodeKey.base64Url
                                                onConnect(target.serverUrl, target.accessCode, null, target.nodeKey)
                                            }
                                            .onFailure { inviteIssue = inviteFailureMessage(it) }
                                    },
                                    onEnrollManual = {
                                        inviteIssue = null
                                        nodeKeyText = null
                                        onConnect(serverUrl, accessCode, null, null)
                                    },
                                )
                            } else {
                                ConnectionControlPanel(
                                    snapshot = snapshot,
                                    serverUrl = serverUrl,
                                    accessRemainingSeconds = accessRemainingSeconds,
                                    preferredProtocol = preferredProtocol,
                                    busy = busy,
                                    canStartConnection = serverUrl.isNotBlank() &&
                                        accessCode.matches(Regex("^[A-Za-z0-9]{32}$")),
                                    onPreferredProtocolChange = { preferredProtocolWireName = it?.wireName },
                                    onLearnMore = {
                                        guideProtocolWireName = (preferredProtocol ?: KetProtocol.Stealth).wireName
                                        destinationWireName = KetAppDestination.Protocols.storedName
                                    },
                                    onToggle = {
                                        if (connected || busy) {
                                            onDisconnect()
                                        } else {
                                            onConnect(
                                                serverUrl,
                                                accessCode,
                                                preferredProtocol,
                                                nodeKeyText?.let(NodePublicKey::fromBase64Url),
                                            )
                                        }
                                    },
                                )
                            }
                            KetUpdateBanner(snapshot.clientUpdate)
                            ConnectionMessage(snapshot) { diagnosticsVisible = true }
                        }
                        KetAppDestination.Metrics -> {
                            ViewHeading("Live route metrics", "Traffic, capacity, and node health")
                            snapshot.node?.let { node ->
                                NodeCapacity(node)
                                Telemetry(snapshot, node)
                            } ?: EmptyDestination("Connect to a server to collect route metrics.")
                        }
                        KetAppDestination.Protocols -> {
                            ViewHeading("Protocol guide", "Choose the route that fits the current network")
                            ProtocolGuideContent(
                                selected = guideProtocol,
                                onSelect = { guideProtocolWireName = it.wireName },
                            )
                        }
                        KetAppDestination.Settings -> {
                            ViewHeading("Settings", "Appearance, apps, saved access, and troubleshooting")
                            ThemeModeSelector(themeMode, onThemeModeChange)
                            SplitTunnelSection(
                                policy = splitTunnel,
                                connected = snapshot.phase == TunnelPhase.Connected,
                                onChange = onSplitTunnelChange,
                                loadApps = loadInstalledApps,
                            )
                            ConnectionOptionsSection(connectionOptions, onConnectionOptionsChange)
                            if (profileSaved) {
                                SavedProfile(serverUrl, accessRemainingSeconds, forgetProfile)
                            }
                            OutlinedButton(
                                onClick = { diagnosticsVisible = true },
                                modifier = Modifier.fillMaxWidth().height(48.dp),
                            ) {
                                Icon(Icons.Filled.BugReport, contentDescription = null, modifier = Modifier.size(17.dp))
                                Spacer(Modifier.width(7.dp))
                                Text("Connection diagnostics")
                            }
                        }
                    }
                }
            }
        }
    }
    if (diagnosticsVisible) {
        KetDiagnosticsDialog(
            report = diagnosticReport,
            onDismissRequest = { diagnosticsVisible = false },
        )
    }
}

@Composable
private fun ConnectionRouteHeader(snapshot: TunnelSnapshot, disconnectedVariant: Int) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(
                "SECURE ROUTE",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.labelSmall,
            )
            Text(
                connectionLocationHeading(snapshot.node),
                style = MaterialTheme.typography.headlineMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        PhaseStatus(
            phase = snapshot.phase,
            disconnectedLabel = disconnectedSurveillanceLabel(disconnectedVariant),
        )
    }
}

@Composable
private fun ConnectionControlPanel(
    snapshot: TunnelSnapshot,
    serverUrl: String,
    accessRemainingSeconds: Long?,
    preferredProtocol: KetProtocol?,
    busy: Boolean,
    canStartConnection: Boolean,
    onPreferredProtocolChange: (KetProtocol?) -> Unit,
    onLearnMore: () -> Unit,
    onToggle: () -> Unit,
) {
    val node = snapshot.node
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(7.dp),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = androidx.compose.ui.Alignment.Top,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(
                        "CURRENT NODE",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.labelSmall,
                    )
                    Text(
                        node?.displayName ?: "Saved server",
                        style = MaterialTheme.typography.titleLarge,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                node?.let { NodeHealthPill(it.health) }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.65f))
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                NodeFact(
                    Icons.Filled.LocationOn,
                    node?.location?.displayName ?: serverUrl,
                )
                NodeFact(
                    Icons.Filled.Schedule,
                    "${formatDuration(accessRemainingSeconds)} access left",
                )
                NodeFact(
                    Icons.Filled.NetworkCheck,
                    snapshot.transportName.ifBlank { "Automatic transport" },
                )
            }
            FallbackNotes(
                notes = snapshot.fallbacks,
                activeTransportName = snapshot.transportName.takeIf { snapshot.phase == TunnelPhase.Connected },
            )
            HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.65f))
            ProtocolPreferenceSelector(
                selected = preferredProtocol,
                onSelect = onPreferredProtocolChange,
                onLearnMore = onLearnMore,
                enabled = !busy && snapshot.phase != TunnelPhase.Connected,
            )
            PowerControl(
                phase = snapshot.phase,
                enabled = snapshot.phase != TunnelPhase.Stopping &&
                    (snapshot.phase == TunnelPhase.Connected || busy || canStartConnection),
                onClick = onToggle,
            )
        }
    }
}

@Composable
private fun NodeHealthPill(health: AndroidNodeHealth) {
    val tone = when (health) {
        AndroidNodeHealth.Healthy -> KetHealthy
        AndroidNodeHealth.Degraded -> KetDegraded
        AndroidNodeHealth.Saturated -> KetSaturated
    }
    Surface(
        color = tone.copy(alpha = 0.14f),
        contentColor = tone,
        shape = CircleShape,
    ) {
        Text(
            health.displayName,
            modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
            style = MaterialTheme.typography.labelSmall,
        )
    }
}

@Composable
private fun NodeFact(icon: ImageVector, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            modifier = Modifier.size(17.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun PowerControl(phase: TunnelPhase, enabled: Boolean, onClick: () -> Unit) {
    val connected = phase == TunnelPhase.Connected
    val cancellable = phase in setOf(
        TunnelPhase.Enrolling,
        TunnelPhase.Connecting,
        TunnelPhase.Reconnecting,
    )
    val container = when {
        connected -> MaterialTheme.colorScheme.primary
        cancellable -> MaterialTheme.colorScheme.errorContainer
        else -> MaterialTheme.colorScheme.background
    }
    val content = when {
        connected -> MaterialTheme.colorScheme.onPrimary
        cancellable -> MaterialTheme.colorScheme.error
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = androidx.compose.ui.Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        Button(
            onClick = onClick,
            enabled = enabled,
            modifier = Modifier
                .size(88.dp)
                .border(1.dp, content.copy(alpha = 0.55f), CircleShape)
                .semantics { contentDescription = connectionActionLabel(phase) },
            shape = CircleShape,
            contentPadding = PaddingValues(0.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = container,
                contentColor = content,
                disabledContainerColor = container.copy(alpha = 0.55f),
                disabledContentColor = content.copy(alpha = 0.55f),
            ),
        ) {
            AnimatedContent(
                targetState = phase,
                transitionSpec = { fadeIn(tween(160)) togetherWith fadeOut(tween(100)) },
                label = "connection power",
            ) { currentPhase ->
                if (currentPhase in setOf(
                        TunnelPhase.Enrolling,
                        TunnelPhase.Connecting,
                        TunnelPhase.Reconnecting,
                        TunnelPhase.Stopping,
                    )
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(29.dp),
                        color = content,
                        strokeWidth = 2.2.dp,
                    )
                } else {
                    Icon(
                        Icons.Filled.PowerSettingsNew,
                        contentDescription = null,
                        modifier = Modifier.size(31.dp),
                    )
                }
            }
        }
        Text(connectionPowerHeadline(phase), style = MaterialTheme.typography.titleMedium)
        Text(
            connectionPowerDetail(phase),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun ConnectionMessage(snapshot: TunnelSnapshot, onOpenDiagnostics: () -> Unit) {
    if (snapshot.message.isBlank()) return
    if (snapshot.phase == TunnelPhase.Failed) {
        FailureNotice(snapshot.message, onOpenDiagnostics)
    } else {
        Text(
            snapshot.message,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun ViewHeading(title: String, subtitle: String) {
    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Text(title, style = MaterialTheme.typography.headlineMedium)
        Text(
            subtitle,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun EmptyDestination(message: String) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(6.dp),
    ) {
        Text(
            message,
            modifier = Modifier.padding(18.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun FailureNotice(message: String, onOpenDiagnostics: () -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.errorContainer,
        shape = RoundedCornerShape(6.dp),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(9.dp)) {
                Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.colorScheme.error)
                Text(message, color = MaterialTheme.colorScheme.onErrorContainer, style = MaterialTheme.typography.bodySmall)
            }
            TextButton(onClick = onOpenDiagnostics) {
                Icon(Icons.Filled.BugReport, contentDescription = null, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text("Troubleshoot")
            }
        }
    }
}

@Composable
private fun ThemeModeSelector(
    selected: KetThemeMode,
    onSelect: (KetThemeMode) -> Unit,
) {
    val icons = listOf(Icons.Default.Computer, Icons.Default.LightMode, Icons.Default.DarkMode)
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        KetThemeMode.entries.forEachIndexed { index, mode ->
            OutlinedButton(
                onClick = { onSelect(mode) },
                modifier = Modifier.weight(1f).height(40.dp),
                colors = ButtonDefaults.outlinedButtonColors(
                    containerColor = if (selected == mode) {
                        MaterialTheme.colorScheme.primary.copy(alpha = 0.14f)
                    } else {
                        Color.Transparent
                    },
                    contentColor = if (selected == mode) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                ),
            ) {
                Icon(icons[index], contentDescription = null, modifier = Modifier.size(15.dp))
                Spacer(Modifier.width(6.dp))
                Text(mode.displayName, maxLines = 1)
            }
        }
    }
}

@Composable
private fun SavedProfile(
    serverUrl: String,
    accessRemainingSeconds: Long?,
    onForget: () -> Unit,
) {
    Surface(
        color = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(6.dp),
        tonalElevation = 1.dp,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
        ) {
            Icon(
                Icons.Default.Link,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.width(11.dp))
            Column(Modifier.weight(1f)) {
                Text("Saved server", style = MaterialTheme.typography.labelMedium)
                Text(
                    serverUrl,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                accessRemainingSeconds?.let {
                    Text(
                        "${formatDuration(it)} access left",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
            TextButton(onClick = onForget) {
                Icon(Icons.Default.DeleteOutline, contentDescription = null, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(5.dp))
                Text("Forget")
            }
        }
    }
}

@Composable
private fun ProtocolPreferenceSelector(
    selected: KetProtocol?,
    onSelect: (KetProtocol?) -> Unit,
    onLearnMore: () -> Unit,
    enabled: Boolean = true,
) {
    var expanded by androidx.compose.runtime.remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(
                "PREFERRED PROTOCOL",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.labelSmall,
            )
            TextButton(onClick = onLearnMore, enabled = enabled) { Text("Learn more") }
        }
        Box(Modifier.fillMaxWidth()) {
            OutlinedButton(
                onClick = { expanded = true },
                enabled = enabled,
                modifier = Modifier.fillMaxWidth().height(48.dp),
            ) {
                Text(selected?.displayName ?: "Automatic")
            }
            DropdownMenu(
                expanded = expanded,
                onDismissRequest = { expanded = false },
            ) {
                DropdownMenuItem(
                    text = { Text("Automatic") },
                    onClick = {
                        onSelect(null)
                        expanded = false
                    },
                )
                KetProtocol.entries.forEach { protocol ->
                    DropdownMenuItem(
                        text = { Text(protocol.displayName) },
                        onClick = {
                            onSelect(protocol)
                            expanded = false
                        },
                    )
                }
            }
        }
        Text(
            selected?.shortInstruction ?: AUTO_PROTOCOL_INSTRUCTION,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun ProtocolGuideContent(
    selected: KetProtocol,
    onSelect: (KetProtocol) -> Unit,
) {
    var menuExpanded by androidx.compose.runtime.remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(18.dp)) {
        Box(Modifier.fillMaxWidth()) {
            OutlinedButton(
                onClick = { menuExpanded = true },
                modifier = Modifier.fillMaxWidth().height(48.dp),
            ) {
                Text(selected.displayName)
            }
            DropdownMenu(
                expanded = menuExpanded,
                onDismissRequest = { menuExpanded = false },
            ) {
                KetProtocol.entries.forEach { item ->
                    DropdownMenuItem(
                        text = { Text(item.displayName) },
                        onClick = {
                            onSelect(item)
                            menuExpanded = false
                        },
                    )
                }
            }
        }
        Text(selected.shortInstruction, style = MaterialTheme.typography.titleMedium)
        ProtocolGuideSection("Best for", selected.bestFor)
        ProtocolGuideSection("How it works", selected.operation)
        ProtocolGuideList("Connect", selected.steps)
        ProtocolGuideList("Limits", selected.limitations)
    }
}

@Composable
private fun ProtocolGuideSection(title: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.55f))
        Text(title, color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge)
        Text(body, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun ProtocolGuideList(title: String, items: List<String>) {
    Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.55f))
        Text(title, color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge)
        items.forEachIndexed { index, item ->
            Text(
                "${index + 1}. $item",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

@Composable
private fun PhaseStatus(
    phase: TunnelPhase,
    disconnectedLabel: String = phaseStatusLabel(TunnelPhase.Disconnected),
) {
    val watchTransition = rememberInfiniteTransition(label = "watched status")
    val watchedAlpha by watchTransition.animateFloat(
        initialValue = 0.58f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1_800), RepeatMode.Reverse),
        label = "watched status alpha",
    )
    val tone by animateColorAsState(
        targetValue = when (phase) {
            TunnelPhase.Connected -> MaterialTheme.colorScheme.primary
            TunnelPhase.Failed -> KetSaturated
            TunnelPhase.Disconnected -> MaterialTheme.colorScheme.onSurfaceVariant
            else -> KetDegraded
        },
        animationSpec = tween(240),
        label = "phase tone",
    )
    AnimatedContent(
        targetState = phase,
        transitionSpec = {
            (fadeIn(tween(180)) + scaleIn(tween(180), initialScale = 0.92f)) togetherWith
                (fadeOut(tween(120)) + scaleOut(tween(120), targetScale = 0.96f))
        },
        label = "connection status",
    ) { currentPhase ->
        Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            when (currentPhase) {
                TunnelPhase.Connected -> Icon(
                    Icons.Filled.Shield,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = tone,
                )
                TunnelPhase.Disconnected -> Icon(
                    Icons.Filled.Visibility,
                    contentDescription = null,
                    modifier = Modifier.size(17.dp).alpha(watchedAlpha),
                    tint = tone,
                )
                TunnelPhase.Failed -> Icon(
                    Icons.Filled.Warning,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = tone,
                )
                else -> CircularProgressIndicator(
                    modifier = Modifier.size(15.dp),
                    color = tone,
                    strokeWidth = 1.8.dp,
                )
            }
            Text(
                if (currentPhase == TunnelPhase.Disconnected) {
                    disconnectedLabel
                } else {
                    phaseStatusLabel(currentPhase)
                },
                color = tone,
                style = MaterialTheme.typography.labelMedium,
            )
        }
    }
}

internal fun phaseStatusLabel(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Disconnected -> "You are being watched"
    TunnelPhase.Enrolling -> "Authorizing"
    TunnelPhase.Connecting -> "Connecting"
    TunnelPhase.Reconnecting -> "Recovering"
    TunnelPhase.Connected -> "Liberated"
    TunnelPhase.Stopping -> "Stopping"
    TunnelPhase.Failed -> "Attention"
}

internal fun phaseHeadline(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Liberated route"
    TunnelPhase.Enrolling, TunnelPhase.Connecting -> "Securing route"
    TunnelPhase.Reconnecting -> "Restoring route"
    TunnelPhase.Stopping -> "Closing route"
    TunnelPhase.Failed -> "Route needs attention"
    TunnelPhase.Disconnected -> "Connect to get liberated"
}

internal fun disconnectedSurveillanceLabel(variant: Int): String =
    if (variant % 2 == 0) "You are being watched" else "You are under surveillance"

internal fun connectionLocationHeading(node: AndroidNodeStatus?): String = node?.location?.let { location ->
    if (location.city.isNullOrBlank()) location.countryName else "${location.city}, ${location.countryName}"
} ?: "Choose your node"

internal fun connectionActionLabel(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Disconnect tunnel"
    TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting -> "Cancel connection attempt"
    TunnelPhase.Stopping -> "Disconnecting tunnel"
    TunnelPhase.Disconnected, TunnelPhase.Failed -> "Connect tunnel"
}

internal fun connectionPowerHeadline(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Liberated"
    TunnelPhase.Enrolling -> "Adding server"
    TunnelPhase.Connecting -> "Connecting"
    TunnelPhase.Reconnecting -> "Reconnecting"
    TunnelPhase.Stopping -> "Disconnecting"
    TunnelPhase.Failed -> "Connection failed"
    TunnelPhase.Disconnected -> "Connect to get liberated"
}

internal fun connectionPowerDetail(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Protected route active"
    TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting -> "Testing available routes"
    TunnelPhase.Stopping -> "Stopping tunnel and restoring network"
    TunnelPhase.Failed -> "Open diagnostics, then try again"
    TunnelPhase.Disconnected -> "You are under surveillance"
}

@Composable
private fun ServerMap(snapshot: TunnelSnapshot, connected: Boolean) {
    val node = snapshot.node
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(7.dp),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column {
            KetMap(node?.location, connected)
            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(9.dp),
                verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
            ) {
                Icon(
                    Icons.Filled.LocationOn,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    node?.displayName ?: "No server selected",
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.labelLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    node?.location?.countryCode ?: "--",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            RouteStatStrip(snapshot, node)
        }
    }
}

@Composable
private fun RouteStatStrip(snapshot: TunnelSnapshot, node: AndroidNodeStatus?) {
    val metrics = listOf(
        Triple(Icons.Filled.CloudDownload, "Received", formatBytes(snapshot.receivedBytes)),
        Triple(Icons.Filled.CloudUpload, "Sent", formatBytes(snapshot.sentBytes)),
        Triple(Icons.Filled.NetworkCheck, "Latency", snapshot.handshakeLatencyMs?.let { "$it ms" } ?: "--"),
        Triple(Icons.Filled.Speed, "Node load", node?.capacityPercent?.let { "${it.toInt()}%" } ?: "--"),
    )
    Row(
        modifier = Modifier.fillMaxWidth().height(76.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
    ) {
        metrics.forEachIndexed { index, (icon, label, value) ->
            if (index > 0) {
                Spacer(
                    Modifier
                        .width(1.dp)
                        .height(48.dp)
                        .background(MaterialTheme.colorScheme.outline.copy(alpha = 0.7f)),
                )
            }
            Column(
                modifier = Modifier.weight(1f).padding(horizontal = 7.dp),
                horizontalAlignment = androidx.compose.ui.Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Icon(
                    imageVector = icon,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
                Text(
                    label,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelSmall,
                    maxLines = 1,
                )
                Text(
                    value,
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun NodeCapacity(node: AndroidNodeStatus) {
    val tone by animateColorAsState(
        targetValue = when (node.health) {
            AndroidNodeHealth.Healthy -> KetHealthy
            AndroidNodeHealth.Degraded -> KetDegraded
            AndroidNodeHealth.Saturated -> KetSaturated
        },
        label = "node health tone",
    )
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(node.health.displayName, color = tone, style = MaterialTheme.typography.labelLarge)
            Text(
                "${node.activeSessions} / ${node.sessionCapacity} sessions",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.labelMedium,
            )
        }
        LinearProgressIndicator(
            progress = { (node.capacityPercent / 100.0).toFloat() },
            modifier = Modifier.fillMaxWidth().height(5.dp),
            color = tone,
            trackColor = MaterialTheme.colorScheme.outline.copy(alpha = 0.45f),
        )
    }
}

@Composable
private fun Telemetry(snapshot: TunnelSnapshot, node: AndroidNodeStatus) {
    val accessRemaining = snapshot.accessExpiresAtEpochSeconds
        ?.minus(System.currentTimeMillis() / 1_000)
        ?.coerceAtLeast(0)
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        MetricRow(
            "Download" to formatBytes(snapshot.receivedBytes),
            "Upload" to formatBytes(snapshot.sentBytes),
            "Handshake" to (snapshot.handshakeLatencyMs?.let { "$it ms" } ?: "--"),
        )
        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.45f))
        MetricRow(
            "Transport" to snapshot.transportName,
            "CPU" to (node.cpuLoadPercent?.let { "%.1f%%".format(it) } ?: "--"),
            "Uptime" to formatDuration(node.uptimeSeconds),
        )
        MetricRow(
            "Memory" to formatMemory(node),
            "Access left" to formatDuration(accessRemaining),
            "Node load" to "${node.capacityPercent.toInt()}%",
        )
    }
}

@Composable
private fun MetricRow(vararg metrics: Pair<String, String>) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        metrics.forEach { (label, value) ->
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    label,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelSmall,
                )
                Text(
                    value,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

private fun formatBytes(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val units = arrayOf("KB", "MB", "GB", "TB")
    var value = bytes.toDouble()
    var unit = -1
    while (value >= 1024 && unit < units.lastIndex) {
        value /= 1024
        unit += 1
    }
    return "%.1f %s".format(value, units[unit])
}

private fun formatMemory(node: AndroidNodeStatus): String =
    if (node.memoryUsedBytes == null || node.memoryTotalBytes == null) {
        "--"
    } else {
        "${formatBytes(node.memoryUsedBytes)} / ${formatBytes(node.memoryTotalBytes)}"
    }

private fun formatDuration(seconds: Long?): String {
    if (seconds == null) return "--"
    val days = seconds / 86_400
    val hours = seconds % 86_400 / 3_600
    val minutes = seconds % 3_600 / 60
    return when {
        days > 0 -> "${days}d ${hours}h"
        hours > 0 -> "${hours}h ${minutes}m"
        else -> "${minutes}m"
    }
}
