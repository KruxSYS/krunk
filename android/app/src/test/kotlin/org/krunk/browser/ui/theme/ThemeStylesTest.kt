package org.krunk.browser.ui.theme

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.ui.components.HexColor
import org.krunk.ffi.ThemeHighlight
import org.krunk.ffi.ThemeShape
import org.krunk.ffi.ToolbarFill
import org.krunk.ffi.accentSwatches
import org.krunk.ffi.defaultSettings
import org.krunk.ffi.resolveTheme
import org.krunk.ffi.themePresets
import org.krunk.ffi.themeWithAccent
import org.krunk.ffi.themeWithPreset

class ThemeStylesTest {
    @Test
    fun coloursBecomeOpaqueNumbers() {
        assertEquals(0xFF2563EB.toInt(), ThemeStyles.argb("#2563eb"))
        assertEquals(0xFFFFFFFF.toInt(), ThemeStyles.argb("#FFFFFF"))
        assertEquals(0xFF000000.toInt(), ThemeStyles.argb("not a colour"))
    }

    @Test
    fun aGradientRunsAlongTheToolbarToTheSideItsAnglePointsTo() {
        val stops = listOf("#ff0000", "#00ff00", "#0000ff")
        val forward = listOf(0xFFFF0000.toInt(), 0xFF00FF00.toInt(), 0xFF0000FF.toInt())
        // 90 runs to the right; slanted angles keep their side; straight up or down has no side.
        for (angle in listOf(90, 45, 120, 135, 1, 179, 0, 180, 360)) {
            assertEquals(
                "$angle",
                forward,
                ThemeStyles.leftToRight(stops, angle),
            )
        }
        for (angle in listOf(
            270,
            225,
            315,
            181,
            359,
        )) {
            assertEquals("$angle", forward.reversed(), ThemeStyles.leftToRight(stops, angle))
        }
        assertEquals(listOf(0xFF123456.toInt()), ThemeStyles.leftToRight(listOf("#123456"), 270))
    }

    @Test
    fun aThemeWithoutAStyleKeepsTheAddressFieldAsItWas() {
        assertEquals(20, ThemeStyles.pillCornerDp(ThemeShape.ROUNDED))
        assertEquals(20, ThemeStyles.pillCornerDp(ThemeShape.PILL))
        assertTrue(ThemeStyles.pillCornerDp(ThemeShape.SQUARE) < 10)
        assertEquals(12, ThemeStyles.cardCornerDp(ThemeShape.ROUNDED))
        assertTrue(ThemeStyles.cardCornerDp(ThemeShape.SQUARE) < ThemeStyles.cardCornerDp(ThemeShape.ROUNDED))
        assertTrue(ThemeStyles.cardCornerDp(ThemeShape.PILL) > ThemeStyles.cardCornerDp(ThemeShape.ROUNDED))
    }

    @Test
    fun theNewTabPagesSearchBoxAndTilesFollowTheStylesCorners() {
        // "Rounded" is how they have always been drawn.
        assertEquals(28, ThemeStyles.fieldCornerDp(ThemeShape.ROUNDED, 56))
        assertEquals(14, ThemeStyles.tileCornerDp(ThemeShape.ROUNDED))
        assertEquals(28, ThemeStyles.fieldCornerDp(ThemeShape.PILL, 56))
        assertEquals(28, ThemeStyles.tileCornerDp(ThemeShape.PILL))
        assertEquals(ThemeStyles.pillCornerDp(ThemeShape.SQUARE), ThemeStyles.fieldCornerDp(ThemeShape.SQUARE, 56))
        assertTrue(ThemeStyles.tileCornerDp(ThemeShape.SQUARE) < 8)
        for (shape in ThemeShape.entries) {
            assertEquals(
                ThemeStyles.pillCornerDp(shape),
                ThemeStyles.fieldCornerDp(shape, 40),
            )
        }
    }

    @Test
    fun theActiveCardIsMarkedInEveryStyleAndNoOtherCardIs() {
        assertEquals(CardMark.FILL, ThemeStyles.cardMark(ThemeHighlight.FILL, active = true))
        assertEquals(CardMark.UNDERLINE, ThemeStyles.cardMark(ThemeHighlight.UNDERLINE, active = true))
        assertEquals(CardMark.OUTLINE, ThemeStyles.cardMark(ThemeHighlight.OUTLINE, active = true))
        assertEquals(CardMark.TITLE, ThemeStyles.cardMark(ThemeHighlight.NONE, active = true))
        for (h in ThemeHighlight.entries) assertEquals(CardMark.NONE, ThemeStyles.cardMark(h, active = false))
    }

    // ----- through the shared core, as the app uses it -----

    @Test
    fun everyStyleResolvesToColoursTheAppCanDraw() {
        for (dark in listOf(false, true)) {
            val presets = themePresets("", dark)
            assertEquals(12, presets.size)
            assertEquals(presets.size, presets.map { it.id }.distinct().size)
            for (p in presets) {
                val t = p.theme
                assertTrue(p.id, p.label.isNotBlank() && t.dark == dark && !t.private)
                val colours = listOf(
                    t.background, t.surfaceRaised, t.surfaceHover, t.surfaceLowest, t.border, t.text, t.textMuted,
                    t.error, t.onError, t.accent, t.onAccent, t.primary, t.onPrimary, t.primaryContainer,
                    t.onPrimaryContainer, t.toolbarText, t.pill, t.pillText, t.secure,
                ) + t.toolbarColors + t.statusStrip
                for (c in colours) assertEquals("${p.id}: $c", c, HexColor.parse(c))
                assertTrue(p.id, t.toolbarColors.isNotEmpty())
                assertEquals(p.id, t.toolbarFill == ToolbarFill.GRADIENT, t.toolbarColors.size > 1)
            }
        }
    }

    @Test
    fun choosingAStyleAndAnAccentChangesOnlyThat() {
        val start = defaultSettings().appearance
        val sunset = themeWithPreset(start, "sunset")
        assertEquals("sunset", sunset.preset)
        assertEquals(ToolbarFill.GRADIENT, sunset.toolbarFill)
        assertEquals(start, themeWithPreset(start, "no such style"))
        val green = themeWithAccent(sunset, "#16A34A")
        assertEquals("#16a34a", green.accent)
        assertEquals(sunset, green.copy(accent = sunset.accent))
        assertEquals(green, themeWithAccent(green, "not a colour"))
        // Krunk's own blue is stored as "no accent".
        assertEquals("", themeWithAccent(green, accentSwatches().first().color).accent)
        assertTrue(accentSwatches().all { HexColor.parse(it.color) == it.color && it.name.isNotBlank() })
    }

    @Test
    fun aPrivateTabLooksTheSameInEveryStyleAndUnlikeAllOfThem() {
        val start = defaultSettings().appearance
        val private = themePresets("", true).map {
            resolveTheme(themeWithPreset(start, it.id), dark = true, private = true)
        }
        // Only shape and highlight follow the style.
        assertEquals(
            1,
            private.map {
                it.copy(shape = ThemeShape.ROUNDED, highlight = ThemeHighlight.FILL)
            }.distinct().size,
        )
        for (dark in listOf(false, true)) {
            for (p in themePresets("", dark)) {
                assertNotEquals(p.id, p.theme.background, private.first().background)
                assertNotEquals(p.id, p.theme.toolbarColors, private.first().toolbarColors)
                assertNotEquals(p.id, p.theme.primary, private.first().primary)
            }
        }
    }
}
