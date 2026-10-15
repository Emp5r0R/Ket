package com.ket.android.invite

import com.google.crypto.tink.subtle.Ed25519Sign
import com.ket.android.KetControlApi
import com.ket.android.testSessionManifest
import com.ket.android.testSessionToken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test
import java.net.InetAddress
import java.net.ServerSocket
import java.nio.charset.StandardCharsets
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

private const val ACCESS_CODE = "Abcdefghijklmnopqrstuvwxyz012345"

/** Exercises `KetControlApi.enroll` against a real loopback HTTP exchange. */
class PinnedEnrollmentTest {
    private val node = Ed25519Sign.KeyPair.newKeyPair()
    private val nodeKey = NodePublicKey.fromBytes(node.publicKey)
    private val manifest = testSessionManifest(testSessionToken('P'))

    @Test
    fun `pinned enrollment accepts the node signature over the exact manifest`() {
        val nonce = serveOnce(signer = node).use { server ->
            val result = KetControlApi.enroll(server.baseUrl, ACCESS_CODE, "Ket Android", nodeKey)
            assertEquals(testSessionToken('P'), result.token)
            server.requestNonce()
        }
        assertEquals(43, nonce?.length)
    }

    @Test
    fun `pinned enrollment rejects missing or foreign signatures`() {
        for (signer in listOf(null, Ed25519Sign.KeyPair.newKeyPair())) {
            serveOnce(signer = signer).use { server ->
                val error = assertThrows(InviteException::class.java) {
                    KetControlApi.enroll(server.baseUrl, ACCESS_CODE, "Ket Android", nodeKey)
                }
                assertEquals(InviteFailure.NodeIdentityMismatch, error.failure)
            }
        }
    }

    @Test
    fun `unpinned enrollment sends no nonce and ignores signatures`() {
        serveOnce(signer = null).use { server ->
            KetControlApi.enroll(server.baseUrl, ACCESS_CODE, "Ket Android")
            assertNull(server.requestNonce())
        }
    }

    private fun serveOnce(signer: Ed25519Sign.KeyPair?) = OneShotEnrollmentServer(manifest, signer)
}

/** Answers one `POST /v1/sessions`, signing the body for the request nonce when [signer] is set. */
private class OneShotEnrollmentServer(
    private val manifest: String,
    private val signer: Ed25519Sign.KeyPair?,
) : AutoCloseable {
    private val socket = ServerSocket(0, 1, InetAddress.getLoopbackAddress())
    private val nonce = CompletableFuture<String?>()
    val baseUrl = "http://127.0.0.1:${socket.localPort}"

    init {
        Thread(::serve, "one-shot-enrollment-server").apply { isDaemon = true }.start()
    }

    fun requestNonce(): String? = nonce.get(5, TimeUnit.SECONDS)

    private fun serve() {
        socket.accept().use { connection ->
            val input = connection.getInputStream().bufferedReader(StandardCharsets.UTF_8)
            val headers = generateSequence { input.readLine()?.takeIf(String::isNotEmpty) }
                .drop(1)
                .associate { line -> line.substringBefore(':').lowercase() to line.substringAfter(':').trim() }
            val length = headers["content-length"]?.toInt() ?: 0
            CharArray(length).also { buffer -> var read = 0; while (read < length) read += input.read(buffer, read, length - read) }
            val requestNonce = headers[ENROLLMENT_NONCE_HEADER.lowercase()]
            nonce.complete(requestNonce)
            val body = manifest.toByteArray(StandardCharsets.UTF_8)
            val signatureHeader = if (signer != null && requestNonce != null) {
                val signed = EnrollmentProof.signatureInput(EnrollmentNonce.fromHeaderValue(requestNonce), body)
                "$NODE_SIGNATURE_HEADER: ${Base64Url.encode(Ed25519Sign(signer.privateKey).sign(signed))}\r\n"
            } else {
                ""
            }
            connection.getOutputStream().apply {
                write(
                    ("HTTP/1.1 201 Created\r\nContent-Type: application/json\r\n" +
                        "Content-Length: ${body.size}\r\n${signatureHeader}Connection: close\r\n\r\n")
                        .toByteArray(StandardCharsets.US_ASCII),
                )
                write(body)
                flush()
            }
        }
    }

    override fun close() = socket.close()
}
