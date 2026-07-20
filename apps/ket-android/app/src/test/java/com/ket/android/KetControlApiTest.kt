package com.ket.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class KetControlApiTest {
    @Test
    fun `authorization status codes are terminal`() {
        assertTrue(KetControlException(401, "unauthorized").authorizationLost)
        assertTrue(KetControlException(403, "forbidden").authorizationLost)
        assertFalse(KetControlException(500, "server error").authorizationLost)
    }

    @Test
    fun `session status carries strict node and nonnegative traffic telemetry`() {
        val body = JSONObject()
            .put("node", testNodeJson())
            .put(
                "traffic",
                JSONObject()
                    .put("available", true)
                    .put("bytes_sent", 1_024)
                    .put("bytes_received", 8_192)
                    .put("online_connections", 2)
                    .put("observed_at_epoch_seconds", 4_000_000_000),
            )
            .toString()

        val telemetry = KetControlApi.parseStatus(body)

        assertEquals("Test node", telemetry.node.displayName)
        assertTrue(telemetry.available)
        assertEquals(1_024L, telemetry.sent)
        assertEquals(8_192L, telemetry.received)
        assertEquals(2, telemetry.online)
    }

    @Test
    fun `session status rejects negative traffic counters`() {
        val body = JSONObject()
            .put("node", testNodeJson())
            .put(
                "traffic",
                JSONObject()
                    .put("available", true)
                    .put("bytes_sent", -1)
                    .put("bytes_received", 0)
                    .put("online_connections", 1),
            )
            .toString()

        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.parseStatus(body)
        }
    }
}
