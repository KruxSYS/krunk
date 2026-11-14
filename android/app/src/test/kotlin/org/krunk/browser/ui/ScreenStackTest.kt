package org.krunk.browser.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test

class ScreenStackTest {
    private val root = listOf("browser")

    @Test
    fun pushPutsAScreenOnTopOnce() {
        val one = ScreenStack.push(root, "settings")
        assertEquals(listOf("browser", "settings"), one)
        // The same screen again: nothing changes.
        assertSame(one, ScreenStack.push(one, "settings"))
        val two = ScreenStack.push(one, "about")
        assertEquals(listOf("browser", "settings", "about"), two)
        // A screen that is already below moves to the top instead of appearing twice.
        assertEquals(listOf("browser", "about", "settings"), ScreenStack.push(two, "settings"))
        // The root is never moved off the bottom: asking for it shows it alone.
        assertEquals(root, ScreenStack.push(two, "browser"))
        assertSame(root, ScreenStack.push(root, "browser"))
    }

    @Test
    fun popStopsAtTheRoot() {
        val stack = listOf("browser", "settings", "about")
        assertEquals(listOf("browser", "settings"), ScreenStack.pop(stack))
        assertEquals(root, ScreenStack.pop(ScreenStack.pop(stack)))
        assertSame(root, ScreenStack.pop(root))
        assertEquals(root, ScreenStack.root(stack))
    }

    @Test
    fun theTopIsComposedAndTheRootUnderItWhenKept() {
        assertEquals(listOf("browser"), ScreenStack.composed(root, keepRoot = true))
        assertEquals(listOf("browser"), ScreenStack.composed(root, keepRoot = false))
        val stack = listOf("browser", "settings", "about")
        assertEquals(listOf("browser", "about"), ScreenStack.composed(stack, keepRoot = true))
        assertEquals(listOf("about"), ScreenStack.composed(stack, keepRoot = false))
    }

    @Test
    fun stateIsKeptWhileCoveredAndDroppedWhenTheScreenLeavesTheStack() {
        val keys = ScreenStateKeys()
        assertEquals(emptySet<String>(), keys.update(listOf("browser")))
        assertEquals(emptySet<String>(), keys.update(listOf("browser", "settings")))
        // Settings is covered by About: nothing is dropped.
        assertEquals(emptySet<String>(), keys.update(listOf("browser", "settings", "about")))
        // Back from About: About's state goes, Settings keeps its own.
        assertEquals(setOf("about"), keys.update(listOf("browser", "settings")))
        // Moved to the top from below: still in the stack, nothing dropped.
        keys.update(listOf("browser", "settings", "about"))
        assertEquals(emptySet<String>(), keys.update(listOf("browser", "about", "settings")))
        // "Show the browser" drops everything above it, never the browser's own.
        assertEquals(setOf("about", "settings"), keys.update(listOf("browser")))
        // Dropped once.
        assertEquals(emptySet<String>(), keys.update(listOf("browser")))
        // Opened again: a new entry, dropped again when it leaves.
        keys.update(listOf("browser", "settings"))
        assertEquals(setOf("settings"), keys.update(listOf("browser")))
    }

    @Test
    fun screensWithAnArgumentAreKeptApart() {
        val keys = ScreenStateKeys()
        val a = Screen.SiteDetail("https://a.example").stateKey
        val b = Screen.SiteDetail("https://b.example").stateKey
        keys.update(listOf(Screen.Browser.stateKey, Screen.SitePermissions.stateKey, a))
        assertEquals(setOf(a), keys.update(listOf(Screen.Browser.stateKey, Screen.SitePermissions.stateKey, b)))
        assertEquals("Browser", Screen.Browser.stateKey)
    }
}
