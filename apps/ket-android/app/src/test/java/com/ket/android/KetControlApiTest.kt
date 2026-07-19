package com.ket.android

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
}
