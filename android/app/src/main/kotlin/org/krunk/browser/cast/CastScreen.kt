package org.krunk.browser.cast

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.provider.Settings
import android.util.Log
import org.krunk.browser.KrunkApp

/**
 * "Cast screen": Android's own screen mirroring. Krunk only opens the system screen for it; the
 * system does the casting (no Play Services, no Cast SDK). Phones name it differently ("Cast",
 * "Smart View", "Screen mirroring"); `android.settings.CAST_SETTINGS` reaches it on most,
 * otherwise the wireless settings, otherwise the settings app.
 */
object CastScreen {
    private val ACTIONS = listOf(
        "android.settings.CAST_SETTINGS",
        Settings.ACTION_WIRELESS_SETTINGS,
        Settings.ACTION_SETTINGS,
    )

    /** Opens the first system screen that exists; false when none did. */
    fun open(context: Context): Boolean {
        for (action in ACTIONS) {
            val intent = Intent(action)
            if (context !is android.app.Activity) intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            try {
                context.startActivity(intent)
                return true
            } catch (e: ActivityNotFoundException) {
                Log.i(KrunkApp.TAG, "cast screen: no $action")
            } catch (e: SecurityException) {
                Log.w(KrunkApp.TAG, "cast screen: $action refused", e)
            }
        }
        return false
    }
}
