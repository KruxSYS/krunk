package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FaviconsTest {
    @Test
    fun ordinaryIconsAreDecodedAsTheyAreOrHalved() {
        assertEquals(1, Favicons.iconSampleSize(16, 16))
        assertEquals(1, Favicons.iconSampleSize(32, 32))
        assertEquals(1, Favicons.iconSampleSize(64, 64))
        assertEquals(1, Favicons.iconSampleSize(96, 96))
        assertEquals(2, Favicons.iconSampleSize(128, 128))
        assertEquals(2, Favicons.iconSampleSize(180, 180))
        assertEquals(4, Favicons.iconSampleSize(256, 256))
        assertEquals(8, Favicons.iconSampleSize(512, 512))
        assertEquals(32, Favicons.iconSampleSize(2048, 2048))
        // The shorter side decides, so a banner is not shrunk to nothing.
        assertEquals(1, Favicons.iconSampleSize(2048, 64))
    }

    @Test
    fun picturesThatAreNotIconsAreRefusedBeforeDecoding() {
        // A one-bit 8000 x 8000 PNG is a few kilobytes and decodes to 256 MB.
        assertNull(Favicons.iconSampleSize(8000, 8000))
        assertNull(Favicons.iconSampleSize(2049, 16))
        assertNull(Favicons.iconSampleSize(16, 100_000))
        // Not an image the decoder understands (SVG, HTML error page): no size.
        assertNull(Favicons.iconSampleSize(-1, -1))
        assertNull(Favicons.iconSampleSize(0, 32))
    }
}
