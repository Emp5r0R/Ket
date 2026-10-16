package com.ket.android.connection

import com.ket.android.AndroidTransportSelector
import com.ket.android.KetProtocol
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TransportFallbacksTest {
    @Test
    fun `reasons accumulate in order and keep only the latest`() {
        var notes = emptyList<TransportFallbackNote>()
        repeat(TransportFallbacks.MAX_NOTES + 3) { index ->
            notes = TransportFallbacks.appended(notes, TransportFallbackNote("T$index", "failed $index"))
        }
        assertEquals(TransportFallbacks.MAX_NOTES, notes.size)
        assertEquals("T3", notes.first().transportName)
        assertEquals("T10", notes.last().transportName)
    }

    @Test
    fun `a chosen protocol the invite no longer offers is named`() {
        val transports = AndroidTransportSelector.parse(JSONArray().put(stealth()))
        assertNull(TransportFallbacks.preferredUnavailable(transports, null))
        assertNull(TransportFallbacks.preferredUnavailable(transports, KetProtocol.Stealth))
        assertEquals(
            "VLESS + REALITY is not available for this invite; choose Automatic or another protocol",
            TransportFallbacks.preferredUnavailable(transports, KetProtocol.Reality),
        )
    }

    @Test
    fun `the heading names the protocol in use`() {
        assertEquals("Using HTTPS Stealth because:", fallbackHeading("HTTPS Stealth"))
        assertEquals("Tried:", fallbackHeading(null))
        assertEquals("Tried:", fallbackHeading(""))
    }

    private fun stealth(): JSONObject = JSONObject()
        .put("id", "https-stealth-primary")
        .put("protocol", "stealth")
        .put("endpoint", "stealth.example.test")
        .put("port", 443)
        .put("network", "tcp")
        .put("priority", 1)
        .put("tls_server_name", "stealth.example.test")
        .put(
            "options",
            JSONObject()
                .put("encryption", "none")
                .put("fingerprint", "chrome")
                .put("mode", "packet-up")
                .put("path", "/a1b2c3d4e5f6g7h8")
                .put("security", "tls")
                .put("transport", "xhttp"),
        )
        .put(
            "credential",
            JSONObject()
                .put("auth", "550e8400-e29b-41d4-a716-446655440000")
                .put("secrets", JSONObject()),
        )
}
