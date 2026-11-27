package org.krunk.browser.cast

import android.content.Context
import com.google.android.gms.cast.CastMediaControlIntent
import com.google.android.gms.cast.framework.CastOptions
import com.google.android.gms.cast.framework.OptionsProvider
import com.google.android.gms.cast.framework.SessionProvider
import com.google.android.gms.cast.framework.media.CastMediaOptions
import com.google.android.gms.cast.framework.media.NotificationOptions

/**
 * The Cast framework's options (named in src/gms/AndroidManifest.xml): Google's Default Media
 * Receiver (CC1AD845, plays an address it fetches itself; no receiver app of Krunk's own), the
 * SDK's media notification and [KrunkExpandedControllerActivity] as the full-screen controls.
 */
class CastOptionsProvider : OptionsProvider {
    override fun getCastOptions(context: Context): CastOptions {
        val controls = KrunkExpandedControllerActivity::class.java.name
        val notification = NotificationOptions.Builder()
            .setTargetActivityClassName(controls)
            .build()
        val media = CastMediaOptions.Builder()
            .setNotificationOptions(notification)
            .setExpandedControllerActivityClassName(controls)
            .build()
        return CastOptions.Builder()
            .setReceiverApplicationId(CastMediaControlIntent.DEFAULT_MEDIA_RECEIVER_APPLICATION_ID)
            .setCastMediaOptions(media)
            .build()
    }

    override fun getAdditionalSessionProviders(context: Context): List<SessionProvider>? = null
}
