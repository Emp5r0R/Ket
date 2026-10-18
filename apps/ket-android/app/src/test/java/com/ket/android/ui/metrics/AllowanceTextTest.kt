package com.ket.android.ui.metrics

import com.ket.android.AllowancePeriod
import com.ket.android.DataAllowanceStatus
import org.junit.Assert.assertEquals
import org.junit.Test

class AllowanceTextTest {
    private val allowance = DataAllowanceStatus(
        bytes = 53_687_091_200L,
        period = AllowancePeriod.Month,
        usedBytes = 13_421_772_800L,
        resetsAtEpochSeconds = 1_793_491_200L,
    )

    @Test
    fun `an allowance in use says what it is and when it starts again`() {
        assertEquals(
            "50.0 GB a month, shared by every device using this invite. Starts again 1 Nov 2026, 00:00 UTC.",
            allowanceText(allowance),
        )
    }

    @Test
    fun `a used up allowance says when Ket can connect again`() {
        assertEquals(
            "Used up. Ket can connect again from 1 Nov 2026, 00:00 UTC.",
            allowanceText(allowance.copy(usedBytes = allowance.bytes)),
        )
    }
}
