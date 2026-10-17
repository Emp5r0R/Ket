package com.ket.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.MenuBook
import androidx.compose.material.icons.filled.Analytics
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.TextButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.zIndex
import androidx.compose.ui.unit.dp
import com.ket.android.KetDiagnosticsDialog
import com.ket.android.KetProtocol
import com.ket.android.KetThemeMode
import com.ket.android.R
import com.ket.android.TunnelPhase
import com.ket.android.TunnelSnapshot
import com.ket.android.appInviteReader
import com.ket.android.connection.ConnectionOptions
import com.ket.android.disconnectedSurveillanceLabel
import com.ket.android.invite.InviteReader
import com.ket.android.invite.NodePublicKey
import com.ket.android.inviteFailureMessage
import com.ket.android.splittunnel.InstalledApp
import com.ket.android.splittunnel.SplitTunnelPolicy
import com.ket.android.ui.connection.AddServerScreen
import com.ket.android.ui.connection.ConnectionScreen
import com.ket.android.ui.connection.StatusIsland
import com.ket.android.ui.metrics.MetricsScreen
import com.ket.android.ui.protocols.ProtocolGuideScreen
import com.ket.android.ui.settings.SettingsScreen
import kotlinx.coroutines.delay

internal enum class KetAppDestination(val storedName: String, val displayName: String) {
    Connection("connection", "Connect"),
    Metrics("metrics", "Metrics"),
    Protocols("protocols", "Protocols"),
    Settings("settings", "Settings");

    companion object {
        fun fromStored(value: String?): KetAppDestination =
            entries.firstOrNull { it.storedName == value } ?: Connection
    }
}

private val busyPhases = setOf(TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting, TunnelPhase.Stopping)
private val ACCESS_CODE = Regex("^[A-Za-z0-9]{32}$")

@Composable
internal fun KetApp(
    snapshot: TunnelSnapshot,
    initialServerUrl: String = "",
    initialAccessCode: String = "",
    initialPreferredProtocol: KetProtocol? = null,
    initialNodeKey: NodePublicKey? = null,
    initialAccessExpiresAtEpochSeconds: Long? = null,
    offeredProtocols: List<KetProtocol> = emptyList(),
    sharedInvite: String? = null,
    onSharedInviteConsumed: () -> Unit = {},
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
    appVersion: String = "",
    inviteReader: InviteReader = appInviteReader,
    onConnect: (String, String, KetProtocol?, NodePublicKey?) -> Unit,
    onDisconnect: () -> Unit,
) {
    var serverUrl by rememberSaveable { mutableStateOf(initialServerUrl) }
    var accessCode by rememberSaveable { mutableStateOf(initialAccessCode) }
    // The node key is public; it pins every enrollment of an invite-based profile.
    var nodeKeyText by rememberSaveable { mutableStateOf(initialNodeKey?.base64Url) }
    var nodeSealedControl by rememberSaveable { mutableStateOf(initialNodeKey?.sealedControl == true) }
    var addIssue by remember { mutableStateOf<String?>(null) }
    var preferredProtocolWireName by rememberSaveable { mutableStateOf(initialPreferredProtocol?.wireName) }
    var accessExpiresAtEpochSeconds by rememberSaveable { mutableStateOf(initialAccessExpiresAtEpochSeconds) }
    var accessRemainingSeconds by rememberSaveable { mutableStateOf<Long?>(null) }
    var profileSaved by rememberSaveable {
        mutableStateOf(initialServerUrl.isNotBlank() && initialAccessCode.length == 32)
    }
    var disconnectedVariant by rememberSaveable { mutableIntStateOf(0) }
    var diagnosticsVisible by rememberSaveable { mutableStateOf(false) }
    var destinationWireName by rememberSaveable { mutableStateOf(KetAppDestination.Connection.storedName) }
    var guideFocusWireName by rememberSaveable { mutableStateOf<String?>(null) }
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
    // The session's node and protocols vanish from the snapshot on disconnect;
    // the screens keep showing the last ones known for this server.
    var lastNode by remember { mutableStateOf(snapshot.node) }
    var lastOffered by remember { mutableStateOf(offeredProtocols) }
    LaunchedEffect(snapshot.node) { snapshot.node?.let { lastNode = it } }
    LaunchedEffect(offeredProtocols) { if (offeredProtocols.isNotEmpty()) lastOffered = offeredProtocols }
    val preferredProtocol = preferredProtocolWireName?.let(KetProtocol::fromWireName)
    val destination = KetAppDestination.fromStored(destinationWireName)
    val connected = snapshot.phase == TunnelPhase.Connected
    val busy = snapshot.phase in busyPhases
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
        if (snapshot.phase == TunnelPhase.Failed && profileSaved) diagnosticsVisible = true
    }
    BackHandler(enabled = destination != KetAppDestination.Connection) {
        destinationWireName = KetAppDestination.Connection.storedName
    }
    val openGuide = { focus: KetProtocol? ->
        guideFocusWireName = (focus ?: preferredProtocol)?.wireName
        destinationWireName = KetAppDestination.Protocols.storedName
    }
    val forgetProfile = {
        if (connected || busy) onDisconnect()
        serverUrl = ""
        accessCode = ""
        nodeKeyText = null
        accessExpiresAtEpochSeconds = null
        accessRemainingSeconds = null
        profileSaved = false
        lastNode = null
        lastOffered = emptyList()
        destinationWireName = KetAppDestination.Connection.storedName
        onForgetProfile()
    }
    val addInvite = { text: String ->
        runCatching { inviteReader.read(text, System.currentTimeMillis() / 1_000) }
            .onSuccess { target ->
                addIssue = null
                serverUrl = target.serverUrl
                accessCode = target.accessCode
                nodeKeyText = target.nodeKey.base64Url
                nodeSealedControl = target.nodeKey.sealedControl
                onConnect(target.serverUrl, target.accessCode, null, target.nodeKey)
            }
            .onFailure { addIssue = inviteFailureMessage(it) }
        Unit
    }
    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
        topBar = {
            // The status island sits at the top centre, over the logo if it opens wide.
            Box(
                Modifier
                    .fillMaxWidth()
                    .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal))
                    .padding(horizontal = 16.dp, vertical = 8.dp)
                    .heightIn(min = 44.dp),
            ) {
                Image(
                    painterResource(R.drawable.ket_mark),
                    contentDescription = "Ket",
                    modifier = Modifier.size(30.dp).align(Alignment.CenterStart),
                )
                StatusIsland(
                    snapshot = snapshot,
                    disconnectedLabel = disconnectedSurveillanceLabel(disconnectedVariant),
                    modifier = Modifier.align(Alignment.TopCenter).zIndex(1f),
                )
            }
        },
        bottomBar = {
            NavigationBar(containerColor = MaterialTheme.colorScheme.surface, tonalElevation = 0.dp) {
                KetAppDestination.entries.forEach { item ->
                    NavigationBarItem(
                        selected = destination == item,
                        onClick = { destinationWireName = item.storedName },
                        icon = {
                            Icon(
                                when (item) {
                                    KetAppDestination.Connection -> Icons.Filled.Shield
                                    KetAppDestination.Metrics -> Icons.Filled.Analytics
                                    KetAppDestination.Protocols -> Icons.AutoMirrored.Filled.MenuBook
                                    KetAppDestination.Settings -> Icons.Filled.Settings
                                },
                                contentDescription = null,
                            )
                        },
                        label = { Text(item.displayName) },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = MaterialTheme.colorScheme.primary,
                            selectedTextColor = MaterialTheme.colorScheme.primary,
                            indicatorColor = MaterialTheme.colorScheme.primaryContainer,
                        ),
                    )
                }
            }
        },
    ) { contentPadding ->
        AnimatedContent(
            targetState = destination,
            transitionSpec = { fadeIn(tween(200)) togetherWith fadeOut(tween(120)) },
            label = "app destination",
            modifier = Modifier.fillMaxSize().padding(contentPadding),
        ) { current ->
            key(current) {
                Column(
                    Modifier
                        .fillMaxSize()
                        .verticalScroll(rememberScrollState())
                        .padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    when (current) {
                        KetAppDestination.Connection ->
                            if (!profileSaved && !connected && !busy) {
                                AddServerScreen(
                                    issue = addIssue ?: snapshot.message.takeIf { snapshot.phase == TunnelPhase.Failed && it.isNotBlank() },
                                    sharedInvite = sharedInvite,
                                    onSharedInviteConsumed = onSharedInviteConsumed,
                                    onInvite = addInvite,
                                    onManual = { url, code ->
                                        addIssue = null
                                        nodeKeyText = null
                                        serverUrl = url
                                        accessCode = code
                                        onConnect(url, code, null, null)
                                    },
                                    onIssueChange = { addIssue = it },
                                )
                            } else {
                                ConnectionScreen(
                                    snapshot = snapshot,
                                    lastNode = lastNode,
                                    serverUrl = serverUrl,
                                    accessRemainingSeconds = accessRemainingSeconds,
                                    preferredProtocol = preferredProtocol,
                                    offeredProtocols = lastOffered,
                                    canStartConnection = serverUrl.isNotBlank() && accessCode.matches(ACCESS_CODE),
                                    onPreferredProtocolChange = { preferredProtocolWireName = it?.wireName },
                                    onOpenProtocolGuide = { openGuide(null) },
                                    onToggle = {
                                        if (connected || busy) {
                                            onDisconnect()
                                        } else {
                                            onConnect(
                                                serverUrl,
                                                accessCode,
                                                preferredProtocol,
                                                nodeKeyText
                                                    ?.let(NodePublicKey::fromBase64Url)
                                                    ?.withSealedControl(nodeSealedControl),
                                            )
                                        }
                                    },
                                    onOpenDiagnostics = { diagnosticsVisible = true },
                                )
                            }
                        KetAppDestination.Metrics -> MetricsScreen(snapshot, accessRemainingSeconds)
                        KetAppDestination.Protocols -> ProtocolGuideScreen(
                            offered = lastOffered,
                            selected = preferredProtocol,
                            expanded = guideFocusWireName?.let(KetProtocol::fromWireName),
                            canChoose = !connected && !busy,
                            onChoose = {
                                preferredProtocolWireName = it?.wireName
                                destinationWireName = KetAppDestination.Connection.storedName
                            },
                        )
                        KetAppDestination.Settings -> SettingsScreen(
                            themeMode = themeMode,
                            onThemeModeChange = onThemeModeChange,
                            splitTunnel = splitTunnel,
                            connected = connected,
                            onSplitTunnelChange = onSplitTunnelChange,
                            loadInstalledApps = loadInstalledApps,
                            connectionOptions = connectionOptions,
                            onConnectionOptionsChange = onConnectionOptionsChange,
                            savedServerUrl = serverUrl.takeIf { profileSaved },
                            accessRemainingSeconds = accessRemainingSeconds,
                            onForgetServer = forgetProfile,
                            onOpenDiagnostics = { diagnosticsVisible = true },
                            onOpenProtocolGuide = { openGuide(null) },
                            appVersion = appVersion,
                            serverVersion = snapshot.serverVersion,
                        )
                    }
                }
            }
        }
    }
    if (diagnosticsVisible) {
        KetDiagnosticsDialog(report = diagnosticReport, onDismissRequest = { diagnosticsVisible = false })
    }
    if (sharedInvite != null && profileSaved) {
        AlertDialog(
            onDismissRequest = onSharedInviteConsumed,
            title = { Text("Ket already has a server") },
            text = {
                Text("To use the invite you shared, first forget the current server in Settings, then share the invite to Ket again.")
            },
            confirmButton = { TextButton(onClick = onSharedInviteConsumed) { Text("OK") } },
        )
    }
}
