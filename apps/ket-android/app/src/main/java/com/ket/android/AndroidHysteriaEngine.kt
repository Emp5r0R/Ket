package com.ket.android

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.os.ParcelFileDescriptor
import android.system.Os
import java.io.File
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

internal class AndroidHysteriaEngine(
    private val service: KetVpnService,
    private val transport: HysteriaTransport,
) : AndroidTransportEngine {
    private var process: Process? = null
    private var protector: FdProtectionServer? = null
    private var configFile: File? = null
    private val diagnostic = AtomicReference<String?>()
    private val connected = AtomicBoolean()

    override val displayName: String = transport.displayName

    override fun start(): AndroidEngineStarted {
        val startedAt = System.nanoTime()
        val engine = File(service.applicationInfo.nativeLibraryDir, "libhysteria.so")
        require(engine.isFile && engine.canExecute()) { "Hysteria2 engine is not installed for this device" }
        val resolved = resolveServer(transport.endpoint)
        val socksPort = reservePort()
        val socketName = "ket_hy2_${android.os.Process.myPid()}_${System.nanoTime()}"
        protector = FdProtectionServer(service, socketName).also { it.start() }
        configFile = writePrivateConfig(
            EngineConfig.hysteria(transport, resolved.hostAddress ?: error("Server address is unavailable"), socketName, socksPort),
        )
        val child = ProcessBuilder(engine.absolutePath, "-c", configFile!!.absolutePath)
            .redirectErrorStream(true)
            .apply {
                environment()["HYSTERIA_DISABLE_UPDATE_CHECK"] = "1"
                environment()["HYSTERIA_LOG_FORMAT"] = "json"
                environment()["HYSTERIA_LOG_LEVEL"] = "info"
            }
            .start()
        process = child
        thread(name = "ket-hysteria-log", isDaemon = true) {
            child.inputStream.bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    if (line.contains("connected to server")) connected.set(true)
                    classifyDiagnostic(line)?.let { diagnostic.compareAndSet(null, it) }
                }
            }
        }
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(20)
        while (System.nanoTime() < deadline) {
            if (!child.isAlive) throw IllegalStateException(diagnostic.get() ?: "Hysteria2 exited during startup")
            if (connected.get() && socksReady(socksPort)) {
                configFile?.delete()
                configFile = null
                return AndroidEngineStarted(
                    socksPort,
                    TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedAt),
                    null,
                )
            }
            Thread.sleep(100)
        }
        throw IllegalStateException(diagnostic.get() ?: "Hysteria2 connection timed out")
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
        protector?.close()
        protector = null
    }

    private fun resolveServer(host: String): InetAddress {
        val addresses = InetAddress.getAllByName(host)
        return addresses.firstOrNull { it is Inet4Address } ?: addresses.firstOrNull()
            ?: throw IllegalStateException("Server DNS returned no addresses")
    }

    private fun reservePort(): Int = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { it.localPort }

    private fun socksReady(port: Int): Boolean = runCatching {
        Socket().use { it.connect(InetSocketAddress("127.0.0.1", port), 200) }
    }.isSuccess

    private fun writePrivateConfig(document: String): File {
        val directory = File(service.noBackupFilesDir, "transport-runtime")
        if (!directory.exists() && !directory.mkdirs()) throw IllegalStateException("Unable to create transport runtime directory")
        Os.chmod(directory.absolutePath, 0x1C0) // 0700
        return File.createTempFile("hysteria-", ".json", directory).also {
            it.writeText(document)
            Os.chmod(it.absolutePath, 0x180) // 0600
        }
    }

    private fun classifyDiagnostic(line: String): String? {
        val normalized = line.lowercase()
        return when {
            "authentication failed" in normalized || "authentication error" in normalized ->
                "The server rejected the transport credential"
            "certificate" in normalized || "tls verification" in normalized ->
                "Server certificate verification failed"
            "network is unreachable" in normalized -> "The server network is unreachable"
            "connect error" in normalized || "no recent network activity" in normalized ->
                "The Hysteria2 server did not respond"
            "permission denied" in normalized || "operation not permitted" in normalized ->
                "Android denied the transport engine permission"
            else -> null
        }
    }
}

private class FdProtectionServer(
    private val service: KetVpnService,
    val name: String,
) : AutoCloseable {
    private val running = AtomicBoolean()
    private var server: LocalServerSocket? = null
    private var worker: Thread? = null

    fun start() {
        check(running.compareAndSet(false, true)) { "FD protection server is already running" }
        server = LocalServerSocket(name)
        worker = thread(name = "ket-fd-protection", isDaemon = true) {
            while (running.get()) {
                val client = try {
                    server?.accept() ?: break
                } catch (_: Exception) {
                    break
                }
                client.use(::protectDescriptor)
            }
        }
    }

    override fun close() {
        running.set(false)
        runCatching { server?.close() }
        worker?.join(1_000)
        worker = null
        server = null
    }

    private fun protectDescriptor(client: LocalSocket) {
        val descriptor = ParcelFileDescriptor.dup(client.fileDescriptor).use { socket ->
            FdReceiver.receiveFileDescriptor(socket.fd)
        }
        if (descriptor < 0) return
        val protected = ParcelFileDescriptor.adoptFd(descriptor).use { received ->
            service.protect(received.fd)
        }
        if (protected) Os.write(client.fileDescriptor, byteArrayOf(1), 0, 1)
    }
}

internal object FdReceiver {
    init {
        System.loadLibrary("ket-android-native")
    }

    external fun receiveFileDescriptor(socketFd: Int): Int
}
