package com.ket.android.ui.settings

import com.ket.android.AlwaysOnState
import org.junit.Assert.assertEquals
import org.junit.Test

class AlwaysOnTextTest {
    @Test
    fun `each always-on state reads plainly`() {
        assertEquals("Connect once to see whether it is on.", alwaysOnText(null))
        assertEquals(
            "On: Ket starts by itself, and apps have no internet without it.",
            alwaysOnText(AlwaysOnState(alwaysOn = true, lockdown = true)),
        )
        assertEquals(
            "Ket starts by itself, but apps can still reach the internet without it.",
            alwaysOnText(AlwaysOnState(alwaysOn = true, lockdown = false)),
        )
        assertEquals(
            "Off: apps use the phone's own connection whenever Ket is not connected.",
            alwaysOnText(AlwaysOnState(alwaysOn = false, lockdown = false)),
        )
    }
}
