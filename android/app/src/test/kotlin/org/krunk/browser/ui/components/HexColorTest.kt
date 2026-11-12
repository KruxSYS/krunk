package org.krunk.browser.ui.components

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HexColorTest {
    @Test
    fun coloursAreReadInEveryUsualSpellingAndWrittenInOne() {
        assertEquals("#336699", HexColor.parse("#336699"))
        assertEquals("#336699", HexColor.parse(" 336699 "))
        assertEquals("#aabbcc", HexColor.parse("#AABBCC"))
        assertEquals("#aabbcc", HexColor.parse("#abc"))
        assertEquals("#ffffff", HexColor.parse("FFF"))
    }

    @Test
    fun anythingElseIsNoColour() {
        val none =
            listOf(null, "", "#", "#12", "#1234", "#12345", "#1234567", "red", "#gggggg", "rgb(1,2,3)", "#33 6699")
        for (bad in none) {
            assertNull(bad, HexColor.parse(bad))
            assertNull(bad, HexColor.argb(bad))
        }
    }

    @Test
    fun theNumberForDrawingIsOpaque() {
        assertEquals(0xFF336699.toInt(), HexColor.argb("#336699"))
        assertEquals(0xFF000000.toInt(), HexColor.argb("#000"))
        assertEquals(0xFFFFFFFF.toInt(), HexColor.argb("ffffff"))
    }

    @Test
    fun thePagesSuggestionsAreOfferedOrAPlainPalette() {
        assertEquals(HexColor.PALETTE, HexColor.swatches(emptyList(), null))
        assertEquals(HexColor.PALETTE, HexColor.swatches(listOf("nonsense"), "also nonsense"))
        assertEquals(
            listOf("#ff0000", "#00ff00"),
            HexColor.swatches(listOf("#F00", "junk", "#00ff00", "#ff0000"), null),
        )
        // The field's own colour is always there to go back to.
        assertEquals(listOf("#123456", "#ff0000"), HexColor.swatches(listOf("#ff0000"), "#123456"))
        assertEquals(listOf("#ff0000"), HexColor.swatches(listOf("#ff0000"), "#F00"))
        assertEquals("#123456", HexColor.swatches(emptyList(), "#123456").first())
        // Never more than fit on a phone.
        val many = (0 until 100).map { "#0000%02x".format(it) }
        assertEquals(HexColor.MAX_SWATCHES, HexColor.swatches(many, null).size)
        assertEquals(HexColor.MAX_SWATCHES, HexColor.swatches(many, "#ffffff").size)
        assertTrue(HexColor.PALETTE.all { HexColor.parse(it) == it })
        assertEquals(HexColor.PALETTE.size, HexColor.PALETTE.distinct().size)
    }
}
