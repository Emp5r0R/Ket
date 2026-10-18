package com.ket.android.connection

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BypassEntriesTest {
    @Test
    fun `entries follow the same rules as the desktop app and tunnel service`() {
        val cases = JSONObject(
            javaClass.classLoader!!.getResource("bypass-entries.json")!!.readText(),
        ).getJSONArray("cases")
        for (index in 0 until cases.length()) {
            val case = cases.getJSONObject(index)
            val input = case.getString("input")
            val result = when (val checked = BypassEntries.check(input)) {
                is BypassCheck.Accepted -> "entry:${checked.prefix}"
                is BypassCheck.Refused -> "error:${checked.refusal.code}"
            }
            val expected = if (case.has("entry")) "entry:${case.getString("entry")}" else "error:${case.getString("error")}"
            assertEquals(input, expected, result)
        }
    }

    @Test
    fun `saved lists drop entries that no longer pass and keep each once`() {
        val valid = BypassEntries.valid(listOf("10.0.0.0/8", "nonsense", "10.0.0.9/8", "2001:db8::/32"))
        assertEquals(listOf("10.0.0.0/8", "2001:db8::/32"), valid.map(BypassPrefix::toString))
    }

    @Test
    fun `adding stores the checked form, refuses with a reason, and stops at the limit`() {
        assertEquals(listOf("203.0.113.0/24"), addBypassEntry(emptyList(), " 203.0.113.9/24 ").getOrThrow())
        assertEquals(listOf("203.0.113.0/24"), addBypassEntry(listOf("203.0.113.0/24"), "203.0.113.0/24").getOrThrow())
        val refused = addBypassEntry(emptyList(), "0.0.0.0/0").exceptionOrNull()?.message.orEmpty()
        assertTrue(refused, refused.contains("too large"))
        val full = List(BypassEntries.MAX_ENTRIES) { "10.0.$it.0/24" }
        assertTrue(addBypassEntry(full, "192.168.1.0/24").isFailure)
    }
}
