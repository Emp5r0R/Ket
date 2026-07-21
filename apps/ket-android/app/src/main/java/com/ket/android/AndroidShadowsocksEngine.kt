package com.ket.android

import android.os.Build
import android.system.Os
import java.io.File
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

internal class AndroidShadowsocksEngine(
    private val service: KetVpnService,
    private val transport: ShadowsocksTransport,
    private val resolvedEndpoint: InetAddress,
) : AndroidTransportEngine {
    private var process: Process? = null
    private var configFile: File? = null
    private val diagnostic = AtomicReference<String?>()

    override val displayName: String = transport.displayName

    override fun start(cancelled: () -> Boolean): AndroidEngineStarted {
        val startedAt = System.nanoTime()
        ensureEngineStartActive(cancelled)
        require(Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            "Shadowsocks requires Android 9 or newer on this architecture"
        }
        val engine = File(service.applicationInfo.nativeLibraryDir, "libsslocal.so")
        require(engine.isFile && engine.canExecute()) {
            "Shadowsocks is not installed for this device architecture"
        }
        val socksPort = reservePort()
        configFile = writePrivateConfig(
            EngineConfig.shadowsocks(
                transport,
                resolvedEndpoint.hostAddress ?: error("Server address is unavailable"),
                socksPort,
            ),
        )
        val child = ProcessBuilder(engine.absolutePath, "--config", configFile!!.absolutePath)
            .redirectErrorStream(true)
            .start()
        process = child
        thread(name = "ket-shadowsocks-log", isDaemon = true) {
            consumeProcessOutput(child.inputStream) { line ->
                classifyDiagnostic(line)?.let { diagnostic.compareAndSet(null, it) }
            }
        }
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(20)
        while (System.nanoTime() < deadline) {
            ensureEngineStartActive(cancelled)
            if (!child.isAlive) {
                throw IllegalStateException(diagnostic.get() ?: "Shadowsocks exited during startup")
            }
            if (socksReady(socksPort)) {
                try {
                    verifySocksTunnel(socksPort, "one.one.one.one")
                    configFile?.delete()
                    configFile = null
                    return AndroidEngineStarted(
                        socksPort,
                        TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedAt),
                        resolvedEndpoint,
                    )
                } catch (error: Exception) {
                    diagnostic.compareAndSet(null, error.message)
                }
            }
            Thread.sleep(100)
        }
        throw IllegalStateException(diagnostic.get() ?: "Shadowsocks handshake timed out")
    }

    override fun isAlive(): Boolean = process?.isAlive == true

    override fun close() {
        configFile?.delete()
        configFile = null
        process?.let { child ->
            child.destroy()
            if (!child.waitFor(3, TimeUnit.SECONDS)) {
                child.destroyForcibly()
                child.waitFor(2, TimeUnit.SECONDS)
            }
        }
        process = null
    }

    private fun reservePort(): Int =
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { it.localPort }

    private fun socksReady(port: Int): Boolean = runCatching {
        Socket().use { it.connect(InetSocketAddress("127.0.0.1", port), 200) }
    }.isSuccess

    private fun writePrivateConfig(document: String): File {
        val directory = File(service.noBackupFilesDir, "transport-runtime")
        if (!directory.exists() && !directory.mkdirs()) {
            throw IllegalStateException("Unable to create transport runtime directory")
        }
        Os.chmod(directory.absolutePath, 0x1C0) // 0700
        return File.createTempFile("shadowsocks-", ".json", directory).also {
            it.writeText(document)
            Os.chmod(it.absolutePath, 0x180) // 0600
        }
    }

    private fun classifyDiagnostic(line: String): String? {
        val normalized = line.lowercase()
        return when {
            "invalid" in normalized && ("password" in normalized || "key" in normalized) ->
                "The server rejected the Shadowsocks credential"
            "network is unreachable" in normalized -> "The server network is unreachable"
            "permission denied" in normalized || "operation not permitted" in normalized ->
                "Android denied the Shadowsocks engine permission"
            else -> null
        }
    }
}
