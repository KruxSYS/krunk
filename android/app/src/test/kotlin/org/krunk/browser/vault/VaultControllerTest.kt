package org.krunk.browser.vault

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class VaultControllerTest {
    private val backend = FakeVaultBackend()
    private var keyPresent = false
    private var keysMade = 0
    private var keysDeleted = 0
    private var failKey: WrapFailure? = null
    private var now = 1_000_000L
    private var lockMinutes = VaultController.DEFAULT_LOCK_MINUTES
    private var graceSeconds = VaultController.DEFAULT_BACKGROUND_GRACE_SECONDS

    private fun TestScope.controller(timerScope: CoroutineScope? = null) = VaultController(
        backend = backend,
        keyPresent = { keyPresent },
        prepareForNewVault = {
            failKey?.let { throw it }
            keysMade++
            keyPresent = true
        },
        deleteKey = {
            keysDeleted++
            keyPresent = false
        },
        clock = { now },
        dispatcher = UnconfinedTestDispatcher(testScheduler),
        lockMinutes = { lockMinutes },
        backgroundGraceSeconds = { graceSeconds },
        timerScope = timerScope,
    )

    /** Every lock event from now on. */
    private fun TestScope.events(vault: VaultController): List<LockReason> {
        val seen = ArrayList<LockReason>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vault.lockEvents.collect { seen += it } }
        return seen
    }

    private inline fun <reified T : VaultFailure> failureOf(block: () -> Unit): T = try {
        block()
        throw AssertionError("expected ${T::class.simpleName}")
    } catch (e: VaultFailure) {
        e as? T ?: throw AssertionError("expected ${T::class.simpleName}, got ${e.javaClass.simpleName}")
    }

    private fun minutes(n: Int) = n * 60_000L

    /** A scope for the controller's own jobs that runs only when the test advances time. */
    private fun TestScope.jobs() =
        CoroutineScope(backgroundScope.coroutineContext + StandardTestDispatcher(testScheduler))

    @Test
    fun createMakesTheKeyThenTheVault() = runTest {
        val vault = controller()
        assertEquals(VaultState.NONE, vault.state.value)
        vault.create(PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        assertEquals(1, keysMade)
        assertTrue(keyPresent)
        assertEquals(VaultFileState.UNLOCKED, backend.state())
    }

    @Test
    fun aFailedCreateLeavesNoKey() = runTest {
        val vault = controller()
        failureOf<VaultFailure.WeakPassword> { vault.create("short") }
        assertEquals(VaultState.NONE, vault.state.value)
        assertFalse(keyPresent)

        backend.failCreate = VaultFailure.Io("disk full")
        failureOf<VaultFailure.Io> { vault.create(PASSWORD) }
        assertFalse(keyPresent)
        assertEquals(VaultState.NONE, vault.state.value)
    }

    @Test
    fun aKeyStoreThatCannotMakeTheKeyFailsCreate() = runTest {
        val vault = controller()
        failKey = WrapFailure(WrapFailureKind.FAILED, "ProviderException")
        failureOf<VaultFailure.Io> { vault.create(PASSWORD) }
        assertEquals(VaultFileState.NONE, backend.state())
        assertFalse(keyPresent)
    }

    @Test
    fun createNeverReplacesTheKeyOfAnExistingVault() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        failureOf<VaultFailure.AlreadyExists> { vault.create("another password") }
        assertEquals(1, keysMade)
        assertEquals(0, keysDeleted)
        assertEquals(VaultState.LOCKED, vault.state.value)
    }

    @Test
    fun unlockAndWrongPassword() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        assertEquals(VaultState.LOCKED, vault.state.value)

        failureOf<VaultFailure.WrongPassword> { vault.unlock("not the password") }
        assertEquals(VaultState.LOCKED, vault.state.value)

        now += 2_000
        vault.unlock(PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
    }

    @Test
    fun rateLimitIsSurfaced() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        assertEquals(0, vault.retryWaitMs())

        failureOf<VaultFailure.WrongPassword> { vault.unlock("wrong 1") }
        assertEquals(1_000, vault.retryWaitMs())
        // Too early: refused without a look at the password, even the right one.
        now += 400
        assertEquals(600, failureOf<VaultFailure.RateLimited> { vault.unlock(PASSWORD) }.waitMs)
        assertEquals(VaultState.LOCKED, vault.state.value)

        now += 600
        failureOf<VaultFailure.WrongPassword> { vault.unlock("wrong 2") }
        assertEquals(2_000, vault.retryWaitMs())
        now += 2_000
        vault.unlock(PASSWORD)
        assertEquals(0, vault.retryWaitMs())
    }

    @Test
    fun unlockWithoutAVault() = runTest {
        val vault = controller()
        failureOf<VaultFailure.NoVault> { vault.unlock(PASSWORD) }
        assertEquals(VaultState.NONE, vault.state.value)
    }

    @Test
    fun locksAfterFifteenMinutesWithoutUse() = runTest {
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)

        now += minutes(15) - 1
        vault.tick()
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        now += 1
        vault.tick()
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(VaultFileState.LOCKED, backend.state())
        assertEquals(listOf(LockReason.TIMER), events)
    }

    @Test
    fun useResetsTheTimer() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        now += minutes(14)
        assertEquals(0, vault.use { it.search("").size })
        now += minutes(14)
        vault.tick()
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        now += minutes(1)
        vault.tick()
        assertEquals(VaultState.LOCKED, vault.state.value)
    }

    @Test
    fun aChangedTimerSettingApplies() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        lockMinutes = 1
        now += minutes(1)
        vault.tick()
        assertEquals(VaultState.LOCKED, vault.state.value)

        vault.unlock(PASSWORD)
        lockMinutes = 60
        now += minutes(59)
        vault.tick()
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        now += minutes(1)
        vault.tick()
        assertEquals(VaultState.LOCKED, vault.state.value)
    }

    @Test
    fun theTimerCoroutineTicksWhileUnlocked() = runTest {
        val vault = controller(jobs())
        vault.create(PASSWORD)
        runCurrent()
        lockMinutes = 1
        now += minutes(1)
        // Nothing happens between ticks; the next one locks.
        advanceTimeBy(VaultController.TICK_MS - 1)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        advanceTimeBy(2)
        assertEquals(VaultState.LOCKED, vault.state.value)

        // Unlocked again, the ticking starts again.
        vault.unlock(PASSWORD)
        runCurrent()
        now += minutes(1)
        advanceTimeBy(VaultController.TICK_MS + 1)
        assertEquals(VaultState.LOCKED, vault.state.value)
    }

    @Test
    fun withoutAGraceItLocksAtOnceInTheBackground() = runTest {
        graceSeconds = 0
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)
        vault.onBackground()
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(VaultFileState.LOCKED, backend.state())
        assertEquals(listOf(LockReason.BACKGROUND), events)

        // Back in front: still locked, and it opens again with the password.
        vault.onForeground()
        assertEquals(VaultState.LOCKED, vault.state.value)
        vault.unlock(PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
    }

    @Test
    fun anUnlockThatEndsInTheBackgroundDoesNotStayOpen() = runTest {
        graceSeconds = 0
        val vault = controller()
        vault.create(PASSWORD)
        vault.onBackground()
        vault.unlock(PASSWORD)
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(VaultFileState.LOCKED, backend.state())
    }

    @Test
    fun theDefaultGraceIsOneMinute() = runTest {
        assertEquals(60, VaultController.DEFAULT_BACKGROUND_GRACE_SECONDS)
        val vault = controller()
        vault.create(PASSWORD)
        vault.onBackground()
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        now += 59_999
        vault.onForeground()
        assertEquals(VaultState.UNLOCKED, vault.state.value)
    }

    @Test
    fun backWithinTheGraceStaysUnlockedAndTheScheduledLockIsCancelled() = runTest {
        val vault = controller(jobs())
        val events = events(vault)
        vault.create(PASSWORD)
        runCurrent()

        vault.onBackground()
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        advanceTimeBy(30_000)
        now += 30_000
        vault.onForeground()
        assertEquals(VaultState.UNLOCKED, vault.state.value)

        // The lock that was scheduled for the end of the grace never comes.
        advanceTimeBy(120_000)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        assertTrue(events.isEmpty())
        assertEquals("pw", vault.use { it.reveal(it.saveLogin("https://example.com/", "alice", "pw", now)) })
    }

    @Test
    fun backAfterTheGraceIsLockedOnForegroundEvenIfNothingRanMeanwhile() = runTest {
        // No scope: nothing can run in the background, as in a frozen process.
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)
        vault.onBackground()
        assertEquals(VaultState.UNLOCKED, vault.state.value)

        now += 60_000
        vault.onForeground()
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(VaultFileState.LOCKED, backend.state())
        assertEquals(listOf(LockReason.BACKGROUND), events)
        failureOf<VaultFailure.Locked> { vault.use { it.search("") } }
    }

    @Test
    fun theScheduledLockComesWhileStillInTheBackground() = runTest {
        graceSeconds = 30
        val vault = controller(jobs())
        val events = events(vault)
        vault.create(PASSWORD)
        runCurrent()

        vault.onBackground()
        advanceTimeBy(29_999)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        advanceTimeBy(2)
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(VaultFileState.LOCKED, backend.state())
        assertEquals(listOf(LockReason.BACKGROUND), events)

        // Coming back later finds it locked and does not lock (or report) again.
        now += 45_000
        vault.onForeground()
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(1, events.size)
    }

    @Test
    fun aScheduledLockFromAnEarlierLeaveDoesNotCutTheNextGraceShort() = runTest {
        val vault = controller(jobs())
        vault.create(PASSWORD)
        runCurrent()

        vault.onBackground()
        advanceTimeBy(50_000)
        now += 50_000
        vault.onForeground()
        vault.onBackground()
        // 60 s after the first leave, 10 s after the second.
        advanceTimeBy(10_001)
        now += 10_001
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        advanceTimeBy(50_000)
        assertEquals(VaultState.LOCKED, vault.state.value)
    }

    @Test
    fun theLockTimerRunsOutDuringTheGrace() = runTest {
        graceSeconds = 300
        lockMinutes = 1
        val vault = controller(jobs())
        val events = events(vault)
        vault.create(PASSWORD)
        runCurrent()

        vault.onBackground()
        now += minutes(1)
        // The periodic check, still running in the background:
        advanceTimeBy(VaultController.TICK_MS + 1)
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(listOf(LockReason.TIMER), events)
    }

    @Test
    fun theLockTimerCountsTheTimeInTheBackground() = runTest {
        graceSeconds = 300
        lockMinutes = 1
        // Nothing runs in the background; the check happens on return.
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)
        now += 40_000
        vault.onBackground()
        now += 20_000
        // Within the grace, but a minute without use: leaving and returning is not use.
        vault.onForeground()
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(listOf(LockReason.TIMER), events)
    }

    @Test
    fun anUnlockThatEndsInTheBackgroundWithinTheGraceStaysOpenUntilItIsOver() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        vault.onBackground()
        now += 10_000
        vault.unlock(PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        now += 50_000
        vault.onForeground()
        assertEquals(VaultState.LOCKED, vault.state.value)
    }

    @Test
    fun aClockSetBackWhileAwayLocks() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.onBackground()
        now -= 3_600_000
        vault.onForeground()
        assertEquals(VaultState.LOCKED, vault.state.value)
    }

    @Test
    fun foregroundChecksTheTimer() = runTest {
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)
        now += minutes(20)
        vault.onForeground()
        assertEquals(VaultState.LOCKED, vault.state.value)
        assertEquals(listOf(LockReason.TIMER), events)
    }

    @Test
    fun relockAndUnlockAgain() = runTest {
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)
        val id = vault.use { it.saveLogin("https://example.com/login", "alice", "pw-1", now) }
        repeat(3) {
            vault.lock()
            assertEquals(VaultState.LOCKED, vault.state.value)
            vault.unlock(PASSWORD)
            assertEquals("pw-1", vault.use { it.reveal(id) })
        }
        assertEquals(List(3) { LockReason.MANUAL }, events)
        // Locking a locked vault is not another lock.
        vault.lock()
        vault.lock()
        assertEquals(4, events.size)
    }

    @Test
    fun useWhileLockedThrowsLocked() = runTest {
        val vault = controller()
        failureOf<VaultFailure.Locked> { vault.use { it.search("") } }
        vault.create(PASSWORD)
        vault.lock()
        var ran = false
        failureOf<VaultFailure.Locked> { vault.use { ran = true } }
        assertFalse(ran)
    }

    @Test
    fun hintsWorkWhileLocked() = runTest {
        val vault = controller()
        assertFalse(vault.hasHint("https://example.com/"))
        vault.create(PASSWORD)
        vault.use { it.saveLogin("https://example.com/login", "alice", "pw-1", now) }
        vault.lock()
        assertTrue(vault.hasHint("https://example.com/other"))
        assertFalse(vault.hasHint("https://example.org/"))
    }

    @Test
    fun aVaultFileWithoutItsKeyIsKeyLostBeforeAnyPassword() = runTest {
        controller().apply {
            create(PASSWORD)
            lock()
        }
        keyPresent = false
        // A new process: the state is worked out from the file and the key.
        val vault = controller()
        assertEquals(VaultState.KEY_LOST, vault.state.value)
        // The password is not even tried, and no key is made.
        failureOf<VaultFailure.KeyLost> { vault.unlock(PASSWORD) }
        assertEquals(0, vault.retryWaitMs())
        assertEquals(1, keysMade)
        assertEquals(VaultState.KEY_LOST, vault.state.value)
    }

    @Test
    fun refreshSeesAKeyThatWentMissing() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        keyPresent = false
        vault.refresh()
        assertEquals(VaultState.KEY_LOST, vault.state.value)
    }

    @Test
    fun onlyAVaultWithALostKeyCanBeDeleted() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        failureOf<VaultFailure.NotAllowed> { vault.destroy() }
        vault.lock()
        failureOf<VaultFailure.NotAllowed> { vault.destroy() }
        assertEquals(VaultFileState.LOCKED, backend.state())
        assertTrue(keyPresent)
    }

    @Test
    fun tryAgainOnTheKeyLostScreenAsksTheKeyStoreAnew() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        backend.keyLost = true
        failureOf<VaultFailure.KeyLost> { vault.unlock(PASSWORD) }
        assertEquals(VaultState.KEY_LOST, vault.state.value)
        // Still gone: asking again does not unlock anything, the next unlock says so again.
        vault.recheckKey()
        assertEquals(VaultState.LOCKED, vault.state.value)
        failureOf<VaultFailure.KeyLost> { vault.unlock(PASSWORD) }
        assertEquals(VaultState.KEY_LOST, vault.state.value)
        // It was only out of reach: the vault is back.
        backend.keyLost = false
        vault.recheckKey()
        vault.unlock(PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
    }

    @Test
    fun whatTheEngineAsksByItselfDoesNotDelayTheLock() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        val touches = backend.touches
        vault.peek { it.matchesFor("https://bank.example/") }
        assertEquals("peek is not a use", touches, backend.touches)
        vault.use { it.matchesFor("https://bank.example/") }
        assertEquals(touches + 1, backend.touches)
        vault.lock()
        failureOf<VaultFailure.Locked> { vault.peek { it.matchesFor("https://bank.example/") } }
    }

    @Test
    fun aKeyStoreThatOnlyFailedLeavesTheVaultLockedNotLost() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        backend.keyUnavailable = true
        failureOf<VaultFailure.KeyUnavailable> { vault.unlock(PASSWORD) }
        // No "this vault cannot be opened, delete it": nothing is known about the key.
        assertEquals(VaultState.LOCKED, vault.state.value)
        vault.refresh()
        assertEquals(VaultState.LOCKED, vault.state.value)
        backend.keyUnavailable = false
        vault.unlock(PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
    }

    @Test
    fun anUnlockThatFindsTheKeyUnusableIsKeyLost() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        backend.keyLost = true
        failureOf<VaultFailure.KeyLost> { vault.unlock(PASSWORD) }
        assertEquals(VaultState.KEY_LOST, vault.state.value)
        // The alias is still there; the state must not flip back to "locked".
        vault.refresh()
        vault.onForeground()
        assertEquals(VaultState.KEY_LOST, vault.state.value)
    }

    @Test
    fun destroyIsTheWayOutOfKeyLost() = runTest {
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)
        vault.lock()
        backend.keyLost = true
        failureOf<VaultFailure.KeyLost> { vault.unlock(PASSWORD) }
        var bioReset: LockReason? = null
        vault.onBiometricReset = { bioReset = it }

        vault.destroy()
        assertEquals(VaultState.NONE, vault.state.value)
        assertEquals(VaultFileState.NONE, backend.state())
        assertFalse(keyPresent)
        assertEquals(LockReason.DESTROYED, bioReset)
        assertEquals(listOf(LockReason.MANUAL, LockReason.DESTROYED), events)

        // And a new vault can be made.
        backend.keyLost = false
        vault.create("a new password")
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        assertEquals(2, keysMade)
    }

    @Test
    fun changePasswordKeepsTheVaultOpenAndDropsTheBiometricBlob() = runTest {
        val vault = controller()
        val events = events(vault)
        vault.create(PASSWORD)
        backend.setBioBlob(byteArrayOf(1, 2, 3))
        var bioReset: LockReason? = null
        vault.onBiometricReset = { bioReset = it }

        failureOf<VaultFailure.WrongPassword> { vault.changePassword("not the old one", NEW_PASSWORD) }
        assertArrayEquals(byteArrayOf(1, 2, 3), backend.bioBlob())
        assertEquals(null, bioReset)

        vault.changePassword(PASSWORD, NEW_PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        assertEquals(null, backend.bioBlob())
        assertEquals(LockReason.PASSWORD_CHANGED, bioReset)
        assertTrue(events.isEmpty())

        vault.lock()
        failureOf<VaultFailure.WrongPassword> { vault.unlock(PASSWORD) }
        now += 2_000
        vault.unlock(NEW_PASSWORD)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
    }

    @Test
    fun changePasswordNeedsAnOpenVault() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        vault.lock()
        failureOf<VaultFailure.Locked> { vault.changePassword(PASSWORD, NEW_PASSWORD) }
    }

    @Test
    fun unlockWithKeyWipesTheKey() = runTest {
        val vault = controller()
        vault.create(PASSWORD)
        val key = vault.use { it.unlockKey() }
        vault.lock()

        val wrong = ByteArray(32) { 9 }
        failureOf<VaultFailure.WrongPassword> { vault.unlockWithKey(wrong) }
        assertArrayEquals(ByteArray(32), wrong)
        assertEquals(VaultState.LOCKED, vault.state.value)

        vault.unlockWithKey(key)
        assertEquals(VaultState.UNLOCKED, vault.state.value)
        assertArrayEquals(ByteArray(32), key)
    }

    private companion object {
        const val PASSWORD = "correct horse"
        const val NEW_PASSWORD = "battery staple"
    }
}
