package com.ket.android

import java.time.Instant
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionDiagnosticsTest {
    @After
    fun reset() {
        AndroidConnectionDiagnostics.resetForTest()
    }

    @Test
    fun `report includes lifecycle context and redacts credentials`() {
        AndroidConnectionDiagnostics.begin(KetProtocol.Stealth)
        AndroidConnectionDiagnostics.record(
            stage = "transport_start",
            outcome = DiagnosticOutcome.Failed,
            phase = TunnelPhase.Connecting,
            protocol = KetProtocol.Stealth,
            transport = "https-stealth-primary",
            code = "transport_start_failed",
            message = "Bearer secret-value access_code=ABCDEFGHIJKLMNOPQRSTUVWXYZ123456",
        )

        val report = AndroidConnectionDiagnostics.report(
            version = "0.2.0",
            snapshot = TunnelSnapshot(phase = TunnelPhase.Failed, transportName = "HTTPS Stealth"),
            generatedAt = Instant.parse("2026-07-26T10:00:00Z"),
        )

        assertTrue(report.contains("version=0.2.0"))
        assertTrue(report.contains("stage=transport_start outcome=failed"))
        assertTrue(report.contains("code=transport_start_failed"))
        assertTrue(report.contains("[REDACTED]"))
        assertFalse(report.contains("secret-value"))
        assertFalse(report.contains("ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"))
    }

    @Test
    fun `event history is bounded`() {
        AndroidConnectionDiagnostics.begin(null)
        repeat(120) { index ->
            AndroidConnectionDiagnostics.record(
                stage = "candidate_$index",
                outcome = DiagnosticOutcome.Observed,
                phase = TunnelPhase.Connecting,
            )
        }
        val report = AndroidConnectionDiagnostics.report("0.2.0", TunnelSnapshot())
        assertEquals(96, report.lineSequence().count { it.startsWith("event=") })
        assertFalse(report.contains("stage=candidate_0 "))
        assertTrue(report.contains("stage=candidate_119 "))
    }
}
