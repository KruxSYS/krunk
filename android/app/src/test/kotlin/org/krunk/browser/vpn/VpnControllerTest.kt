package org.krunk.browser.vpn

import java.io.File
import java.nio.file.Files
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.vault.WrapFailure

class VpnControllerTest {
    private class FakeEngine : VpnEngine {
        var failStart: String? = null
        var started = 0
        var stopped = 0
        var changed = 0
        var lastToken: ByteArray? = null
        var listener: ((VpnStatus) -> Unit)? = null

        override fun importProfile(fileName: String, bytes: ByteArray) =
            VpnProfileInfo("wireguard", fileName.substringBefore('.'), "vpn.example.net:51820", false, listOf("w"))

        override fun start(
            fileName: String,
            bytes: ByteArray,
            username: String?,
            password: String?,
            token: ByteArray,
            onStatus: (VpnStatus) -> Unit,
        ): Int {
            failStart?.let { throw VpnEngineException.Start(it) }
            started++
            lastToken = token.copyOf()
            listener = onStatus
            return 40000 + started
        }

        override fun stop() {
            stopped++
        }

        override fun stopForExit() = Unit

        override fun status(): VpnStatus? = null

        override fun networkChanged() {
            changed++
        }

        override fun refusedRecently(host: String) = false
    }

    /** Identity "wrap": the controller's tests are not about the key store. */
    private object PlainWrap : VpnKeyWrap {
        override fun protect(data: ByteArray) = data.copyOf()

        @Throws(WrapFailure::class)
        override fun unprotect(data: ByteArray) = data.copyOf()
    }

    private val dir: File = Files.createTempDirectory("krunk-vpn-ctl").toFile()
    private val engine = FakeEngine()
    private val guard = mutableListOf<List<Any?>>()

    private fun controller() = VpnController(
        engine = engine,
        store = VpnStore(dir, PlainWrap),
        settingsFile = VpnSettingsFile(dir),
        io = Dispatchers.Unconfined,
        guard = { on, pending, port, pass, scope, lan -> guard += listOf(on, pending, port, pass, scope, lan) },
        clock = { 1L },
    )

    @After
    fun cleanUp() {
        dir.deleteRecursively()
        VpnRoute.state = VpnRouteState()
    }

    private fun status(state: VpnState) = VpnStatus(state, null, 2, 0, 0, "10.8.0.2", "vpn.example.net:51820")

    @Test
    fun aPickIsSavedAtOnceAlsoWhileOff() = runBlocking {
        val c = controller()
        c.load()
        val a = c.save(c.importFile("a.conf", "[Interface]".toByteArray()) as ImportResult.Ok, null).getOrThrow()
        val b = c.save(c.importFile("b.conf", "[Interface]".toByteArray()) as ImportResult.Ok, null).getOrThrow()
        assertEquals(a.id, c.state.value.settings.selectedProfileId)
        // Off: saved, nothing started.
        c.select(b.id).getOrThrow()
        assertEquals(b.id, c.state.value.settings.selectedProfileId)
        assertEquals(0, engine.started)
        // A fresh controller reads the choice back from the settings file.
        val again = controller()
        again.load()
        assertEquals(b.id, again.state.value.settings.selectedProfileId)
        // Unknown or malformed ids change nothing.
        c.select("nope").getOrThrow()
        c.select("../x").getOrThrow()
        assertEquals(b.id, c.state.value.settings.selectedProfileId)
        // On: a new pick reconnects with that profile.
        c.connect()
        assertEquals(1, engine.started)
        c.select(a.id).getOrThrow()
        assertEquals(a.id, c.state.value.settings.selectedProfileId)
        assertEquals(2, engine.started)
    }

    @Test
    fun connectStatusDisconnect() = runBlocking {
        val c = controller()
        c.load()
        assertEquals(VpnPhase.OFF, c.state.value.phase)
        val import = c.importFile("home.conf", "[Interface]".toByteArray()) as ImportResult.Ok
        assertEquals(listOf("w"), import.warnings)
        val p = c.save(import, null).getOrThrow()
        assertEquals(p.id, c.state.value.settings.selectedProfileId)

        c.connect()
        assertEquals(VpnPhase.CONNECTING, c.state.value.phase)
        assertEquals(40001, c.proxyPort())
        val hex = c.token()!!
        assertEquals(64, hex.length)
        assertEquals(VpnController.hex(engine.lastToken!!), hex)
        assertEquals(listOf(true, false, 40001, hex, "all", false), guard.last())
        // Not connected yet: Krunk's own requests wait (refused).
        assertEquals(Route.BLOCKED, VpnRoute.decide("example.com", false))

        engine.listener!!(status(VpnState.CONNECTED))
        assertEquals(VpnPhase.CONNECTED, c.state.value.phase)
        assertEquals(Route.PROXY, VpnRoute.decide("example.com", false))
        assertEquals("10.8.0.2", c.state.value.status?.tunnelIp)

        c.setScope(VpnScope.PRIVATE)
        assertEquals(listOf(true, false, 40001, hex, "private", false), guard.last())
        assertEquals(Route.DIRECT, VpnRoute.decide("example.com", false))
        assertEquals(Route.PROXY, VpnRoute.decide("example.com", true))

        // Reconnecting on resume: the runtime is told to try now; failed: a new start, new token.
        engine.listener!!(status(VpnState.RECONNECTING))
        c.onForeground()
        assertEquals(1, engine.changed)
        engine.listener!!(status(VpnState.FAILED))
        c.onForeground()
        assertEquals(2, engine.started)
        assertNotEquals(hex, c.token())

        c.disconnect()
        assertEquals(VpnPhase.OFF, c.state.value.phase)
        assertNull(c.proxyPort())
        assertNull(c.token())
        assertEquals(listOf(false, false, null, null, "private", false), guard.last())
        assertEquals(Route.DIRECT, VpnRoute.decide("example.com", true))
    }

    @Test
    fun aFailedStartStaysClosed() = runBlocking {
        val c = controller()
        c.load()
        c.save(c.importFile("home.conf", "x".toByteArray()) as ImportResult.Ok, null)
        engine.failStart = "no route"
        c.connect()
        assertEquals(VpnPhase.FAILED, c.state.value.phase)
        assertEquals("no route", c.state.value.status?.error)
        assertNull(c.proxyPort())
        // Pending: the guard holds the engine's requests, Krunk's own are refused.
        assertEquals(listOf(true, true, null, null, "all", false), guard.last())
        assertEquals(Route.BLOCKED, VpnRoute.decide("example.com", false))
        assertEquals(Route.DIRECT, VpnRoute.decide("localhost", false))
    }

    @Test
    fun atLaunchOnlyWithConnectAtStart() = runBlocking {
        val first = controller()
        first.load()
        first.save(first.importFile("home.conf", "x".toByteArray()) as ImportResult.Ok, null)
        first.connect()
        first.disconnect()
        first.setConnectAtStart(false)
        first.connect()

        // Next process: on, but no "connect at start": the start is direct and the VPN off.
        assertNull(controller().startAtLaunch())
        assertFalse(VpnSettingsFile(dir).read().enabled)

        val third = controller()
        third.load()
        third.setConnectAtStart(true)
        third.connect()
        val before = engine.started
        val port = controller().startAtLaunch()
        assertEquals(40000 + before + 1, port)
        assertTrue(VpnSettingsFile(dir).read().enabled)
    }
}
