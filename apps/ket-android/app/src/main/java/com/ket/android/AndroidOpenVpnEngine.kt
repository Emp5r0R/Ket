package com.ket.android

import android.os.ParcelFileDescriptor
import java.io.File
import java.net.InetAddress
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

internal class AndroidOpenVpnEngine(
    private val service: KetVpnService,
    private val transport: OpenVpnStunnelTransport,
    private val resolvedEndpoint: InetAddress,
    private val openTun: (OpenVpnTunRequest) -> ParcelFileDescriptor,
) : AndroidTransportEngine {
    private val diagnostic = AtomicReference<String?>()
    private var process: Process? = null
    private var carrier: PinnedTlsTcpBridge? = null
    private var management: AndroidOpenVpnManagement? = null

    override val displayName: String = transport.displayName

    override fun start(cancelled: () -> Boolean): AndroidEngineStarted {
        val startedAt = System.nanoTime()
        ensureEngineStartActive(cancelled)
        val libraryDirectory = File(service.applicationInfo.nativeLibraryDir)
        val executable = File(libraryDirectory, "libovpnexec.so")
        val library = File(libraryDirectory, "libopenvpn.so")
        require(executable.isFile && executable.canExecute() && library.isFile) {
            "OpenVPN is not installed for this device architecture"
        }

        try {
            val tlsCarrier = PinnedTlsTcpBridge(service, transport, resolvedEndpoint)
            carrier = tlsCarrier
            tlsCarrier.start()

            val socketFile = File(
                service.cacheDir,
                "ket-ovpn-${java.lang.Long.toUnsignedString(System.nanoTime(), 36)}.sock",
            )
            require(socketFile.absolutePath.length <= 96) {
                "Android's private cache path is too long for OpenVPN management"
            }
            val manager = AndroidOpenVpnManagement(
                service,
                socketFile,
                transport.username,
                transport.password,
                openTun,
            )
            management = manager
            manager.start()
            ensureEngineStartActive(cancelled)

            val child = ProcessBuilder(executable.absolutePath, "--config", "stdin")
                .directory(service.cacheDir)
                .redirectErrorStream(true)
                .apply {
                    environment()["LD_LIBRARY_PATH"] = libraryDirectory.absolutePath
                    environment()["TMPDIR"] = service.cacheDir.absolutePath
                }
                .start()
            process = child
            thread(name = "ket-openvpn-log", isDaemon = true) {
                consumeProcessOutput(child.inputStream) { line ->
                    classifyDiagnostic(line)?.let { diagnostic.compareAndSet(null, it) }
                }
            }
            val config = OpenVpnAndroidConfig.render(transport, tlsCarrier.port, socketFile.absolutePath)
            try {
                child.outputStream.use { it.write(config) }
            } finally {
                config.fill(0)
            }
            manager.awaitConnected(child, cancelled, STARTUP_TIMEOUT_SECONDS)
            ensureEngineStartActive(cancelled)
            require(tlsCarrier.isAlive()) {
                tlsCarrier.diagnostic ?: "OpenVPN TLS carrier stopped during startup"
            }
            require(child.isAlive) { diagnostic.get() ?: "OpenVPN stopped during startup" }
            return AndroidEngineStarted(
                AndroidEngineRoute.NativeTun,
                TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedAt),
                resolvedEndpoint,
            )
        } catch (error: Exception) {
            val reason = carrier?.diagnostic ?: management?.diagnostic ?: diagnostic.get()
            close()
            throw IllegalStateException(reason ?: error.message ?: "OpenVPN failed to start", error)
        }
    }

    override fun isAlive(): Boolean =
        process?.isAlive == true && carrier?.isAlive() == true && management?.isConnected() == true

    override fun trafficStats(): LongArray? = management?.trafficStats()

    @Synchronized
    override fun close() {
        management?.signalStop()
        process?.let { child ->
            if (!child.waitFor(1, TimeUnit.SECONDS)) {
                child.destroy()
                if (!child.waitFor(2, TimeUnit.SECONDS)) {
                    child.destroyForcibly()
                    child.waitFor(1, TimeUnit.SECONDS)
                }
            }
        }
        process = null
        management?.close()
        management = null
        carrier?.close()
        carrier = null
    }

    private fun classifyDiagnostic(line: String): String? {
        val normalized = line.lowercase()
        return when {
            "auth_failed" in normalized || "auth failure" in normalized ->
                "OpenVPN rejected the scoped credential"
            "certificate verify failed" in normalized || "tls error" in normalized ->
                "OpenVPN server certificate verification failed"
            "permission denied" in normalized || "operation not permitted" in normalized ->
                "Android denied the OpenVPN engine permission"
            "options error" in normalized -> "OpenVPN rejected its hardened configuration"
            else -> null
        }
    }

    private companion object {
        const val STARTUP_TIMEOUT_SECONDS = 30L
    }
}
