package com.ket.android

import java.io.InputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket

internal data class AndroidEngineStarted(
    val socksPort: Int,
    val handshakeLatencyMs: Long,
    val bypassAddress: InetAddress?,
)

internal interface AndroidTransportEngine : AutoCloseable {
    val displayName: String
    fun start(): AndroidEngineStarted
    fun isAlive(): Boolean
}

internal fun verifySocksTunnel(port: Int, target: String) {
    val targetBytes = target.toByteArray(Charsets.US_ASCII)
    require(targetBytes.size <= 255) { "SOCKS target is too long" }
    Socket().use { socket ->
        socket.soTimeout = 10_000
        socket.connect(InetSocketAddress("127.0.0.1", port), 2_000)
        val output = socket.getOutputStream()
        val input = socket.getInputStream()
        output.write(byteArrayOf(5, 1, 0))
        output.flush()
        require(input.read() == 5 && input.read() == 0) { "Local SOCKS authentication failed" }
        output.write(byteArrayOf(5, 1, 0, 3, targetBytes.size.toByte()))
        output.write(targetBytes)
        output.write(byteArrayOf(1, -69)) // Port 443.
        output.flush()
        require(input.read() == 5 && input.read() == 0) { "The protected server rejected the connection probe" }
        input.read() // Reserved byte.
        when (input.read()) {
            1 -> readExact(input, 4)
            3 -> readExact(input, input.read().also { require(it >= 0) { "Local SOCKS response ended early" } })
            4 -> readExact(input, 16)
            else -> throw IllegalStateException("Local SOCKS response is invalid")
        }
        readExact(input, 2)
    }
}

private fun readExact(input: InputStream, length: Int) {
    val buffer = ByteArray(length)
    var offset = 0
    while (offset < length) {
        val read = input.read(buffer, offset, length - offset)
        require(read > 0) { "Local SOCKS response ended early" }
        offset += read
    }
}
