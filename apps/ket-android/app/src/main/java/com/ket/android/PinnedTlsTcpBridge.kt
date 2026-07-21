package com.ket.android

import android.net.VpnService
import java.io.ByteArrayInputStream
import java.io.Closeable
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.security.KeyStore
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import javax.net.ssl.SNIHostName
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManagerFactory
import kotlin.concurrent.thread

/** A certificate-pinned TLS client carrier compatible with the server-side stunnel listener. */
internal class PinnedTlsTcpBridge(
    private val service: VpnService,
    private val transport: OpenVpnStunnelTransport,
    private val resolvedEndpoint: InetAddress,
) : Closeable {
    private val running = AtomicBoolean()
    private val failure = AtomicReference<String?>()
    private val listener = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1"))
    private val socketFactory = pinnedContext(transport.stunnelCaCertificate).socketFactory
    private var worker: Thread? = null
    @Volatile
    private var localSocket: Socket? = null
    @Volatile
    private var remoteSocket: Socket? = null

    val port: Int = listener.localPort
    val diagnostic: String?
        get() = failure.get()

    fun start() {
        check(running.compareAndSet(false, true)) { "OpenVPN TLS carrier is already running" }
        worker = thread(name = "ket-openvpn-tls", isDaemon = true) {
            while (running.get()) {
                try {
                    listener.accept().use { local -> bridge(local) }
                } catch (error: IOException) {
                    if (running.get()) {
                        failure.compareAndSet(null, classify(error))
                    }
                } catch (error: Exception) {
                    if (running.get()) {
                        failure.compareAndSet(null, classify(error))
                    }
                }
            }
        }
    }

    fun isAlive(): Boolean = running.get() && worker?.isAlive == true && listener.isClosed.not()

    private fun bridge(local: Socket) {
        localSocket = local
        val raw = Socket()
        remoteSocket = raw
        try {
            require(service.protect(raw)) { "Android could not protect the OpenVPN carrier socket" }
            raw.connect(InetSocketAddress(resolvedEndpoint, transport.port), CONNECT_TIMEOUT_MILLIS)
            val tls = socketFactory.createSocket(
                raw,
                transport.tlsServerName,
                transport.port,
                true,
            ) as SSLSocket
            remoteSocket = tls
            tls.enabledProtocols = tls.supportedProtocols.filter {
                it == "TLSv1.3" || it == "TLSv1.2"
            }.toTypedArray()
            tls.sslParameters = tls.sslParameters.apply {
                endpointIdentificationAlgorithm = "HTTPS"
                serverNames = listOf(SNIHostName(transport.tlsServerName))
            }
            tls.startHandshake()

            val upstream = thread(name = "ket-openvpn-tls-upstream", isDaemon = true) {
                runCatching {
                    local.getInputStream().use { input ->
                        tls.getOutputStream().use(input::copyTo)
                    }
                }
                runCatching { tls.shutdownOutput() }
            }
            runCatching {
                tls.getInputStream().use { input ->
                    local.getOutputStream().use(input::copyTo)
                }
            }
            runCatching { local.shutdownOutput() }
            upstream.join(2_000)
        } finally {
            runCatching { remoteSocket?.close() }
            remoteSocket = null
            localSocket = null
        }
    }

    @Synchronized
    override fun close() {
        running.set(false)
        runCatching { listener.close() }
        runCatching { localSocket?.close() }
        runCatching { remoteSocket?.close() }
        localSocket = null
        remoteSocket = null
        worker?.join(2_000)
        worker = null
    }

    private fun classify(error: Exception): String {
        val message = error.message.orEmpty().lowercase()
        return when {
            "certificate" in message || "handshake" in message || "hostname" in message ->
                "OpenVPN carrier certificate verification failed"
            "network is unreachable" in message || "no route to host" in message ->
                "The OpenVPN server network is unreachable"
            else -> "The OpenVPN TLS carrier failed"
        }
    }

    private fun pinnedContext(pem: String): SSLContext {
        val certificates = CertificateFactory.getInstance("X.509")
            .generateCertificates(ByteArrayInputStream(pem.toByteArray(Charsets.US_ASCII)))
            .map {
                require(it is X509Certificate) { "OpenVPN carrier trust material is invalid" }
                it.checkValidity()
                it
            }
        require(certificates.isNotEmpty()) { "OpenVPN carrier trust material is empty" }
        val store = KeyStore.getInstance(KeyStore.getDefaultType()).apply {
            load(null)
            certificates.forEachIndexed { index, certificate ->
                setCertificateEntry("ket-openvpn-carrier-$index", certificate)
            }
        }
        val managers = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm()).apply {
            init(store)
        }
        return SSLContext.getInstance("TLS").apply { init(null, managers.trustManagers, null) }
    }

    private companion object {
        const val CONNECT_TIMEOUT_MILLIS = 10_000
    }
}
