package org.krunk.browser.engine.permissions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.DocumentChange
import org.krunk.browser.engine.permissions.PermissionPolicy.Decision
import org.krunk.ffi.FfiException
import org.krunk.ffi.SitePermission
import org.krunk.ffi.siteOfHost
import org.krunk.ffi.webOrigin

/** The plain parts behind `SitePermissions` (review of 0.4.0: P-1 ... P-18, S-3). */
class SitePermissionsLogicTest {
    private val fine = AndroidPermissions.FINE_LOCATION
    private val coarse = AndroidPermissions.COARSE_LOCATION

    // ----- P-1: the engine's own location step -----

    @Test
    fun theEnginesFineOnlyStepIsAnsweredFromApproximate() {
        // The engine asks for precise only; approximate is enough on Android 12+.
        assertTrue(AndroidPermissions.satisfied(listOf(fine), setOf(coarse), 31))
        assertTrue(AndroidPermissions.satisfied(listOf(fine), setOf(coarse), 35))
        assertFalse(AndroidPermissions.satisfied(listOf(fine), setOf(coarse), 30))
        assertTrue(AndroidPermissions.satisfied(listOf(fine), setOf(fine), 26))
        assertFalse(AndroidPermissions.satisfied(listOf(fine), emptySet(), 35))
        // What the gate checks for a location request: both, whichever was asked.
        assertEquals(listOf(fine, coarse), AndroidPermissions.toCheck(listOf(fine)))
        assertEquals(listOf(AndroidPermissions.CAMERA), AndroidPermissions.toCheck(listOf(AndroidPermissions.CAMERA)))
        // Krunk's own request and the engine's step agree, so the row and the engine agree.
        for (sdk in listOf(30, 35)) {
            for (granted in listOf(emptySet(), setOf(coarse), setOf(fine), setOf(fine, coarse))) {
                assertEquals(
                    "$sdk $granted",
                    AndroidPermissions.satisfied(listOf(fine, coarse), granted, sdk),
                    AndroidPermissions.satisfied(listOf(fine), granted, sdk),
                )
            }
        }
    }

    @Test
    fun theGateAnswersTheEnginesStepWithoutADialog() {
        val granted = mutableSetOf(coarse)
        val gate = AndroidPermissionGate({ it in granted }, 35) { 0L }
        var launched = 0
        gate.attach("activity", { launched++ }, { false })
        var answer: AndroidPermissionGate.Outcome? = null
        gate.request(listOf(fine)) { answer = it }
        assertEquals(0, launched)
        assertTrue(AndroidPermissions.satisfied(listOf(fine), answer!!.granted, 35))
        assertTrue(gate.satisfied(listOf(fine)))
    }

    // ----- P-9 / P-14: the gate and the activity -----

    private class GateRig(var clock: Long = 0L) {
        val granted = HashSet<String>()
        val launched = ArrayList<List<String>>()
        var rationale = false
        val gate = AndroidPermissionGate({ it in granted }, 35) { clock }

        fun attach(owner: Any) = gate.attach(owner, { launched += it.toList() }, { rationale })
    }

    @Test
    fun aRecreatedActivityFinishesTheRequestInFlight() {
        val rig = GateRig()
        rig.attach("old")
        val answers = ArrayList<Boolean>()
        rig.gate.request(listOf(AndroidPermissions.CAMERA)) { answers += AndroidPermissions.CAMERA in it.granted }
        assertEquals(1, rig.launched.size)
        // Destroyed while the dialog is open, not finishing: nothing is answered early.
        rig.gate.detach("old", finishing = false)
        assertTrue(answers.isEmpty())
        // A request meanwhile waits (no activity to launch from, no early deny either).
        rig.gate.request(listOf(AndroidPermissions.RECORD_AUDIO)) { answers += true }
        // The new activity: the in-flight dialog is not launched again; its result arrives there.
        rig.attach("new")
        assertEquals(1, rig.launched.size)
        rig.granted += AndroidPermissions.CAMERA
        rig.gate.onResult()
        assertEquals(listOf(true), answers)
        // Then the waiting one gets its dialog.
        assertEquals(2, rig.launched.size)
    }

    @Test
    fun aLostResultEndsWhenTheNewActivityIsInFront() {
        val rig = GateRig()
        rig.attach("old")
        var answered = 0
        rig.gate.request(listOf(AndroidPermissions.CAMERA)) { answered++ }
        rig.gate.detach("old", finishing = false)
        rig.attach("new")
        // Its own request is not touched by resuming...
        rig.gate.resumed("old")
        assertEquals(0, answered)
        // ...the old activity's one is, when the new one is in front without a result.
        rig.gate.resumed("new")
        assertEquals(1, answered)
        rig.gate.onResult()
        assertEquals(1, answered)
    }

    @Test
    fun aFinishingActivityAnswersWithTheCurrentState() {
        val rig = GateRig()
        rig.attach("a")
        var answer: AndroidPermissionGate.Outcome? = null
        rig.gate.request(listOf(AndroidPermissions.CAMERA)) { answer = it }
        rig.gate.detach("a", finishing = true)
        assertTrue(answer!!.granted.isEmpty())
        assertFalse(rig.gate.hasActivity)
    }

    @Test
    fun settingsAreOfferedOnlyWhenAndroidWillNotAskAgain() {
        val rig = GateRig(clock = 1_000L)
        rig.attach("a")
        val outcomes = ArrayList<AndroidPermissionGate.Outcome>()
        // Dismissed after a while, no rationale: Android would ask again.
        rig.gate.request(listOf(AndroidPermissions.CAMERA)) { outcomes += it }
        rig.clock += 3_000
        rig.gate.onResult()
        assertFalse(outcomes.last().noDialog)
        // Refused a second time: no dialog any more.
        rig.gate.request(listOf(AndroidPermissions.CAMERA)) { outcomes += it }
        rig.clock += 3_000
        rig.gate.onResult()
        assertTrue(outcomes.last().noDialog)
        // An answer that came back at once (no dialog was on screen).
        rig.gate.request(listOf(AndroidPermissions.RECORD_AUDIO)) { outcomes += it }
        rig.clock += 20
        rig.gate.onResult()
        assertTrue(outcomes.last().noDialog)
        // With a rationale Android still asks.
        rig.rationale = true
        rig.gate.request(listOf(AndroidPermissions.RECORD_AUDIO)) { outcomes += it }
        rig.gate.onResult()
        assertFalse(outcomes.last().noDialog)
    }

    // ----- P-2 / P-3 / P-4: who is named and keyed -----

    private val news = "https://news.example"

    @Test
    fun aFramesRequestIsThePagesAnswerAndNamesTheFrame() {
        val s = PermissionSubject.content(
            PermissionKind.GEOLOCATION,
            "https://news.example/article",
            "https://ads.example^firstPartyDomain=news.example",
            "https://news.example/article",
            ::webOrigin,
        )!!
        assertEquals(news, s.origin)
        assertNull(s.asker)
        assertEquals("https://ads.example", s.frame)
        // The page itself, or a frame of the same origin: no frame line.
        val own = PermissionSubject.content(PermissionKind.NOTIFICATIONS, "$news/", "$news", "$news/", ::webOrigin)!!
        assertEquals(PermissionSubject(news), own)
        // A frame without a web address (sandboxed): named as such, the page is the site.
        val sandboxed = PermissionSubject.content(
            PermissionKind.GEOLOCATION,
            "$news/",
            "moz-nullprincipal:{1234}",
            "$news/",
            ::webOrigin,
        )!!
        assertEquals("", sandboxed.frame)
        assertEquals(news, sandboxed.origin)
    }

    @Test
    fun storageAccessNamesBothSites() {
        val s = PermissionSubject.content(
            PermissionKind.STORAGE_ACCESS,
            "https://video.example/embed",
            "https://video.example",
            "$news/a",
            ::webOrigin,
        )!!
        assertEquals(news, s.origin)
        assertEquals("https://video.example", s.asker)
        assertNull(s.frame)
    }

    @Test
    fun cameraFromAFrameIsKeyedLikeTheOtherKinds() {
        // The Deny shortcut before Android's dialog uses the page's origin too.
        val s = PermissionSubject.media("https://meet.example/frame", "$news/a", ::webOrigin)!!
        assertEquals(news, s.origin)
        assertEquals("https://meet.example", s.frame)
        // A frame without an address of its own (P-4): the page is named, the frame as such.
        val blank = PermissionSubject.media("about:srcdoc", "$news/a", ::webOrigin)!!
        assertEquals(news, blank.origin)
        assertEquals("", blank.frame)
        assertEquals(PermissionSubject(news), PermissionSubject.media("$news/a", "$news/a", ::webOrigin))
        assertEquals(PermissionSubject(news), PermissionSubject.media("", "$news/a", ::webOrigin))
    }

    @Test
    fun noSiteToNameIsRefused() {
        assertNull(PermissionSubject.media("about:blank", "about:blank", ::webOrigin))
        assertNull(PermissionSubject.media("https://a.test/", "file:///sdcard/x.html", ::webOrigin))
        assertNull(
            PermissionSubject.content(PermissionKind.GEOLOCATION, "file:///sdcard/x.html", null, "", ::webOrigin),
        )
        val noPage = PermissionSubject.content(
            PermissionKind.STORAGE_ACCESS,
            "https://v.test/",
            null,
            "about:blank",
            ::webOrigin,
        )
        assertNull(noPage)
    }

    // ----- P-5 / P-18: how a site is shown -----

    @Test
    fun theRegistrableDomainIsWeightedAndLongHostsAreBounded() {
        val plain = SiteLabel.of("https://www.shop.example.co.uk", ::siteOfHost)
        assertEquals(SiteLabel("", "www.shop.", "example.co.uk", ""), plain)
        assertEquals("www.shop.example.co.uk", plain.text)
        // Plain http keeps its scheme; a port is kept.
        assertEquals("http://bank.example", SiteLabel.of("http://bank.example", ::siteOfHost).text)
        assertEquals("a.test:8443", SiteLabel.of("https://a.test:8443", ::siteOfHost).text)
        assertEquals("10.0.2.2:18080", SiteLabel.of("http://10.0.2.2:18080", ::siteOfHost).text.removePrefix("http://"))
        assertEquals("[::1]:8080", SiteLabel.of("https://[::1]:8080", ::siteOfHost).text)
        // A long host: the start is cut, the registrable domain stays whole.
        val long = "accounts.google.com." + "x".repeat(200) + ".evil.example"
        val label = SiteLabel.of("https://$long", ::siteOfHost)
        assertEquals("evil.example", label.registrable)
        assertTrue(label.sub.startsWith("…"))
        assertEquals(SiteLabel.MAX_HOST, (label.sub + label.registrable).length)
        assertTrue(label.text.endsWith(".evil.example"))
        // A registrable domain longer than the bound is cut at its start too.
        val huge = SiteLabel.of("https://" + "y".repeat(100) + ".example", ::siteOfHost)
        assertEquals(SiteLabel.MAX_HOST, huge.registrable.length)
        assertTrue(huge.registrable.endsWith(".example"))
    }

    // ----- P-7: a real document change -----

    @Test
    fun onlyACommittedDocumentEndsThePage() {
        val doc = DocumentChange(::webOrigin)
        // The first page.
        doc.pageStart()
        assertTrue(doc.locationChanged("https://a.test/"))
        doc.pageStop()
        // A download, an HTTP 204, an external link: a load starts and stops, the location stays.
        doc.pageStart()
        doc.pageStop()
        // pushState / a fragment: the same document.
        assertFalse(doc.locationChanged("https://a.test/#x"))
        assertFalse(doc.locationChanged("https://a.test/other"))
        // A real navigation commits during its load.
        doc.pageStart()
        assertTrue(doc.locationChanged("https://a.test/next"))
        doc.pageStop()
        // A change of origin is always another document.
        assertTrue(doc.locationChanged("https://b.test/"))
    }

    // ----- P-8 / S-3: one-time answers -----

    private class MapStore : PermissionStore {
        val rows = HashMap<Pair<String, String>, Boolean>()

        override fun get(origin: String, kind: String) = rows[origin to kind]

        override fun set(origin: String, kind: String, allow: Boolean) {
            rows[origin to kind] = allow
        }

        override fun remove(origin: String, kind: String) {
            rows.remove(origin to kind)
        }
    }

    @Test
    fun aOneTimeLocationAllowStaysWithItsTabAndPage() {
        val store = MapStore()
        val policy = PermissionPolicy(store)
        val geo = listOf(PermissionKind.GEOLOCATION)
        policy.answered(1u, news, false, geo, allow = true, remember = false)
        assertTrue(store.rows.isEmpty())
        // The same page asks again: no prompt (the engine's entry was forgotten at once).
        assertEquals(Decision.Allow, policy.decide(1u, news, false, geo))
        // Another tab of the same site, or another site in this tab: asked.
        assertEquals(Decision.Ask(geo), policy.decide(2u, news, false, geo))
        assertEquals(Decision.Ask(geo), policy.decide(1u, "https://other.example", false, geo))
        // Another document: asked again.
        policy.newDocument(1u)
        assertEquals(Decision.Ask(geo), policy.decide(1u, news, false, geo))
        // Notifications keep the engine's entry for the page instead: nothing kept here.
        policy.answered(1u, news, false, listOf(PermissionKind.NOTIFICATIONS), allow = true, remember = false)
        assertEquals(
            Decision.Ask(listOf(PermissionKind.NOTIFICATIONS)),
            policy.decide(1u, news, false, listOf(PermissionKind.NOTIFICATIONS)),
        )
    }

    @Test
    fun whichEngineEntriesAreForgottenAtOnce() {
        for (kind in PermissionKind.entries) {
            val atOnceDeny = kind.forgetEngineEntryAtOnce(allow = false)
            val atOnceAllow = kind.forgetEngineEntryAtOnce(allow = true)
            when (kind) {
                PermissionKind.STORAGE_ACCESS, PermissionKind.AUTOPLAY_AUDIBLE, PermissionKind.AUTOPLAY_INAUDIBLE -> {
                    assertFalse(kind.name, atOnceDeny || atOnceAllow)
                }

                PermissionKind.GEOLOCATION, PermissionKind.DRM -> assertTrue(kind.name, atOnceDeny && atOnceAllow)

                else -> assertTrue(kind.name, atOnceDeny && !atOnceAllow)
            }
        }
    }

    @Test
    fun resettingASiteDropsThePrivateAnswersToo() {
        val policy = PermissionPolicy(MapStore())
        val cam = listOf(PermissionKind.CAMERA)
        policy.answered(5u, news, true, cam, allow = true, remember = true)
        policy.answered(5u, "https://keep.example", true, cam, allow = false, remember = true)
        assertEquals(setOf(news, "https://keep.example"), policy.privateOrigins())
        policy.forgetPrivate(news)
        assertEquals(Decision.Ask(cam), policy.decide(6u, news, true, cam))
        assertEquals(Decision.Deny(stored = true), policy.decide(6u, "https://keep.example", true, cam))
        assertEquals(1, policy.privateAnswerCount)
    }

    // ----- P-17: the table in memory -----

    private class CountingRows : PermissionRows {
        val rows = LinkedHashMap<Pair<String, String>, Boolean>()
        var reads = 0
        var failing = false

        override fun all(): List<SitePermission> {
            reads++
            if (failing) throw FfiException.Database("busy")
            return rows.map { (k, v) -> SitePermission(k.first, k.second, v) }
        }

        override fun set(origin: String, kind: String, allow: Boolean) {
            if (failing) throw FfiException.Database("busy")
            rows[origin to kind] = allow
        }

        override fun remove(origin: String, kind: String) {
            rows.remove(origin to kind)
        }

        override fun clearOrigin(origin: String) {
            rows.keys.removeAll { it.first == origin }
        }

        override fun clear() = rows.clear()
    }

    @Test
    fun requestsAreAnsweredFromMemory() {
        val rows = CountingRows()
        rows.rows["https://a.test" to "camera"] = true
        val table = TablePermissionStore(rows, ::webOrigin)
        repeat(50) { assertEquals(true, table.get("https://a.test", "camera")) }
        assertNull(table.get("https://a.test/page", "microphone"))
        assertEquals(1, rows.reads)
        // Changes made through the table are seen without reading again.
        table.set("https://b.test/x?y", "geolocation", false)
        assertEquals(false, table.get("https://b.test", "geolocation"))
        table.remove("https://a.test", "camera")
        assertNull(table.get("https://a.test", "camera"))
        table.set("https://c.test", "camera", true)
        table.clearOrigin("https://c.test/")
        assertNull(table.get("https://c.test", "camera"))
        table.clear()
        assertNull(table.get("https://b.test", "geolocation"))
        assertEquals(1, rows.reads)
        // Invalidated: read once more.
        rows.rows["https://d.test" to "camera"] = false
        table.invalidate()
        assertEquals(false, table.get("https://d.test", "camera"))
        assertEquals(2, rows.reads)
    }

    @Test
    fun aFailingDatabaseIsAskedAgainLater() {
        val rows = CountingRows()
        rows.failing = true
        val table = TablePermissionStore(rows, ::webOrigin)
        assertNull(table.get("https://a.test", "camera"))
        table.set("https://a.test", "camera", true)
        rows.failing = false
        rows.rows["https://a.test" to "camera"] = true
        assertEquals(true, table.get("https://a.test", "camera"))
    }
}
