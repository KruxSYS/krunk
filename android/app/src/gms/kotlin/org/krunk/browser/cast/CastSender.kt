package org.krunk.browser.cast

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log
import android.view.ContextThemeWrapper
import androidx.core.content.ContextCompat
import androidx.mediarouter.app.MediaRouteChooserDialog
import com.google.android.gms.cast.MediaInfo
import com.google.android.gms.cast.MediaLoadRequestData
import com.google.android.gms.cast.MediaMetadata
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.SessionManagerListener
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.common.images.WebImage
import org.krunk.browser.KrunkApp
import org.krunk.browser.R

/**
 * The Cast sender (0.15.0, builds with Google Play Services): sends a page's video to a Cast
 * device's Default Media Receiver, which fetches the address itself. Krunk only hands over the
 * address, type, title, poster and position; cookies and headers of the page are not sent.
 *
 * Flow: [cast] gets the [CastContext] (it can fail on devices with old or broken Play Services:
 * then Cast is reported unavailable for the rest of the run), loads the video at once when a
 * session is already connected, otherwise opens the route chooser and loads it when the session
 * the user picked has started. Playback controls are the SDK's: its media notification and
 * [KrunkExpandedControllerActivity] (play/pause, seek, stop). Main thread only.
 */
object CastSender {
    /** Set once getting the CastContext failed: the menu stops offering Cast. */
    private var broken = false
    private var gmsOk: Boolean? = null
    private var listening = false

    /** What waits for a session the user is picking in the chooser, and when it was asked for. */
    private var pending: Pending? = null

    private class Pending(val media: CastMedia, val context: Context, val say: (String) -> Unit, val at: Long)

    /** Whether Play Services can run the Cast framework on this device (checked once per run). */
    fun available(context: Context): Boolean {
        if (broken) return false
        val ok = gmsOk ?: runCatching {
            GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(context) == ConnectionResult.SUCCESS
        }.getOrDefault(false).also { gmsOk = it }
        return ok
    }

    fun cast(context: Context, media: CastMedia, say: (String) -> Unit) {
        if (!available(context)) {
            say(context.getString(R.string.cast_unavailable))
            return
        }
        val task = try {
            CastContext.getSharedInstance(context.applicationContext, ContextCompat.getMainExecutor(context))
        } catch (e: Exception) {
            fail(context, say, e)
            return
        }
        task.addOnSuccessListener { castContext -> withContext(castContext, context, media, say) }
            .addOnFailureListener { e -> fail(context, say, e) }
    }

    private fun fail(context: Context, say: (String) -> Unit, e: Exception) {
        Log.w(KrunkApp.TAG, "cast: no CastContext", e)
        broken = true
        say(context.getString(R.string.cast_unavailable))
    }

    private fun withContext(castContext: CastContext, context: Context, media: CastMedia, say: (String) -> Unit) {
        val sessions = try {
            castContext.sessionManager
        } catch (e: IllegalStateException) {
            fail(context, say, e)
            return
        }
        if (!listening) {
            sessions.addSessionManagerListener(Listener, CastSession::class.java)
            listening = true
        }
        val current = sessions.currentCastSession
        if (current != null && current.isConnected) {
            pending = null
            load(current, media, context, say)
            return
        }
        pending = Pending(media, context, say, System.currentTimeMillis())
        val selector = try {
            castContext.mergedSelector
        } catch (e: IllegalStateException) {
            null
        }
        if (selector == null) {
            pending = null
            fail(context, say, IllegalStateException("no route selector"))
            return
        }
        // MainActivity is a ComponentActivity with a platform theme: the MediaRouter dialog
        // needs an AppCompat one (src/gms/res/values/themes_cast.xml).
        val themed = ContextThemeWrapper(context, R.style.Theme_Krunk_CastDialog)
        // The dialog's own title ("Cast to"); its setTitle crashes before show() (no view yet).
        runCatching {
            MediaRouteChooserDialog(themed, R.style.Theme_Krunk_CastDialog).apply {
                routeSelector = selector
                show()
            }
        }.onFailure {
            pending = null
            Log.w(KrunkApp.TAG, "cast: no device chooser", it)
            say(context.getString(R.string.cast_unavailable))
        }
    }

    private fun load(session: CastSession, media: CastMedia, context: Context, say: (String) -> Unit) {
        val client = session.remoteMediaClient
        if (client == null) {
            say(context.getString(R.string.cast_failed))
            return
        }
        val metadata = MediaMetadata(MediaMetadata.MEDIA_TYPE_MOVIE).apply {
            putString(MediaMetadata.KEY_TITLE, media.title)
            media.poster?.let { addImage(WebImage(Uri.parse(it))) }
        }
        val info = MediaInfo.Builder(media.url)
            .setContentUrl(media.url)
            .setContentType(media.contentType)
            .setStreamType(if (media.live) MediaInfo.STREAM_TYPE_LIVE else MediaInfo.STREAM_TYPE_BUFFERED)
            .setMetadata(metadata)
            .build()
        val request = MediaLoadRequestData.Builder()
            .setMediaInfo(info)
            .setAutoplay(true)
            .setCurrentTime(media.startMs)
            .build()
        val device = session.castDevice?.friendlyName.orEmpty()
        client.load(request).setResultCallback { result ->
            if (result.status.isSuccess) {
                if (device.isNotEmpty()) say(context.getString(R.string.cast_sent, device))
                runCatching {
                    context.startActivity(Intent(context, KrunkExpandedControllerActivity::class.java))
                }.onFailure { Log.w(KrunkApp.TAG, "cast: no controller screen", it) }
            } else {
                Log.w(KrunkApp.TAG, "cast: load failed: ${result.status.statusCode} ${result.status.statusMessage}")
                say(context.getString(R.string.cast_failed))
            }
        }
    }

    /** Loads what waited for the chooser once the session the user picked is up. */
    private object Listener : SessionManagerListener<CastSession> {
        private fun take(): Pending? {
            val p = pending ?: return null
            pending = null
            // A chooser closed without a pick leaves its request behind: not for a later session.
            return p.takeIf { System.currentTimeMillis() - it.at < PENDING_MS }
        }

        override fun onSessionStarted(session: CastSession, sessionId: String) {
            take()?.let { load(session, it.media, it.context, it.say) }
        }

        override fun onSessionResumed(session: CastSession, wasSuspended: Boolean) {
            take()?.let { load(session, it.media, it.context, it.say) }
        }

        override fun onSessionStartFailed(session: CastSession, error: Int) {
            Log.w(KrunkApp.TAG, "cast: session start failed: $error")
            take()?.let { it.say(it.context.getString(R.string.cast_connect_failed)) }
        }

        override fun onSessionStarting(session: CastSession) = Unit

        override fun onSessionEnding(session: CastSession) = Unit

        override fun onSessionEnded(session: CastSession, error: Int) = Unit

        override fun onSessionResuming(session: CastSession, sessionId: String) = Unit

        override fun onSessionResumeFailed(session: CastSession, error: Int) = Unit

        override fun onSessionSuspended(session: CastSession, reason: Int) = Unit
    }

    private const val PENDING_MS = 120_000L
}
