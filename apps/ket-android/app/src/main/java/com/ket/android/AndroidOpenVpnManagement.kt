package com.ket.android

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.net.LocalSocketAddress
import android.os.ParcelFileDescriptor
import android.system.Os
import java.io.Closeable
import java.io.File
import java.io.FileDescriptor
import java.io.IOException
import java.net.Inet4Address
import java.net.InetAddress
import java.util.ArrayDeque
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

internal data class OpenVpnTunRequest(
    val ipv4Address: InetAddress,
    val ipv4PrefixLength: Int,
    val ipv6Address: InetAddress?,
    val ipv6PrefixLength: Int?,
    val mtu: Int,
    val dnsServers: List<InetAddress>,
)

/** Implements the Android OpenVPN 2 management callbacks without importing another VpnService. */
internal class AndroidOpenVpnManagement(
    private val service: KetVpnService,
    private val socketFile: File,
    private val username: String,
    private val password: String,
    private val openTun: (OpenVpnTunRequest) -> ParcelFileDescriptor,
) : Closeable {
    private val connected = AtomicBoolean()
    private val failure = AtomicReference<String?>()
    private val bytesIn = AtomicLong()
    private val bytesOut = AtomicLong()
    private val closing = AtomicBoolean()
    private val descriptors = ArrayDeque<FileDescriptor>()
    private var localServerSocket: LocalServerSocket? = null
    private var boundSocket: LocalSocket? = null
    private var clientSocket: LocalSocket? = null
    private var worker: Thread? = null
    private var tun = MutableTunConfig()

    val diagnostic: String?
        get() = failure.get()

    fun start() {
        socketFile.delete()
        val bound = LocalSocket()
        bound.bind(LocalSocketAddress(socketFile.absolutePath, LocalSocketAddress.Namespace.FILESYSTEM))
        Os.chmod(socketFile.absolutePath, 0x180) // 0600
        val server = LocalServerSocket(bound.fileDescriptor)
        boundSocket = bound
        localServerSocket = server
        worker = thread(name = "ket-openvpn-management", isDaemon = true) { runServer(server) }
    }

    fun awaitConnected(process: Process, cancelled: () -> Boolean, timeoutSeconds: Long) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(timeoutSeconds)
        while (System.nanoTime() < deadline) {
            ensureEngineStartActive(cancelled)
            failure.get()?.let { throw IllegalStateException(it) }
            if (!process.isAlive) throw IllegalStateException("OpenVPN exited during startup")
            if (connected.get()) return
            Thread.sleep(100)
        }
        throw IllegalStateException("OpenVPN handshake timed out")
    }

    fun isConnected(): Boolean = connected.get() && failure.get() == null && worker?.isAlive == true

    fun trafficStats(): LongArray = longArrayOf(0, bytesOut.get(), 0, bytesIn.get())

    @Synchronized
    fun signalStop() {
        send("signal SIGINT\n")
    }

    private fun runServer(server: LocalServerSocket) {
        try {
            val socket = server.accept()
            clientSocket = socket
            runCatching { server.close() }
            localServerSocket = null
            send("version 3\n")
            val buffer = ByteArray(4_096)
            var pending = ""
            val input = socket.inputStream
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                socket.ancillaryFileDescriptors?.forEach(descriptors::addLast)
                pending += String(buffer, 0, read, Charsets.UTF_8)
                require(pending.length <= MAX_PENDING_CHARS) { "OpenVPN management message is too large" }
                while ('\n' in pending) {
                    val line = pending.substringBefore('\n').trimEnd('\r')
                    pending = pending.substringAfter('\n')
                    process(line)
                }
            }
            if (connected.get()) failure.compareAndSet(null, "OpenVPN management channel closed")
        } catch (error: Exception) {
            if (!closing.get()) failure.compareAndSet(null, classify(error))
        } finally {
            connected.set(false)
            closeDescriptors()
        }
    }

    private fun process(line: String) {
        when {
            line.startsWith(">HOLD:") -> {
                send("hold release\n")
                send("bytecount 1\n")
                send("state on\n")
            }
            line.startsWith(">PASSWORD:") -> handlePassword(line.substringAfter(':'))
            line.startsWith(">NEED-OK:") -> handleNeedOk(line.substringAfter(':'))
            line.startsWith(">STATE:") -> handleState(line.substringAfter(':'))
            line.startsWith(">BYTECOUNT:") -> handleByteCount(line.substringAfter(':'))
            line.startsWith(">FATAL:") -> failure.compareAndSet(
                null,
                sanitizeDiagnostic(line.substringAfter(':')),
            )
            line.startsWith("PROTECTFD:") -> protectDescriptor(respond = false)
            line.startsWith(">INFO:") || line.startsWith("SUCCESS:") || line.isBlank() -> Unit
        }
    }

    private fun handlePassword(argument: String) {
        if (argument.startsWith("Auth-Token:")) return
        if (argument.startsWith("Verification Failed")) {
            failure.compareAndSet(null, "OpenVPN rejected the scoped credential")
            return
        }
        val requested = quotedName(argument)
        if (requested != "Auth") {
            failure.compareAndSet(null, "OpenVPN requested unsupported credentials")
            return
        }
        send("username 'Auth' $username\n")
        send("password 'Auth' $password\n")
    }

    private fun handleNeedOk(argument: String) {
        val requested = quotedName(argument)
        val extra = argument.substringAfter(':', "").trim()
        try {
            when (requested) {
                "PROTECTFD" -> protectDescriptor(respond = true)
                "IFCONFIG" -> {
                    tun.applyIfConfig(extra)
                    reply(requested, "ok")
                }
                "IFCONFIG6" -> {
                    tun.applyIfConfig6(extra)
                    reply(requested, "ok")
                }
                "DNSSERVER", "DNS6SERVER" -> {
                    tun.addDns(extra)
                    reply(requested, "ok")
                }
                "DNSDOMAIN", "ROUTE", "ROUTE6" -> {
                    require(extra.length <= 1_024 && extra.none(Char::isISOControl)) {
                        "OpenVPN pushed invalid route metadata"
                    }
                    reply(requested, "ok")
                }
                "PERSIST_TUN_ACTION" -> reply(requested, "OPEN_BEFORE_CLOSE")
                "OPENTUN" -> sendTun(requested, extra)
                else -> {
                    reply(requested, "cancel")
                    failure.compareAndSet(null, "OpenVPN requested unsupported Android operation")
                }
            }
        } catch (error: Exception) {
            reply(requested, "cancel")
            failure.compareAndSet(null, error.message ?: "OpenVPN supplied invalid tunnel settings")
        }
    }

    private fun sendTun(requested: String, extra: String) {
        require(extra == "tun") { "OpenVPN requested an unsupported device type" }
        val descriptor = openTun(tun.validated())
        try {
            val socket = clientSocket ?: throw IOException("OpenVPN management socket is unavailable")
            socket.setFileDescriptorsForSend(arrayOf(descriptor.fileDescriptor))
            reply(requested, "ok")
            socket.setFileDescriptorsForSend(null)
        } finally {
            runCatching { clientSocket?.setFileDescriptorsForSend(null) }
            descriptor.close()
        }
        tun = MutableTunConfig()
    }

    private fun protectDescriptor(respond: Boolean) {
        val descriptor = descriptors.pollFirst()
            ?: throw IllegalStateException("OpenVPN did not attach the socket descriptor to protect")
        val protected = try {
            ParcelFileDescriptor.dup(descriptor).use { service.protect(it.fd) }
        } finally {
            runCatching { Os.close(descriptor) }
        }
        if (respond) reply("PROTECTFD", if (protected) "ok" else "cancel")
        require(protected) { "Android could not protect the OpenVPN socket" }
    }

    private fun handleState(argument: String) {
        val fields = argument.split(',', limit = 4)
        if (fields.size >= 3 && fields[1] == "CONNECTED" && fields[2] == "SUCCESS") {
            connected.set(true)
        } else if (fields.size >= 2 && fields[1] == "AUTH_FAILED") {
            failure.compareAndSet(null, "OpenVPN rejected the scoped credential")
        }
    }

    private fun handleByteCount(argument: String) {
        val fields = argument.split(',', limit = 2)
        if (fields.size != 2) return
        val received = fields[0].toLongOrNull() ?: return
        val sent = fields[1].toLongOrNull() ?: return
        if (received >= 0 && sent >= 0) {
            bytesIn.set(received)
            bytesOut.set(sent)
        }
    }

    private fun quotedName(argument: String): String {
        val start = argument.indexOf('\'')
        val end = argument.indexOf('\'', start + 1)
        require(start >= 0 && end > start) { "OpenVPN management request is malformed" }
        return argument.substring(start + 1, end)
    }

    private fun reply(requested: String, status: String) {
        require(requested.matches(Regex("[A-Z0-9_]{1,32}"))) { "OpenVPN request name is invalid" }
        send("needok '$requested' $status\n")
    }

    @Synchronized
    private fun send(command: String) {
        val socket = clientSocket ?: return
        socket.outputStream.write(command.toByteArray(Charsets.US_ASCII))
        socket.outputStream.flush()
    }

    private fun sanitizeDiagnostic(message: String): String {
        val safe = message.take(256).filter { !it.isISOControl() }
        return if (safe.isBlank()) "OpenVPN reported a fatal error" else "OpenVPN failed: $safe"
    }

    private fun classify(error: Exception): String = when (error) {
        is InterruptedException -> "OpenVPN management was interrupted"
        else -> error.message?.takeIf(String::isNotBlank) ?: "OpenVPN management failed"
    }

    private fun closeDescriptors() {
        while (descriptors.isNotEmpty()) runCatching { Os.close(descriptors.removeFirst()) }
    }

    override fun close() {
        val activeWorker = synchronized(this) {
            closing.set(true)
            connected.set(false)
            runCatching { clientSocket?.close() }
            clientSocket = null
            runCatching { localServerSocket?.close() }
            localServerSocket = null
            runCatching { boundSocket?.close() }
            boundSocket = null
            worker.also { worker = null }
        }
        activeWorker?.join(2_000)
        closeDescriptors()
        socketFile.delete()
    }

    private class MutableTunConfig {
        private var ipv4Address: InetAddress? = null
        private var ipv4PrefixLength: Int? = null
        private var ipv6Address: InetAddress? = null
        private var ipv6PrefixLength: Int? = null
        private var mtu: Int? = null
        private val dnsServers = linkedSetOf<InetAddress>()

        fun applyIfConfig(extra: String) {
            val fields = extra.split(Regex("\\s+"))
            require(fields.size == 4 && fields[3] == "subnet") {
                "OpenVPN supplied unsupported IPv4 interface settings"
            }
            val address = numericAddress(fields[0])
            require(address is Inet4Address && address.isSiteLocalAddress) {
                "OpenVPN supplied an invalid IPv4 interface address"
            }
            val prefix = netmaskPrefix(fields[1])
            val configuredMtu = fields[2].toIntOrNull()
            require(prefix in 8..32 && configuredMtu != null && configuredMtu in 1_000..1_500) {
                "OpenVPN supplied invalid IPv4 interface settings"
            }
            ipv4Address = address
            ipv4PrefixLength = prefix
            mtu = configuredMtu
        }

        fun applyIfConfig6(extra: String) {
            val fields = extra.split(Regex("\\s+"))
            require(fields.size >= 2) { "OpenVPN supplied invalid IPv6 interface settings" }
            val addressAndPrefix = fields[0].split('/', limit = 2)
            require(addressAndPrefix.size == 2) { "OpenVPN supplied invalid IPv6 interface settings" }
            val address = numericAddress(addressAndPrefix[0])
            val prefix = addressAndPrefix[1].toIntOrNull()
            val configuredMtu = fields[1].toIntOrNull()
            require(
                address !is Inet4Address &&
                    prefix != null && prefix in 1..128 &&
                    configuredMtu != null && configuredMtu in 1_000..1_500,
            ) {
                "OpenVPN supplied invalid IPv6 interface settings"
            }
            ipv6Address = address
            ipv6PrefixLength = prefix
            mtu = configuredMtu
        }

        fun addDns(value: String) {
            require(dnsServers.size < 4) { "OpenVPN supplied too many DNS servers" }
            val address = numericAddress(value)
            require(!address.isAnyLocalAddress && !address.isLoopbackAddress && !address.isMulticastAddress) {
                "OpenVPN supplied an invalid DNS server"
            }
            dnsServers += address
        }

        fun validated(): OpenVpnTunRequest {
            val address = requireNotNull(ipv4Address) { "OpenVPN did not supply an IPv4 interface" }
            val prefix = requireNotNull(ipv4PrefixLength) { "OpenVPN did not supply an IPv4 prefix" }
            val configuredMtu = requireNotNull(mtu) { "OpenVPN did not supply an MTU" }
            require(dnsServers.isNotEmpty()) { "OpenVPN did not supply a DNS server" }
            return OpenVpnTunRequest(
                address,
                prefix,
                ipv6Address,
                ipv6PrefixLength,
                configuredMtu,
                dnsServers.toList(),
            )
        }

        private fun netmaskPrefix(value: String): Int {
            val address = numericAddress(value)
            require(address is Inet4Address) { "OpenVPN supplied an invalid IPv4 netmask" }
            var seenZero = false
            var prefix = 0
            address.address.forEach { byte ->
                for (bit in 7 downTo 0) {
                    val set = byte.toInt() and (1 shl bit) != 0
                    require(!set || !seenZero) { "OpenVPN supplied a non-contiguous IPv4 netmask" }
                    if (set) prefix++ else seenZero = true
                }
            }
            return prefix
        }

        private fun numericAddress(value: String): InetAddress {
            require(
                value.isNotEmpty() &&
                    (value.all { it.isDigit() || it == '.' } || value.all {
                        it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' || it == ':' || it == '.'
                    }),
            ) { "OpenVPN supplied a non-numeric network address" }
            return InetAddress.getByName(value)
        }
    }

    private companion object {
        const val MAX_PENDING_CHARS = 64 * 1024
    }
}
