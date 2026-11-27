package org.krunk.browser.cast

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CastVideosTest {
    @Test
    fun onlyHttpAddressesAreCastable() {
        assertTrue(CastVideos.castableUrl("https://example.com/a.mp4"))
        assertTrue(CastVideos.castableUrl("http://10.0.0.2:8080/v.webm"))
        assertTrue(CastVideos.castableUrl("HTTPS://Example.com/x"))
        assertFalse(CastVideos.castableUrl(""))
        assertFalse(CastVideos.castableUrl("   "))
        assertFalse(CastVideos.castableUrl("blob:https://example.com/0b1c-22"))
        assertFalse(CastVideos.castableUrl("data:video/mp4;base64,AAAA"))
        assertFalse(CastVideos.castableUrl("file:///sdcard/a.mp4"))
        assertFalse(CastVideos.castableUrl("https://"))
        assertFalse(CastVideos.castableUrl("https://example.com/" + "a".repeat(5000)))
    }

    @Test
    fun mseVideosAreNotCastableEvenWithAnHttpLookingAddress() {
        assertFalse(CastVideos.castable(PageVideo(src = "https://example.com/a.mp4", mse = true)))
        assertTrue(CastVideos.castable(PageVideo(src = "https://example.com/a.mp4")))
    }

    @Test
    fun contentTypeFromExtensionOrDeclaredType() {
        assertEquals("video/mp4", CastVideos.contentType("https://e.com/v/clip.mp4?token=1#t=3"))
        assertEquals("video/webm", CastVideos.contentType("https://e.com/clip.WEBM"))
        assertEquals("application/x-mpegURL", CastVideos.contentType("https://e.com/live/index.m3u8"))
        assertEquals("application/dash+xml", CastVideos.contentType("https://e.com/manifest.mpd"))
        // Unknown extension or none: the receiver's most common format.
        assertEquals("video/mp4", CastVideos.contentType("https://e.com/watch"))
        assertEquals("video/mp4", CastVideos.contentType("https://e.com/v.php?f=a.webm"))
        // A declared media type wins, without its codecs.
        assertEquals("video/webm", CastVideos.contentType("https://e.com/x.mp4", "video/webm; codecs=\"vp9\""))
        // A declared type that is not a media type is ignored.
        assertEquals("video/mp4", CastVideos.contentType("https://e.com/x.mp4", "text/html"))
    }

    @Test
    fun decideNoVideoNotCastableAndCastable() {
        assertEquals(CastDecision.NoVideo, CastVideos.decide(emptyList(), "Page"))
        assertEquals(
            CastDecision.NotCastable,
            CastVideos.decide(listOf(PageVideo(src = "blob:https://e.com/1", mse = true)), "Page"),
        )
        assertEquals(CastDecision.NotCastable, CastVideos.decide(listOf(PageVideo(src = "")), "Page"))

        val d = CastVideos.decide(
            listOf(
                PageVideo(src = "https://e.com/ad.mp4", paused = true, area = 900_000),
                PageVideo(
                    src = "https://e.com/film.webm",
                    poster = "https://e.com/p.jpg",
                    currentTime = 12.5,
                    duration = 600.0,
                    paused = false,
                    area = 100,
                ),
            ),
            "The page",
        ) as CastDecision.Castable
        // The playing video wins over a larger paused one; the title falls back to the page's.
        assertEquals("https://e.com/film.webm", d.media.url)
        assertEquals("video/webm", d.media.contentType)
        assertEquals("The page", d.media.title)
        assertEquals("https://e.com/p.jpg", d.media.poster)
        assertEquals(12_500L, d.media.startMs)
        assertFalse(d.media.live)
    }

    @Test
    fun aCastableVideoIsPreferredToABetterPlacedBlobVideo() {
        val d = CastVideos.decide(
            listOf(
                PageVideo(src = "blob:https://e.com/1", mse = true, paused = false, area = 1_000_000),
                PageVideo(src = "https://e.com/b.mp4", title = "B", poster = "data:image/png;base64,AA"),
            ),
            "Page",
        ) as CastDecision.Castable
        assertEquals("https://e.com/b.mp4", d.media.url)
        assertEquals("B", d.media.title)
        assertNull(d.media.poster)
    }

    @Test
    fun liveStreamsStartAtTheLiveEdge() {
        val d = CastVideos.decide(
            listOf(PageVideo(src = "https://e.com/live.m3u8", currentTime = 3000.0, duration = -1.0)),
            "Live",
        ) as CastDecision.Castable
        assertTrue(d.media.live)
        assertEquals(0L, d.media.startMs)
    }
}
