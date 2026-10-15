package com.ket.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidUpdateAdviceTest {
    private fun update(status: String, url: String = "https://mirror.example.test/ket") = JSONObject()
        .put("status", status)
        .put("latest_version", "1.2.0")
        .put("summary", "Fixes\na leak")
        .put("download_url", url)

    @Test
    fun `the app names its platform and version`() {
        assertEquals("android/1.2.0", AndroidUpdateAdviceParser.clientHeaderValue("1.2.0"))
        assertTrue(AndroidUpdateAdviceParser.clientHeaderValue().startsWith("android/"))
    }

    @Test
    fun `advice is parsed with bounded control-free text`() {
        val advice = AndroidUpdateAdviceParser.parseAdvice(
            JSONObject().put("server_version", "1.2.0").put(
                "client",
                update("security").put("required_after_epoch_seconds", 1_800_000_000L),
            ),
        )!!
        assertEquals("1.2.0", advice.serverVersion)
        assertEquals(ClientUpdateStatus.Security, advice.client.status)
        assertEquals(1_800_000_000L, advice.client.requiredAfterEpochSeconds)
        assertEquals("Fixesa leak", advice.client.summary)
    }

    @Test
    fun `malformed or unsafe advice is ignored rather than failing the session`() {
        assertNull(AndroidUpdateAdviceParser.parseAdvice(null))
        assertNull(AndroidUpdateAdviceParser.parseAdvice(JSONObject().put("server_version", "1.2.0")))
        for (url in listOf("http://mirror.example.test/", "javascript:alert(1)", "https://user:pw@mirror.example.test/", "")) {
            assertNull(url, AndroidUpdateAdviceParser.parseClientUpdate(update("optional", url)))
        }
        assertNull(AndroidUpdateAdviceParser.parseClientUpdate(update("urgent")))
        assertNull(AndroidUpdateAdviceParser.parseClientUpdate(update("security").put("required_after_epoch_seconds", -5)))
    }

    @Test
    fun `a 426 refusal carries the update and a clear message`() {
        val body = JSONObject()
            .put("code", CLIENT_UPDATE_REQUIRED_CODE)
            .put("message", "update Ket")
            .put("update", update("required"))
            .toString()
        val update = AndroidUpdateAdviceParser.parseRefusal(426, body)!!
        assertEquals(ClientUpdateStatus.Required, update.status)
        assertNull(AndroidUpdateAdviceParser.parseRefusal(400, body))
        assertNull(AndroidUpdateAdviceParser.parseRefusal(426, "{}"))
        assertEquals(
            "This server needs Ket 1.2.0 or newer. Update the app to connect.",
            KetControlApi.failureMessage(426, body, "Enrollment failed"),
        )
        val error = KetControlException(426, "update", update)
        assertTrue(error.updateRequired)
        assertFalse(error.authorizationLost)
    }

    @Test
    fun `notices escalate and stop being dismissible as urgency rises`() {
        val parsed = { status: String -> AndroidUpdateAdviceParser.parseClientUpdate(update(status)) }
        assertNull(UpdateNotices.describe(null))
        assertNull(UpdateNotices.describe(parsed("current")))
        assertEquals(UpdateTone.Info, UpdateNotices.describe(parsed("optional"))!!.tone)
        assertTrue(UpdateNotices.describe(parsed("recommended"))!!.dismissible)
        val security = UpdateNotices.describe(parsed("security")) { "day" }!!
        assertEquals(UpdateTone.Warning, security.tone)
        assertFalse(security.dismissible)
        assertTrue(security.title.endsWith("soon"))
        val dated = UpdateNotices.describe(
            AndroidUpdateAdviceParser.parseClientUpdate(update("security").put("required_after_epoch_seconds", 9L)),
        ) { "1 Jan" }!!
        assertTrue(dated.title.endsWith("before 1 Jan"))
        val required = UpdateNotices.describe(parsed("required"))!!
        assertEquals(UpdateTone.Danger, required.tone)
        assertEquals("https://mirror.example.test/ket", required.downloadUrl)
    }
}

class AndroidUpdateAdviceEnrollmentTest {
    @Test
    fun `enrollment keeps the server advice through the launch spec and stored manifest`() {
        val manifest = JSONObject(testSessionManifest(testSessionToken('A')))
            .put(
                "updates",
                JSONObject().put("server_version", "1.2.0").put(
                    "client",
                    JSONObject()
                        .put("status", "recommended")
                        .put("latest_version", "1.2.0")
                        .put("download_url", "https://mirror.example.test/ket"),
                ),
            )
            .toString()
        val enrollment = KetControlApi.parseEnrollment(manifest)
        val spec = TunnelLaunchSpec.fromEnrollment("https://vpn.example.test", enrollment)
        assertEquals(ClientUpdateStatus.Recommended, spec.updates!!.client.status)
        assertEquals("1.2.0", spec.withSessionExpiry(1L).updates!!.serverVersion)
        // The stored manifest is reparsed on restart and still carries the advice.
        assertEquals("1.2.0", KetControlApi.parseEnrollment(enrollment.manifestJson).updates!!.serverVersion)
        // Servers from before the update lifecycle send none.
        assertNull(KetControlApi.parseEnrollment(testSessionManifest(testSessionToken('B'))).updates)
    }
}
