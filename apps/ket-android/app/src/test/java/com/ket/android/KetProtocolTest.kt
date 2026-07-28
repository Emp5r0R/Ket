package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class KetProtocolTest {
    @Test
    fun `wire catalog covers every implemented Android protocol exactly once`() {
        assertEquals(
            listOf(
                "stealth",
                "wire_guard",
                "hysteria2",
                "xor_scrambled",
                "vless_xtls_reality",
                "shadowsocks2022",
                "open_vpn_stunnel",
            ),
            KetProtocol.entries.map(KetProtocol::wireName),
        )
        assertEquals(KetProtocol.entries.size, KetProtocol.entries.map(KetProtocol::wireName).toSet().size)
        KetProtocol.entries.forEach { protocol ->
            assertSame(protocol, KetProtocol.fromWireName(protocol.wireName))
        }
    }

    @Test
    fun `protocol metadata remains complete and actionable`() {
        assertTrue(AUTO_PROTOCOL_INSTRUCTION.endsWith('.'))
        KetProtocol.entries.forEach { protocol ->
            assertTrue(protocol.displayName.isNotBlank())
            assertTrue(protocol.shortInstruction.endsWith('.'))
            assertTrue(protocol.bestFor.endsWith('.'))
            assertTrue(protocol.operation.endsWith('.'))
            assertTrue(protocol.steps.size >= 3)
            assertTrue(protocol.limitations.size >= 2)
            assertTrue(protocol.steps.all { it.endsWith('.') })
            assertTrue(protocol.limitations.all { it.endsWith('.') })
        }
    }

    @Test
    fun `wire parser rejects automatic unknown whitespace and case variants`() {
        listOf("", "auto", "ikev2", "stealth ", "STEALTH").forEach {
            assertNull(KetProtocol.fromWireName(it))
        }
        assertFalse(KetProtocol.entries.any { it.wireName == "auto" })
    }
}
