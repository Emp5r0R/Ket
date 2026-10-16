package com.ket.android.splittunnel

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SplitTunnelPolicyTest {
    private val own = "com.ket.android"
    private val installed = setOf("org.mozilla.firefox", "com.whatsapp", "org.telegram.messenger", own)

    private class RecordingRules : VpnAppRules {
        val allowed = mutableListOf<String>()
        val disallowed = mutableListOf<String>()
        override fun allow(packageName: String) { allowed += packageName }
        override fun disallow(packageName: String) { disallowed += packageName }
    }

    private fun apply(policy: SplitTunnelPolicy): Pair<AppliedSplitTunnel, RecordingRules> {
        val rules = RecordingRules()
        return SplitTunnelRules.apply(policy, own, installed::contains, rules) to rules
    }

    @Test
    fun allAppsAddsNoRules() {
        val (applied, rules) = apply(SplitTunnelPolicy(SplitTunnelMode.AllApps, setOf("com.whatsapp")))
        assertEquals(AppliedSplitTunnel(SplitTunnelMode.AllApps, 0), applied)
        assertTrue(rules.allowed.isEmpty() && rules.disallowed.isEmpty())
    }

    @Test
    fun onlyChosenAppsAlwaysIncludesKet() {
        val (applied, rules) = apply(
            SplitTunnelPolicy(SplitTunnelMode.OnlySelected, setOf("org.mozilla.firefox", "com.whatsapp")),
        )
        assertEquals(SplitTunnelMode.OnlySelected, applied.mode)
        assertEquals(2, applied.appCount)
        assertEquals(listOf(own, "com.whatsapp", "org.mozilla.firefox"), rules.allowed)
        assertTrue(rules.disallowed.isEmpty())
    }

    @Test
    fun exceptChosenAppsNeverExcludesKet() {
        val (applied, rules) = apply(
            SplitTunnelPolicy(SplitTunnelMode.ExceptSelected, setOf(own, "org.telegram.messenger")),
        )
        assertEquals(AppliedSplitTunnel(SplitTunnelMode.ExceptSelected, 1), applied)
        assertEquals(listOf("org.telegram.messenger"), rules.disallowed)
        assertTrue(rules.allowed.isEmpty())
    }

    @Test
    fun allowAndDenyListsAreNeverMixed() {
        for (mode in SplitTunnelMode.entries) {
            val (_, rules) = apply(SplitTunnelPolicy(mode, installed))
            assertFalse("$mode mixed rules", rules.allowed.isNotEmpty() && rules.disallowed.isNotEmpty())
        }
    }

    @Test
    fun uninstalledAndInvalidAppsAreSkipped() {
        val (applied, rules) = apply(
            SplitTunnelPolicy(
                SplitTunnelMode.ExceptSelected,
                setOf("com.removed.app", "not a package", "com.whatsapp", "nodots"),
            ),
        )
        assertEquals(1, applied.appCount)
        assertEquals(listOf("com.whatsapp"), rules.disallowed)
    }

    @Test
    fun nothingLeftToChooseFallsBackToEveryApp() {
        for (mode in listOf(SplitTunnelMode.OnlySelected, SplitTunnelMode.ExceptSelected)) {
            for (packages in listOf(emptySet(), setOf(own), setOf("com.removed.app"))) {
                val (applied, rules) = apply(SplitTunnelPolicy(mode, packages))
                assertEquals(SplitTunnelMode.AllApps, applied.mode)
                assertEquals("no chosen app is installed", applied.fallbackReason)
                assertTrue(rules.allowed.isEmpty() && rules.disallowed.isEmpty())
            }
        }
    }

    @Test
    fun appliedChoicesReportNoFallback() {
        val (applied, _) = apply(SplitTunnelPolicy(SplitTunnelMode.OnlySelected, setOf("com.whatsapp")))
        assertNull(applied.fallbackReason)
    }

    @Test
    fun storedModesRoundTripAndUnknownMeansAllApps() {
        for (mode in SplitTunnelMode.entries) {
            assertEquals(mode, SplitTunnelMode.fromWireName(mode.wireName))
        }
        assertEquals(SplitTunnelMode.AllApps, SplitTunnelMode.fromWireName(null))
        assertEquals(SplitTunnelMode.AllApps, SplitTunnelMode.fromWireName("bogus"))
    }

    @Test
    fun packageNamesAreValidated() {
        assertTrue(SplitTunnelPolicy.isPackageName("org.mozilla.firefox"))
        assertTrue(SplitTunnelPolicy.isPackageName("com.example_app.v2"))
        for (invalid in listOf("", "firefox", ".com.app", "com..app", "com.app.", "1com.app", "com.app;rm", "com.${"a".repeat(260)}")) {
            assertFalse(invalid, SplitTunnelPolicy.isPackageName(invalid))
        }
        assertEquals(
            setOf("com.whatsapp"),
            SplitTunnelPolicy(SplitTunnelMode.OnlySelected, setOf("com.whatsapp", "bad")).normalized().packages,
        )
    }

    @Test
    fun choicesAreSearchedByLabelOrPackage() {
        val apps = listOf(
            InstalledApp("org.mozilla.firefox", "Firefox"),
            InstalledApp("com.whatsapp", "WhatsApp"),
            InstalledApp("org.telegram.messenger", "Telegram"),
        )
        assertEquals(listOf("Firefox"), SplitTunnelChoices.search(apps, "fire").map { it.label })
        assertEquals(listOf("Telegram"), SplitTunnelChoices.search(apps, "telegram.mess").map { it.label })
        assertEquals(3, SplitTunnelChoices.search(apps, "  ").size)
    }

    @Test
    fun togglingAChoiceAddsOrRemovesIt() {
        val policy = SplitTunnelPolicy(SplitTunnelMode.OnlySelected, setOf("com.whatsapp"))
        assertEquals(setOf("com.whatsapp", "org.mozilla.firefox"), SplitTunnelChoices.toggle(policy, "org.mozilla.firefox").packages)
        assertEquals(emptySet<String>(), SplitTunnelChoices.toggle(policy, "com.whatsapp").packages)
    }

    @Test
    fun summariesExplainWhatBypassesKet() {
        assertEquals("Every app uses Ket.", SplitTunnelChoices.summary(SplitTunnelPolicy.AllApps))
        assertEquals(
            "Only 2 chosen apps use Ket. Other apps connect directly.",
            SplitTunnelChoices.summary(SplitTunnelPolicy(SplitTunnelMode.OnlySelected, setOf("a.b", "c.d"))),
        )
        assertEquals(
            "1 chosen app connects directly, without Ket.",
            SplitTunnelChoices.summary(SplitTunnelPolicy(SplitTunnelMode.ExceptSelected, setOf("a.b"))),
        )
        assertEquals(
            "Choose at least one app. Until then, every app uses Ket.",
            SplitTunnelChoices.summary(SplitTunnelPolicy(SplitTunnelMode.OnlySelected)),
        )
    }
}
