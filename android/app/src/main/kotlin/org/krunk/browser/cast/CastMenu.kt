package org.krunk.browser.cast

/** The casting entries of the page menu (0.15.0). */
enum class CastEntry {
    /** Send the page's video to a Cast device (Google Cast sender SDK, Play Services builds). */
    CAST,

    /** Android's own screen mirroring (system settings; every phone build). */
    CAST_SCREEN,
}

/**
 * Which casting entries the page menu shows. Plain logic (no Android or Cast classes), so the
 * JVM tests cover it:
 *  - TV: none. A TV is a cast receiver, not a sender.
 *  - Cast needs the Cast SDK in the build ([castSdk], not `-Pkrunk.noGms=true`) and working
 *    Google Play Services on the device ([gmsAvailable]).
 *  - Cast screen is the system's own mirroring screen: always there on phones and tablets.
 */
object CastMenu {
    fun entries(isTv: Boolean, castSdk: Boolean, gmsAvailable: Boolean): List<CastEntry> {
        if (isTv) return emptyList()
        return buildList {
            if (castSdk && gmsAvailable) add(CastEntry.CAST)
            add(CastEntry.CAST_SCREEN)
        }
    }
}
