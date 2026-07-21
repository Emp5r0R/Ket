package com.ket.android

import android.system.Os
import android.os.Build
import java.io.File
import java.net.DatagramSocket
import java.net.Inet6Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

internal class AndroidWireGuardTlsEngine(
    private val service: KetVpnService,
    private val transport: WireGuardTlsTransport,
    private val resolvedEndpoint: InetAddress,
) : AndroidTransportEngine {
    private var xray: Process? = null
    private var wstunnel: Process? = null
    private var configFile: File? = null
    private val diagnostic = AtomicReference<String?>()

    override val displayName: String = transport.displayName

    override fun start(cancelled: () -> Boolean): AndroidEngineStarted {
        val startedAt = System.nanoTime()
        ensureEngineStartActive(cancelled)
        require(Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            "WireGuard TLS requires Android 9 or newer"
        }
        val libraryDirectory = service.applicationInfo.nativeLibraryDir
        val xrayBinary = File(libraryDirectory, "libxray.so")
        val wstunnelBinary = File(libraryDirectory, "libwstunnel.so")
        require(xrayBinary.isFile && xrayBinary.canExecute()) {
            "Xray is not installed for this device architecture"
        }
        require(wstunnelBinary.isFile && wstunnelBinary.canExecute()) {
            "WireGuard TLS is available only on supported 64-bit ARM devices"
        }
        val socksPort = reserveTcpPort()
        val wireGuardPort = reserveUdpPort()
        configFile = writePrivateConfig(EngineConfig.wireGuard(transport, wireGuardPort, socksPort))
        validateXray(xrayBinary)
        ensureEngineStartActive(cancelled)

        val carrier = ProcessBuilder(wstunnelArguments(wstunnelBinary, wireGuardPort))
            .redirectErrorStream(true)
            .apply { environment()["NO_COLOR"] = "true" }
            .start()
        wstunnel = carrier
        thread(name = "ket-wstunnel-log", isDaemon = true) {
            consumeProcessOutput(carrier.inputStream) { line ->
                classifyDiagnostic(line)?.let { diagnostic.compareAndSet(null, it) }
            }
        }
        repeat(5) {
            ensureEngineStartActive(cancelled)
            if (!carrier.isAlive) throw IllegalStateException(diagnostic.get() ?: "wstunnel exited during startup")
            Thread.sleep(100)
        }

        val engine = ProcessBuilder(xrayBinary.absolutePath, "run", "-c", configFile!!.absolutePath)
            .redirectErrorStream(true)
            .start()
        xray = engine
        thread(name = "ket-wireguard-xray-log", isDaemon = true) {
            consumeProcessOutput(engine.inputStream) { line ->
                classifyDiagnostic(line)?.let { diagnostic.compareAndSet(null, it) }
            }
        }
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(25)
        while (System.nanoTime() < deadline) {
            ensureEngineStartActive(cancelled)
            if (!carrier.isAlive) throw IllegalStateException(diagnostic.get() ?: "wstunnel exited during startup")
            if (!engine.isAlive) throw IllegalStateException(diagnostic.get() ?: "WireGuard Xray exited during startup")
            if (socksReady(socksPort)) {
                try {
                    verifySocksTunnel(socksPort, "one.one.one.one")
                    configFile?.delete()
                    configFile = null
                    return AndroidEngineStarted(
                        AndroidEngineRoute.Socks(socksPort),
                        TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedAt),
                        resolvedEndpoint,
                    )
                } catch (error: Exception) {
                    diagnostic.compareAndSet(null, error.message)
                }
            }
            Thread.sleep(100)
        }
        throw IllegalStateException(diagnostic.get() ?: "WireGuard TLS handshake timed out")
    }

    override fun isAlive(): Boolean = xray?.isAlive == true && wstunnel?.isAlive == true

    override fun close() {
        configFile?.delete()
        configFile = null
        stop(xray)
        xray = null
        stop(wstunnel)
        wstunnel = null
    }

    private fun validateXray(binary: File) {
        val check = ProcessBuilder(binary.absolutePath, "run", "-test", "-c", configFile!!.absolutePath)
            .redirectErrorStream(true)
            .start()
        if (!check.waitFor(5, TimeUnit.SECONDS)) {
            check.destroyForcibly()
            throw IllegalStateException("WireGuard Xray configuration validation timed out")
        }
        require(check.exitValue() == 0) { "Xray rejected the WireGuard configuration" }
    }

    private fun wstunnelArguments(binary: File, localPort: Int): List<String> {
        val address = requireNotNull(resolvedEndpoint.hostAddress) { "Server address is unavailable" }
        val host = if (resolvedEndpoint is Inet6Address) "[$address]" else address
        return listOf(
            binary.absolutePath,
            "client",
            "--no-color",
            "--log-lvl",
            "WARN",
            "--tls-sni-override",
            transport.tlsServerName,
            "--tls-verify-certificate",
            "--http-upgrade-path-prefix",
            transport.pathPrefix,
            "--http-headers",
            "Host: ${transport.tlsServerName}",
            "--local-to-remote",
            "udp://127.0.0.1:$localPort:${transport.remoteAddress}?timeout_sec=0",
            "wss://$host:${transport.port}",
        )
    }

    private fun reserveTcpPort(): Int =
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { it.localPort }

    private fun reserveUdpPort(): Int =
        DatagramSocket(InetSocketAddress(InetAddress.getByName("127.0.0.1"), 0)).use { it.localPort }

    private fun socksReady(port: Int): Boolean = runCatching {
        Socket().use { it.connect(InetSocketAddress("127.0.0.1", port), 200) }
    }.isSuccess

    private fun writePrivateConfig(document: String): File {
        val directory = File(service.noBackupFilesDir, "transport-runtime")
        if (!directory.exists() && !directory.mkdirs()) {
            throw IllegalStateException("Unable to create transport runtime directory")
        }
        Os.chmod(directory.absolutePath, 0x1C0) // 0700
        return File.createTempFile("wireguard-", ".json", directory).also {
            it.writeText(document)
            Os.chmod(it.absolutePath, 0x180) // 0600
        }
    }

    private fun stop(process: Process?) {
        process?.let { child ->
            child.destroy()
            if (!child.waitFor(3, TimeUnit.SECONDS)) {
                child.destroyForcibly()
                child.waitFor(2, TimeUnit.SECONDS)
            }
        }
    }

    private fun classifyDiagnostic(line: String): String? {
        val normalized = line.lowercase()
        return when {
            "certificate" in normalized || "tls" in normalized && "verify" in normalized ->
                "WireGuard TLS certificate verification failed"
            "network is unreachable" in normalized -> "The server network is unreachable"
            "permission denied" in normalized || "operation not permitted" in normalized ->
                "Android denied the WireGuard TLS engine permission"
            else -> null
        }
    }
}
