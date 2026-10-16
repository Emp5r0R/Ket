package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class KetCopyTest {
    @Test
    fun `invites are found inside any shared or pasted text`() {
        val invite = "ket:invite:AbC_12-xyz"
        assertEquals(invite, extractInvite(invite))
        assertEquals(invite, extractInvite("Here is your access:\n$invite\nSee you!"))
        assertEquals(invite, extractInvite("($invite)."))
        assertNull(extractInvite("https://example.com"))
        assertNull(extractInvite(""))
        assertNull(extractInvite(null))
    }

    @Test
    fun `setup time reads as time, not as a ping`() {
        assertEquals("--", formatSetupTime(null))
        assertEquals("850 ms", formatSetupTime(850))
        assertEquals("11.6 s", formatSetupTime(11_642))
    }

    @Test
    fun `sizes and durations stay short`() {
        assertEquals("512 B", formatBytes(512))
        assertEquals("1.5 MB", formatBytes(1_572_864))
        assertEquals("2d 3h", formatDuration(2 * 86_400L + 3 * 3_600))
        assertEquals("27m", formatDuration(27 * 60L))
        assertEquals("--", formatDuration(null))
    }
}
