package com.ket.android.share

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ShareControllerTest {
    private val wifi = ShareNetwork("wlan0", "192.168.1.40", ShareNetworkKind.WiFi)

    private class FakeEngine : ShareEngine {
        var started = mutableListOf<String>()
        var stopped = mutableListOf<Long>()
        val networks = mutableListOf<Pair<Long, Long>>()
        var refusal: String? = null
        var counts: ShareCounts? = ShareCounts(devices = 2, connections = 5, refusedSignIns = 1)

        override fun start(address: String): ShareStart {
            started += address
            refusal?.let { return ShareStart.Refused(it) }
            return ShareStart.Started(StartedShare(started.size.toLong(), 39740, "ket", "abcdefghjkmn"))
        }

        override fun setNetwork(handle: Long, network: Long) {
            networks += handle to network
        }

        override fun counts(handle: Long): ShareCounts? = counts

        override fun stop(handle: Long) {
            stopped += handle
        }
    }

    @Test
    fun `a share starts with the vpn network and follows it`() {
        val engine = FakeEngine()
        var current = listOf(wifi)
        val controller = ShareController(engine) { current }
        controller.vpnNetworkChanged(77)

        assertTrue(controller.start("192.168.1.40"))
        val state = controller.state()
        assertEquals(ActiveShare(wifi, 39740, "ket", "abcdefghjkmn"), state.active)
        assertEquals("http://ket:abcdefghjkmn@192.168.1.40:39740", state.active?.proxyUrl)
        assertTrue(state.tunnel)
        assertEquals(listOf(1L to 77L), engine.networks)

        controller.vpnNetworkChanged(0)
        assertFalse(controller.state().tunnel)
        assertEquals(1L to 0L, engine.networks.last())

        controller.tick()
        assertEquals(2, controller.state().devices)
        assertEquals(1L, controller.state().refusedSignIns)

        current = emptyList()
        controller.tick()
        assertNull(controller.state().active)
        assertEquals(ShareNotice.NetworkGone, controller.state().notice)
        assertEquals(listOf(1L), engine.stopped)
    }

    @Test
    fun `a network the phone left or a refused start says why`() {
        val engine = FakeEngine()
        val controller = ShareController(engine) { listOf(wifi) }
        assertFalse(controller.start("192.168.9.9"))
        assertEquals(ShareNotice.NetworkUnavailable, controller.state().notice)
        assertTrue(engine.started.isEmpty())

        engine.refusal = "port_unavailable"
        assertFalse(controller.start("192.168.1.40"))
        assertEquals(ShareNotice.PortUnavailable, controller.state().notice)
        assertFalse(controller.isSharing)
    }

    @Test
    fun `stopping closes the share once and starting again replaces it`() {
        val engine = FakeEngine()
        val controller = ShareController(engine) { listOf(wifi) }
        controller.start("192.168.1.40")
        controller.start("192.168.1.40")
        assertEquals(listOf(1L), engine.stopped)
        controller.stop(ShareNotice.KetDisconnected)
        controller.stop()
        assertEquals(listOf(1L, 2L), engine.stopped)
        assertEquals(ShareNotice.KetDisconnected, controller.state().notice)
    }

    @Test
    fun `a share the library no longer knows is reported as stopped`() {
        val engine = FakeEngine()
        val controller = ShareController(engine) { listOf(wifi) }
        controller.start("192.168.1.40")
        engine.counts = null
        controller.tick()
        assertFalse(controller.isSharing)
        assertEquals(ShareNotice.Failed, controller.state().notice)
    }

    @Test
    fun `native rows are read strictly and the password never prints`() {
        val started = ShareRows.start(arrayOf("ok", "3", "39740", "ket", "abcdefghjkmn"))
        assertEquals(ShareStart.Started(StartedShare(3, 39740, "ket", "abcdefghjkmn")), started)
        assertFalse(started.toString().contains("abcdefghjkmn"))
        assertFalse(ActiveShare(wifi, 1, "ket", "abcdefghjkmn").toString().contains("abcdefghjkmn"))
        assertEquals(ShareStart.Refused("port_unavailable"), ShareRows.start(arrayOf("error", "port_unavailable")))
        assertEquals(ShareStart.Refused("native_failure"), ShareRows.start(null))
        assertEquals(ShareStart.Refused("native_failure"), ShareRows.start(arrayOf("ok", "x", "1", "ket", "p")))
        assertEquals(ShareStart.Refused("native_failure"), ShareRows.start(arrayOf("ok", "1", "1", "ket")))
        assertEquals(ShareCounts(1, 2, 3), ShareRows.counts(longArrayOf(1, 2, 3)))
        assertNull(ShareRows.counts(longArrayOf(1, 2)))
        assertNull(ShareRows.counts(null))
    }
}
