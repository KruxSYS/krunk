package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Test
import org.krunk.browser.engine.BackAction

class BackDecisionTest {
    private val idle = BackInput(
        findActive = false,
        editing = false,
        fullScreen = false,
        backAction = BackAction.NONE,
        backOpenerAlive = false,
        fromApp = false,
        exitArmedAtMs = null,
        nowMs = 10_000,
    )

    /** Everything that can take Back at once: each earlier branch wins over the later ones. */
    private val all = idle.copy(
        findActive = true,
        editing = true,
        fullScreen = true,
        backAction = BackAction.HISTORY,
        backOpenerAlive = true,
        fromApp = true,
        exitArmedAtMs = 9_000,
    )

    @Test
    fun theOrderIsFindEditingFullScreenPageOpenerAppExit() {
        assertEquals(BackDecision.CloseFind, decideBack(all))
        assertEquals(BackDecision.StopEditing, decideBack(all.copy(findActive = false)))
        assertEquals(BackDecision.LeaveFullScreen, decideBack(all.copy(findActive = false, editing = false)))
        val noOverlay = all.copy(findActive = false, editing = false, fullScreen = false)
        assertEquals(BackDecision.PageBack, decideBack(noOverlay))
        val noHistory = noOverlay.copy(backAction = BackAction.NONE)
        assertEquals(BackDecision.CloseToOpener, decideBack(noHistory))
        assertEquals(BackDecision.CloseAndBackground, decideBack(noHistory.copy(backOpenerAlive = false)))
        assertEquals(BackDecision.Exit, decideBack(noHistory.copy(backOpenerAlive = false, fromApp = false)))
    }

    @Test
    fun theBlockedPanelIsLeftLikePageHistory() {
        assertEquals(BackDecision.PageBack, decideBack(idle.copy(backAction = BackAction.LEAVE_BLOCKED)))
        assertEquals(BackDecision.PageBack, decideBack(idle.copy(backAction = BackAction.HISTORY)))
    }

    @Test
    fun aClosedOpenerFallsThroughToTheApp() {
        assertEquals(BackDecision.CloseAndBackground, decideBack(idle.copy(fromApp = true)))
        assertEquals(BackDecision.ArmExit, decideBack(idle.copy(backOpenerAlive = false)))
    }

    @Test
    fun theFirstBackArmsTheExit() {
        assertEquals(BackDecision.ArmExit, decideBack(idle))
    }

    @Test
    fun theSecondBackExitsWithinTwoSeconds() {
        val armed = idle.copy(exitArmedAtMs = 10_000)
        assertEquals(BackDecision.Exit, decideBack(armed.copy(nowMs = 10_000)))
        assertEquals(BackDecision.Exit, decideBack(armed.copy(nowMs = 10_000 + EXIT_WINDOW_MS - 1)))
        assertEquals(BackDecision.Exit, decideBack(armed.copy(nowMs = 10_000 + EXIT_WINDOW_MS)))
        assertEquals(BackDecision.ArmExit, decideBack(armed.copy(nowMs = 10_000 + EXIT_WINDOW_MS + 1)))
        // A clock that went backwards (another boot's time) does not count as armed.
        assertEquals(BackDecision.ArmExit, decideBack(armed.copy(nowMs = 9_999)))
    }

    @Test
    fun anArmedExitDoesNotSkipEarlierSteps() {
        val armed = idle.copy(exitArmedAtMs = 9_500)
        assertEquals(BackDecision.PageBack, decideBack(armed.copy(backAction = BackAction.HISTORY)))
        assertEquals(BackDecision.CloseToOpener, decideBack(armed.copy(backOpenerAlive = true)))
        assertEquals(BackDecision.CloseFind, decideBack(armed.copy(findActive = true)))
    }
}
