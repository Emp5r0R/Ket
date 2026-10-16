package com.ket.android

import android.Manifest
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.mutableStateOf
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import com.ket.android.invite.NodePublicKey
import com.ket.android.splittunnel.InstalledApps
import com.ket.android.splittunnel.SplitTunnelPolicy
import com.ket.android.connection.ConnectionOptions
import com.ket.android.connection.ConnectionOptionsStore
import com.ket.android.splittunnel.SplitTunnelStore

private data class PendingConnectionRequest(
    val serverUrl: String,
    val accessCode: String,
    val preferredProtocol: KetProtocol?,
    val nodeKey: NodePublicKey?,
)

private object PendingPermissionRequest {
    var serverUrl: String? = null
    var accessCode: String? = null
    var preferredProtocol: KetProtocol? = null
    var nodeKey: NodePublicKey? = null

    fun clear(): PendingConnectionRequest? {
        val server = serverUrl
        val code = accessCode
        val protocol = preferredProtocol
        val key = nodeKey
        serverUrl = null
        accessCode = null
        preferredProtocol = null
        nodeKey = null
        return if (server != null && code != null) PendingConnectionRequest(server, code, protocol, key) else null
    }
}

class MainActivity : ComponentActivity() {
    private val tunnelState = mutableStateOf(KetTunnelRuntime.snapshot())
    private val themeMode = mutableStateOf(KetThemeMode.System)
    private val splitTunnel = mutableStateOf(SplitTunnelPolicy.AllApps)
    private val connectionOptions = mutableStateOf(ConnectionOptions())
    private val startupMode = mutableStateOf(KetStartupMode.Ready)
    private var startupRestoreStarted = false
    private var subscription: AutoCloseable? = null
    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val request = PendingPermissionRequest.clear()
        if (result.resultCode == RESULT_OK && request != null) {
            KetTunnelController.connect(
                this,
                request.serverUrl,
                request.accessCode,
                request.preferredProtocol,
                request.nodeKey,
            )
        } else if (request != null) {
            AndroidConnectionDiagnostics.record(
                stage = "vpn_permission",
                outcome = DiagnosticOutcome.Failed,
                phase = TunnelPhase.Failed,
                protocol = request.preferredProtocol,
                code = "vpn_permission_denied",
            )
            KetTunnelRuntime.publish(
                TunnelSnapshot(phase = TunnelPhase.Failed, message = "Android VPN permission was not granted"),
            )
        }
    }
    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        requestVpnPermission()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val preferences = getSharedPreferences(APPEARANCE_PREFERENCES, MODE_PRIVATE)
        themeMode.value = KetThemeMode.fromStored(preferences.getString(THEME_MODE_KEY, null))
        val splitTunnelStore = SplitTunnelStore(this)
        splitTunnel.value = splitTunnelStore.load()
        val connectionOptionsStore = ConnectionOptionsStore(this)
        connectionOptions.value = connectionOptionsStore.load()
        val savedCredentialsResult = runCatching {
            AndroidTunnelCredentialStore.get(this).load()?.let { credentials ->
                if (credentials.accessExpired()) {
                    AndroidTunnelCredentialStore.get(this).clearAll()
                    null
                } else {
                    credentials
                }
            }
        }
        val savedCredentials = savedCredentialsResult.getOrNull()
        savedCredentialsResult.exceptionOrNull()?.let { error ->
            AndroidConnectionDiagnostics.begin(null, source = "profile_restore")
            AndroidConnectionDiagnostics.record(
                stage = "saved_profile_load",
                outcome = DiagnosticOutcome.Failed,
                phase = TunnelPhase.Failed,
                code = "saved_profile_unavailable",
                message = error.message,
            )
            if (KetTunnelRuntime.snapshot().phase == TunnelPhase.Disconnected) {
                KetTunnelRuntime.publish(
                    TunnelSnapshot(
                        phase = TunnelPhase.Failed,
                        message = "The saved server profile could not be opened. It was not deleted; restart Ket and open diagnostics if the problem continues.",
                    ),
                )
            }
        }
        val startupDecision = KetStartupPolicy.initial(
            hasSavedProfile = savedCredentials != null,
            phase = tunnelState.value.phase,
        )
        startupMode.value = startupDecision.mode
        startupRestoreStarted = startupDecision.observeExistingConnection
        subscription = KetTunnelRuntime.subscribe { snapshot ->
            runOnUiThread {
                tunnelState.value = snapshot
                if (KetStartupPolicy.restoreFinished(startupRestoreStarted, snapshot.phase)) {
                    startupMode.value = KetStartupMode.Ready
                }
            }
        }
        setContent {
            KetTheme(
                connected = tunnelState.value.phase == TunnelPhase.Connected,
                mode = themeMode.value,
            ) {
                if (startupMode.value == KetStartupMode.RestoringProfile) {
                    KetStartupScreen()
                } else {
                    KetApp(
                        snapshot = tunnelState.value,
                        initialServerUrl = savedCredentials?.profile?.serverUrl.orEmpty(),
                        initialAccessCode = savedCredentials?.profile?.accessCode.orEmpty(),
                        initialPreferredProtocol = savedCredentials?.profile?.preferredProtocol,
                        initialNodeKey = savedCredentials?.profile?.nodeKey,
                        initialAccessExpiresAtEpochSeconds = savedCredentials?.accessExpiresAtEpochSeconds,
                        onAccessExpired = {
                            runCatching { AndroidTunnelCredentialStore.get(this).clearAll() }
                        },
                        themeMode = themeMode.value,
                        onThemeModeChange = { selected ->
                            themeMode.value = selected
                            preferences.edit {
                                putString(THEME_MODE_KEY, selected.storedName)
                            }
                        },
                        splitTunnel = splitTunnel.value,
                        onSplitTunnelChange = { policy ->
                            splitTunnel.value = policy
                            splitTunnelStore.save(policy)
                        },
                        loadInstalledApps = { InstalledApps.launchable(this) },
                        connectionOptions = connectionOptions.value,
                        onConnectionOptionsChange = { options ->
                            connectionOptions.value = options
                            connectionOptionsStore.save(options)
                        },
                        onForgetProfile = {
                            runCatching { AndroidTunnelCredentialStore.get(this).clearAll() }
                        },
                        diagnosticReport = AndroidConnectionDiagnostics.report(
                            BuildConfig.VERSION_NAME,
                            tunnelState.value,
                        ),
                        onConnect = ::requestConnection,
                        onDisconnect = { KetTunnelController.disconnect(this) },
                    )
                }
            }
        }
        if (startupDecision.requestSavedProfileConnection && savedCredentials != null) {
            startupRestoreStarted = true
            window.decorView.post {
                if (!isFinishing && !isDestroyed) {
                    requestConnection(
                        savedCredentials.profile.serverUrl,
                        savedCredentials.profile.accessCode,
                        savedCredentials.profile.preferredProtocol,
                        savedCredentials.profile.nodeKey,
                    )
                }
            }
        }
    }

    override fun onDestroy() {
        subscription?.close()
        subscription = null
        super.onDestroy()
    }

    private fun requestConnection(
        serverUrl: String,
        accessCode: String,
        preferredProtocol: KetProtocol?,
        nodeKey: NodePublicKey?,
    ) {
        PendingPermissionRequest.serverUrl = serverUrl
        PendingPermissionRequest.accessCode = accessCode
        PendingPermissionRequest.preferredProtocol = preferredProtocol
        PendingPermissionRequest.nodeKey = nodeKey
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else {
            requestVpnPermission()
        }
    }

    private fun requestVpnPermission() {
        val permission = VpnService.prepare(this)
        if (permission == null) {
            PendingPermissionRequest.clear()?.let {
                KetTunnelController.connect(this, it.serverUrl, it.accessCode, it.preferredProtocol, it.nodeKey)
            }
        } else {
            vpnPermission.launch(permission)
        }
    }

    private companion object {
        const val APPEARANCE_PREFERENCES = "ket-appearance"
        const val THEME_MODE_KEY = "theme-mode"
    }
}
