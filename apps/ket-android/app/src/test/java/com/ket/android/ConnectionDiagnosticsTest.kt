package com.ket.android

import java.time.Instant
import org.json.JSONObject
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

        assertTrue(report.contains("app=android 0.2.0"))
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

    @Test
    fun `report names the phone, network kind, and server without identifying them`() {
        val report = AndroidConnectionDiagnostics.report(
            version = "1.2.0",
            snapshot = TunnelSnapshot(serverVersion = "1.2.0"),
            environment = DiagnosticEnvironment(
                system = "Android 14 (API 34)",
                network = "cellular",
                control = DiagnosticEnvironment.controlMode(null),
                automaticSkips = listOf("open_vpn_stunnel", "wire_guard"),
            ),
        )
        assertTrue(report.contains("format=2"))
        assertTrue(report.contains("system=Android_14__API_34_"))
        assertTrue(report.contains("network=cellular"))
        assertTrue(report.contains("server=none"))
        assertTrue(report.contains("server_version=1.2.0"))
        assertTrue(report.contains("control=plain"))
        assertTrue(report.contains("automatic_skips=open_vpn_stunnel,wire_guard"))
        assertTrue(report.contains("attempts:\nnone"))
    }

    @Test
    fun `attempts list each transport tried, why it stopped, and the one that connected`() {
        AndroidConnectionDiagnostics.begin(null)
        fun start(transport: String, protocol: KetProtocol) = AndroidConnectionDiagnostics.record(
            stage = "transport_start",
            outcome = DiagnosticOutcome.Started,
            phase = TunnelPhase.Connecting,
            protocol = protocol,
            transport = transport,
        )
        start("reality-primary", KetProtocol.Reality)
        AndroidConnectionDiagnostics.record(
            stage = "transport_start",
            outcome = DiagnosticOutcome.Failed,
            phase = TunnelPhase.Connecting,
            protocol = KetProtocol.Reality,
            transport = "reality-primary",
            code = "transport_start_failed",
            message = "dial 203.0.113.5:443 timed out",
        )
        AndroidConnectionDiagnostics.record(
            stage = "transport_resolution",
            outcome = DiagnosticOutcome.Failed,
            phase = TunnelPhase.Connecting,
            protocol = KetProtocol.Hysteria2,
            transport = "hy2-primary",
            code = "endpoint_resolution_failed",
        )
        start("https-stealth-primary", KetProtocol.Stealth)
        AndroidConnectionDiagnostics.record(
            stage = "transport_start",
            outcome = DiagnosticOutcome.Succeeded,
            phase = TunnelPhase.Connected,
            protocol = KetProtocol.Stealth,
            transport = "https-stealth-primary",
        )

        val attempts = AndroidConnectionDiagnostics
            .report("1.2.0", TunnelSnapshot(handshakeLatencyMs = 812))
            .lines()
            .filter { it.startsWith("attempt=") }
        assertEquals(3, attempts.size)
        assertTrue(attempts[0].startsWith("attempt=1 protocol=vless_xtls_reality transport=reality-primary stage=connect outcome=failed elapsed_ms="))
        assertTrue(attempts[0].endsWith("code=transport_start_failed message=dial [address] timed out"))
        assertTrue(attempts[1].startsWith("attempt=2 protocol=hysteria2 transport=hy2-primary stage=probe outcome=failed"))
        assertTrue(attempts[2].startsWith("attempt=3 protocol=stealth transport=https-stealth-primary outcome=connected elapsed_ms="))
        assertTrue(attempts[2].endsWith("handshake_ms=812"))
    }

    @Test
    fun `messages are redacted exactly like the desktop app`() {
        val vectors = JSONObject(
            javaClass.classLoader!!.getResource("diagnostic-redaction.json")!!.readText(),
        ).getJSONArray("cases")
        for (index in 0 until vectors.length()) {
            val case = vectors.getJSONObject(index)
            assertEquals(
                case.getString("name"),
                case.getString("output"),
                AndroidConnectionDiagnostics.sanitizeMessage(case.getString("input")),
            )
        }
    }
}
