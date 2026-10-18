package com.ket.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class NetworkTransportMemoryTest {
    private var stored: String? = null
    private var now = 1_000L
    private val memory = NetworkTransportMemory(read = { stored }, write = { stored = it }, clock = { now++ })

    private fun history(id: String) = JSONObject().put(id, JSONObject().put("worked", true))

    @Test
    fun `each network keeps its own history`() {
        memory.save("wifi\nwlan0\n192.168.1.1", history("hysteria"))
        memory.save("cellular\nrmnet0\n", history("reality"))
        assertEquals(history("hysteria").toString(), memory.load("wifi\nwlan0\n192.168.1.1").toString())
        assertEquals(history("reality").toString(), memory.load("cellular\nrmnet0\n").toString())
        assertNull(memory.load("wifi\nwlan0\n10.0.0.1"))
        assertNull(memory.load(null))
    }

    @Test
    fun `the stored data names no network and survives corruption`() {
        memory.save("wifi\nwlan0\n192.168.1.1", history("hysteria"))
        for (secret in listOf("wlan0", "192.168.1.1", "wifi")) {
            assertFalse("$secret was stored", stored!!.contains(secret))
        }
        stored = "not json"
        assertNull(memory.load("wifi\nwlan0\n192.168.1.1"))
        memory.save("wifi\nwlan0\n192.168.1.1", history("hysteria"))
        assertEquals(history("hysteria").toString(), memory.load("wifi\nwlan0\n192.168.1.1").toString())
    }

    @Test
    fun `only the most recently seen networks are kept`() {
        repeat(NetworkTransportMemory.MAX_NETWORKS + 3) { memory.save("network-$it", history("x")) }
        assertEquals(
            NetworkTransportMemory.MAX_NETWORKS,
            JSONObject(stored!!).getJSONObject("networks").length(),
        )
        assertNull("the oldest network was forgotten", memory.load("network-0"))
        assertEquals(history("x").toString(), memory.load("network-${NetworkTransportMemory.MAX_NETWORKS + 2}").toString())
    }
}
