package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Test

class KetThemeModeTest {
    @Test
    fun `stored appearance values are strict and default to system`() {
        assertEquals(KetThemeMode.System, KetThemeMode.fromStored(null))
        assertEquals(KetThemeMode.System, KetThemeMode.fromStored("unsupported"))
        assertEquals(KetThemeMode.Light, KetThemeMode.fromStored("light"))
        assertEquals(KetThemeMode.Dark, KetThemeMode.fromStored("dark"))
    }
}
