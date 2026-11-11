package org.krunk.browser.engine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ExternalLinkTest {
    private fun tapped(uri: String) = ExternalLinks.classify(uri, userGesture = true)

    @Test
    fun mailAndPhoneLinksTheUserTappedAreHandedOver() {
        assertEquals(
            HandOff.Open(ExternalLink(ExternalKind.MAIL, "mailto:info@example.com", "info@example.com")),
            tapped("mailto:info@example.com"),
        )
        assertEquals(
            HandOff.Open(
                ExternalLink(ExternalKind.MAIL, "mailto:a%40example.com?subject=Hi%20there&body=x", "a@example.com"),
            ),
            tapped("mailto:a%40example.com?subject=Hi%20there&body=x"),
        )
        // Only fields, no address: still mail.
        assertEquals(
            HandOff.Open(ExternalLink(ExternalKind.MAIL, "mailto:?subject=Hello", "mailto:")),
            tapped("MAILTO:?subject=Hello"),
        )
        assertEquals(
            HandOff.Open(ExternalLink(ExternalKind.PHONE, "tel:+49301234567", "+49 30 1234567")),
            tapped("tel:+49 30 1234567"),
        )
        assertEquals(
            HandOff.Open(ExternalLink(ExternalKind.PHONE, "tel:(030)123-45", "(030)123-45")),
            tapped("tel:(030)123-45"),
        )
        // A service code keeps its # encoded for the dialer.
        assertEquals(
            HandOff.Open(ExternalLink(ExternalKind.PHONE, "tel:*100%23", "*100#")),
            tapped("tel:*100%23"),
        )
    }

    @Test
    fun withoutATapThePageIsNotObeyedAtOnce() {
        val link = ExternalLink(ExternalKind.PHONE, "tel:+15551234", "+15551234")
        // A script or a meta refresh.
        assertEquals(HandOff.Ask(link), ExternalLinks.classify("tel:+15551234", userGesture = false))
        // A server redirect that ends on tel: is not the user's tap either.
        assertEquals(HandOff.Ask(link), ExternalLinks.classify("tel:+15551234", userGesture = true, redirect = true))
        assertTrue(ExternalLinks.classify("mailto:x@example.com", userGesture = false) is HandOff.Ask)
    }

    @Test
    fun everythingElseStaysWithTheEngine() {
        for (uri in listOf(
            "https://example.com/",
            "http://example.com/?to=mailto:x@example.com",
            "about:blank",
            "intent://scan/#Intent;scheme=zxing;package=com.example;end",
            "market://details?id=com.example",
            "sms:+15551234",
            "geo:52.5,13.4",
            "whatsapp://send?text=hi",
            "javascript:location='tel:1'",
            "",
            "telephone",
            "mailto",
        )) {
            assertEquals(uri, HandOff.NotExternal, tapped(uri))
        }
    }

    @Test
    fun malformedOrOversizedLinksAreRefused() {
        for (uri in listOf(
            "mailto:",
            "mailto:not-an-address",
            "tel:",
            "tel:call-me",
            "tel:+1555;ext=12",
            "tel:()",
            "tel:123\u0000456",
            "mailto:x@example.com\r\nBcc: y@example.com",
            "tel:" + "1".repeat(60),
            "mailto:x@example.com?body=" + "a".repeat(ExternalLinks.MAX_URI),
        )) {
            assertEquals(uri.take(40), HandOff.Refuse, tapped(uri))
        }
    }

    @Test
    fun whatTheQuestionShowsIsBounded() {
        val long = "a".repeat(300) + "@example.com"
        val shown = (tapped("mailto:$long") as HandOff.Open).link.shown
        assertEquals(120, shown.length)
    }
}
