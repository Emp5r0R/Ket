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

    @Test
    fun `navigation destinations use stable stored names and reject unknown values`() {
        KetAppDestination.entries.forEach { destination ->
            assertEquals(destination, KetAppDestination.fromStored(destination.storedName))
        }
        assertEquals(KetAppDestination.Connection, KetAppDestination.fromStored(null))
        assertEquals(KetAppDestination.Connection, KetAppDestination.fromStored("unknown"))
    }
}
