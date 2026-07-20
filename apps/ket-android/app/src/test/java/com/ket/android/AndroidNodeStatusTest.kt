package com.ket.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidNodeStatusTest {
    @Test
    fun `parses complete node geography health capacity and system telemetry`() {
        val node = AndroidNodeStatusParser.parse(testNodeJson())

        assertEquals("node-test-1", node.id)
        assertEquals("Frankfurt, Testland", node.location.displayName)
        assertEquals("TL", node.location.countryCode)
        assertEquals(50.1109, node.location.latitude, 0.000001)
        assertEquals(8.6821, node.location.longitude, 0.000001)
        assertEquals(AndroidNodeHealth.Healthy, node.health)
        assertEquals(12, node.activeSessions)
        assertEquals(120, node.sessionCapacity)
        assertEquals(10.0, node.capacityPercent, 0.001)
        assertEquals(18.5, node.cpuLoadPercent ?: -1.0, 0.001)
        assertEquals(2_147_483_648, node.memoryUsedBytes)
        assertEquals(8_589_934_592, node.memoryTotalBytes)
        assertEquals(86_400L, node.uptimeSeconds)
    }

    @Test
    fun `accepts unavailable optional host telemetry`() {
        val json = testNodeJson()
            .put("cpu_load_percent", JSONObject.NULL)
            .put("memory_used_bytes", JSONObject.NULL)
            .put("memory_total_bytes", JSONObject.NULL)
            .put("uptime_seconds", JSONObject.NULL)

        val node = AndroidNodeStatusParser.parse(json)

        assertNull(node.cpuLoadPercent)
        assertNull(node.memoryUsedBytes)
        assertNull(node.memoryTotalBytes)
        assertNull(node.uptimeSeconds)
    }

    @Test
    fun `rejects out of range geography and capacity telemetry`() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidNodeStatusParser.parse(
                testNodeJson().put(
                    "location",
                    testNodeJson().getJSONObject("location").put("latitude", 91),
                ),
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidNodeStatusParser.parse(testNodeJson().put("capacity_percent", 101))
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidNodeStatusParser.parse(testNodeJson().put("active_sessions", 121))
        }
    }

    @Test
    fun `rejects unknown health and inconsistent memory telemetry`() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidNodeStatusParser.parse(testNodeJson().put("health", "offline"))
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidNodeStatusParser.parse(
                testNodeJson().put("memory_total_bytes", JSONObject.NULL),
            )
        }
    }
}

internal fun testNodeJson(): JSONObject = JSONObject()
    .put("node_id", "node-test-1")
    .put("display_name", "Test node")
    .put("public_url", "https://node.example.test")
    .put(
        "location",
        JSONObject()
            .put("country_code", "TL")
            .put("country_name", "Testland")
            .put("city", "Frankfurt")
            .put("latitude", 50.1109)
            .put("longitude", 8.6821),
    )
    .put("health", "healthy")
    .put("active_sessions", 12)
    .put("session_capacity", 120)
    .put("capacity_percent", 10.0)
    .put("cpu_load_percent", 18.5)
    .put("memory_used_bytes", 2_147_483_648)
    .put("memory_total_bytes", 8_589_934_592)
    .put("uptime_seconds", 86_400)
    .put("observed_at_epoch_seconds", 4_000_000_000)
