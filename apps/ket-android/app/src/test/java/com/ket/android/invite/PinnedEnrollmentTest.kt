package com.ket.android.invite

import com.ket.android.KetControlApi
import com.ket.android.testSessionManifest
import com.ket.android.testSessionToken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test
import java.net.InetAddress
import java.net.ServerSocket
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.Signature
import java.util.Base64
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

private const val ACCESS_CODE = "Abcdefghijklmnopqrstuvwxyz012345"

/** Exercises `KetControlApi.enroll` against a real loopback HTTP exchange. */
class PinnedEnrollmentTest {
    private val node = newNodeKeyPair()
    private val nodeKey = NodePublicKey.fromBytes(node.public.encoded.takeLast(32).toByteArray())
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
        for (signer in listOf(null, newNodeKeyPair())) {
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

    private fun serveOnce(signer: KeyPair?) = OneShotEnrollmentServer(manifest, signer)
}

/** Answers one `POST /v1/sessions`, signing the body for the request nonce when [signer] is set. */
private class OneShotEnrollmentServer(
    private val manifest: String,
    private val signer: KeyPair?,
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
                val signature = Signature.getInstance("Ed25519").run {
                    initSign(signer.private)
                    update(enrollmentSignatureInput(requestNonce, body))
                    sign()
                }
                "$NODE_SIGNATURE_HEADER: ${Base64.getUrlEncoder().withoutPadding().encodeToString(signature)}\r\n"
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

/** The JDK's Ed25519 (Java 15+) stands in for the server's node key. */
private fun newNodeKeyPair(): KeyPair = KeyPairGenerator.getInstance("Ed25519").generateKeyPair()

/** `KET-ENROLLMENT-V1 || nonce || u32be(len(body)) || body`, written out independently of the app. */
private fun enrollmentSignatureInput(nonceHeader: String, body: ByteArray): ByteArray =
    "KET-ENROLLMENT-V1".toByteArray(Charsets.US_ASCII) +
        Base64.getUrlDecoder().decode(nonceHeader) +
        ByteBuffer.allocate(4).putInt(body.size).array() +
        body
