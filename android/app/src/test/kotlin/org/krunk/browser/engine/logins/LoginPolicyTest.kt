package org.krunk.browser.engine.logins

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.logins.LoginPolicy.Fill
import org.krunk.browser.engine.logins.LoginPolicy.Save
import org.krunk.browser.engine.logins.LoginPolicy.SaveKind
import org.krunk.browser.engine.logins.LoginVault.State
import org.krunk.browser.vault.SaveDecision

class LoginPolicyTest {
    private fun fill(
        url: String = "https://shop.example/login",
        private: Boolean = false,
        armed: String? = null,
        suppressed: Boolean = false,
        form: String? = "https://shop.example",
        vault: State = State.UNLOCKED,
        hint: Boolean = true,
        tap: Boolean = true,
    ) = LoginPolicy.fill(LoginPolicy.FillContext(private, armed, suppressed, url, form, vault, hint, tap))

    @Test
    fun onlyATapBringsThePickerNeverAFocusThePageSetItself() {
        assertTrue(fill(tap = false) is Fill.No)
        assertTrue(fill(tap = false, vault = State.LOCKED) is Fill.No)
        // Not even in a tab armed with "Fill from vault": it waits for the tap too.
        assertTrue(fill(tap = false, armed = "https://shop.example") is Fill.No)
        assertTrue(LoginPolicy.recentTap(0))
        assertTrue(LoginPolicy.recentTap(LoginPolicy.TAP_WINDOW_MS))
        assertFalse(LoginPolicy.recentTap(LoginPolicy.TAP_WINDOW_MS + 1))
        assertFalse("never touched", LoginPolicy.recentTap(null))
        assertFalse(LoginPolicy.recentTap(-5))
    }

    @Test
    fun anOpenVaultOffersOnAnHttpsPage() {
        assertEquals(Fill.Ask(unlockFirst = false, warnInsecure = false), fill())
    }

    @Test
    fun aLockedVaultAsksForUnlockOnlyWhereItHasLogins() {
        assertEquals(Fill.Ask(unlockFirst = true, warnInsecure = false), fill(vault = State.LOCKED, hint = true))
        assertTrue(fill(vault = State.LOCKED, hint = false) is Fill.No)
    }

    @Test
    fun noVaultOrALostKeyOffersNothing() {
        assertTrue(fill(vault = State.NONE) is Fill.No)
        assertTrue(fill(vault = State.KEY_LOST) is Fill.No)
    }

    @Test
    fun aPrivateTabGetsNothingUnlessArmedForThisVeryOrigin() {
        assertTrue(fill(private = true) is Fill.No)
        assertTrue(fill(private = true, armed = "https://other.example") is Fill.No)
        assertEquals(
            Fill.Ask(unlockFirst = false, warnInsecure = false),
            fill(private = true, armed = "https://shop.example"),
        )
        // Armed and locked: the unlock is asked even without a hint (the user asked for it).
        assertEquals(
            Fill.Ask(unlockFirst = true, warnInsecure = false),
            fill(private = true, armed = "https://shop.example", vault = State.LOCKED, hint = false),
        )
    }

    @Test
    fun aPlainHttpPageNeedsTheWarning() {
        assertEquals(
            Fill.Ask(unlockFirst = false, warnInsecure = true),
            fill(url = "http://shop.example/login", form = "http://shop.example"),
        )
        assertEquals(
            Fill.Ask(unlockFirst = true, warnInsecure = true),
            fill(url = "http://10.0.0.1:8080/x", form = "http://10.0.0.1:8080", vault = State.LOCKED),
        )
    }

    @Test
    fun aFormOfAnotherOriginThanTheTabIsNotFilled() {
        assertTrue(fill(form = "https://evil.example") is Fill.No)
        assertTrue(fill(form = "http://shop.example") is Fill.No)
        assertTrue(fill(form = "https://shop.example:8443") is Fill.No)
        assertTrue(fill(form = null) is Fill.No)
    }

    @Test
    fun aPageThatIsNotAWebPageIsNotFilled() {
        assertTrue(fill(url = "about:blank", form = "about:blank") is Fill.No)
        assertTrue(fill(url = "file:///sdcard/a.html", form = "file://") is Fill.No)
    }

    @Test
    fun aClosedPickerStaysClosedOnThatPageUntilArmedAgain() {
        assertTrue(fill(suppressed = true) is Fill.No)
        assertTrue(fill(suppressed = true, armed = "https://shop.example") is Fill.Ask)
    }

    private fun save(
        private: Boolean = false,
        origin: String? = "https://shop.example",
        empty: Boolean = false,
        vault: State = State.UNLOCKED,
        never: Boolean = false,
        offers: Boolean = true,
        known: SaveDecision? = SaveDecision.New,
    ) = LoginPolicy.save(LoginPolicy.SaveContext(private, origin, empty, vault, never, offers, known))

    @Test
    fun saveOffersNewAndUpdateAndNothingWhenUnchanged() {
        assertEquals(Save.Offer(SaveKind.NEW, null, false), save())
        assertEquals(Save.Offer(SaveKind.UPDATE, "id1", false), save(known = SaveDecision.Update("id1")))
        assertTrue(save(known = SaveDecision.Unchanged) is Save.No)
        // The vault could not be asked: offered anyway (the dialog asks it again).
        assertEquals(Save.Offer(SaveKind.NEW, null, false), save(known = null))
    }

    @Test
    fun neverInAPrivateTab() {
        for (v in State.entries) assertTrue(save(private = true, vault = v) is Save.No)
    }

    @Test
    fun aLockedVaultOffersAndAsksForUnlockLater() {
        assertEquals(Save.Offer(SaveKind.LOCKED, null, false), save(vault = State.LOCKED, known = null))
    }

    @Test
    fun withoutAVaultTheOfferIsToCreateOne() {
        assertEquals(Save.Offer(SaveKind.NO_VAULT, null, false), save(vault = State.NONE, known = null))
        assertTrue(save(vault = State.KEY_LOST, known = null) is Save.No)
    }

    @Test
    fun neverForThisSiteAndTheSwitchSilenceTheOffer() {
        assertTrue(save(never = true) is Save.No)
        assertTrue(save(offers = false) is Save.No)
        assertTrue(save(empty = true) is Save.No)
        assertTrue(save(origin = null) is Save.No)
        assertTrue(save(origin = "moz-extension://abc") is Save.No)
    }

    @Test
    fun aPlainHttpLoginIsOfferedWithTheWarning() {
        assertEquals(Save.Offer(SaveKind.NEW, null, true), save(origin = "http://shop.example"))
        assertEquals(
            Save.Offer(SaveKind.LOCKED, null, true),
            save(origin = "http://10.0.0.1:8080", vault = State.LOCKED),
        )
    }

    @Test
    fun aFillPromptIsOnlyGoodForItsTabAndOrigin() {
        assertTrue(LoginPolicy.stillValid(1u, "https://a.example/x", 1u, "https://a.example/y"))
        assertFalse(LoginPolicy.stillValid(1u, "https://a.example/x", 2u, "https://a.example/x"))
        assertFalse(LoginPolicy.stillValid(1u, "https://a.example/x", null, "https://a.example/x"))
        assertFalse(LoginPolicy.stillValid(1u, "https://a.example/x", 1u, "http://a.example/x"))
        assertFalse(LoginPolicy.stillValid(1u, "https://a.example/x", 1u, "https://sub.a.example/x"))
    }

    @Test
    fun aSaveOfferFollowsTheSignInWithinTheSiteOnly() {
        assertTrue(LoginPolicy.saveStillValid(1u, "https://login.a.example", 1u, "https://a.example/home"))
        assertTrue(LoginPolicy.saveStillValid(1u, "https://a.example", 1u, "https://app.a.example/"))
        assertFalse(LoginPolicy.saveStillValid(1u, "https://a.example", 1u, "https://b.example/"))
        assertFalse(LoginPolicy.saveStillValid(1u, "https://a.example", 1u, "https://nota.example/"))
        assertFalse(LoginPolicy.saveStillValid(1u, "http://a.example", 1u, "http://www.a.example/"))
        assertTrue(LoginPolicy.saveStillValid(1u, "http://a.example:8080", 1u, "http://a.example:8080/next"))
        assertFalse(LoginPolicy.saveStillValid(1u, "https://a.example", 2u, "https://a.example/"))
        assertFalse(LoginPolicy.saveStillValid(1u, "https://a.example", 1u, "about:blank"))
    }

    private val sites: (String) -> String? = { host ->
        when {
            host.endsWith(".github.io") -> host.split('.').takeLast(3).joinToString(".")
            else -> host.split('.').takeLast(2).joinToString(".")
        }
    }

    @Test
    fun aSaveOfferFollowsASignInToASiblingSubdomain() {
        assertTrue(LoginPolicy.saveStillValid(1u, "https://login.a.example", 1u, "https://app.a.example/", sites))
        assertTrue(LoginPolicy.saveStillValid(1u, "https://accounts.a.example", 1u, "https://www.a.example/", sites))
        // Not across a public suffix, not over http, not to another site.
        assertFalse(LoginPolicy.saveStillValid(1u, "https://x.github.io", 1u, "https://y.github.io/", sites))
        assertFalse(LoginPolicy.saveStillValid(1u, "http://login.a.example", 1u, "http://app.a.example/", sites))
        assertFalse(LoginPolicy.saveStillValid(1u, "https://login.a.example", 1u, "https://b.example/", sites))
        // Without the core's answer only a host and its subdomains count.
        assertFalse(LoginPolicy.saveStillValid(1u, "https://login.a.example", 1u, "https://app.a.example/") { null })
    }

    private fun suggest(
        url: String = "https://shop.example/signup",
        private: Boolean = false,
        form: String? = null,
        suppressed: Boolean = false,
        enabled: Boolean = true,
        tap: Boolean = true,
        vault: State = State.UNLOCKED,
    ) = LoginPolicy.suggest(LoginPolicy.SuggestContext(private, url, form, suppressed, enabled, tap, vault))

    @Test
    fun aStrongPasswordIsSuggestedAtATapOnANormalTabsOwnForm() {
        assertTrue(suggest())
        assertTrue(suggest(form = "https://shop.example"))
        assertTrue("without a vault: saving creates it", suggest(vault = State.NONE))
        assertTrue(suggest(vault = State.LOCKED))
        assertTrue("http page", suggest(url = "http://10.0.0.1:8080/signup"))
        assertFalse(suggest(vault = State.KEY_LOST))
        assertFalse(suggest(private = true))
        assertFalse(suggest(form = "https://other.example"))
        assertFalse(suggest(suppressed = true))
        assertFalse(suggest(enabled = false))
        assertFalse(suggest(tap = false))
        assertFalse(suggest(url = "about:blank"))
    }

    @Test
    fun theEngineHearsOnlyAboutOpenTabsOfTheDomainAndNeverAboutUnarmedPrivateOnes() {
        val pages = listOf(
            LoginPage("https://shop.example/login", private = false, armedOrigin = null),
            LoginPage("https://www.shop.example/", private = false, armedOrigin = null),
            LoginPage("https://shop.example/other", private = false, armedOrigin = null),
            LoginPage("https://pay.shop.example/", private = true, armedOrigin = null),
            LoginPage("https://my.shop.example/", private = true, armedOrigin = "https://my.shop.example"),
            LoginPage("https://notshop.example/", private = false, armedOrigin = null),
            LoginPage("about:blank", private = false, armedOrigin = null),
            LoginPage("http://shop.example:8080/", private = false, armedOrigin = null),
        )
        val got = LoginsHub.announcedOrigins("shop.example", pages, State.UNLOCKED)
        assertEquals(
            listOf(
                "https://shop.example" to false,
                "https://www.shop.example" to false,
                "https://my.shop.example" to true,
                "http://shop.example:8080" to false,
            ),
            got.map { it.origin to it.armed },
        )
        assertTrue(LoginsHub.announcedOrigins("shop.example", pages, State.NONE).isEmpty())
        assertTrue(LoginsHub.announcedOrigins("shop.example", pages, State.KEY_LOST).isEmpty())
        assertEquals(4, LoginsHub.announcedOrigins("SHOP.example", pages, State.LOCKED).size)
        assertEquals(
            listOf("http://10.0.0.1:18080"),
            LoginsHub.announcedOrigins(
                "10.0.0.1",
                listOf(LoginPage("http://10.0.0.1:18080/login.html", false, null)),
                State.UNLOCKED,
            ).map { it.origin },
        )
    }
}
