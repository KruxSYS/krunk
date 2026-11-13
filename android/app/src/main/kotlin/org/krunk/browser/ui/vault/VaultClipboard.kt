package org.krunk.browser.ui.vault

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import java.util.UUID

/** What to do about a copied password whose time may be up. */
enum class ClipAction {
    /** Nothing of ours is waiting, or its time is not up. */
    NOTHING,

    /** The clipboard still holds our clip: empty it. */
    CLEAR,

    /** Something else was copied since: it is not ours to remove. */
    FORGET,

    /** The app is in the background, where the clipboard cannot be read: look again when it is back. */
    WAIT_FOR_FOREGROUND,

    /** In the foreground but the clipboard would not say (no window focus yet): look again shortly. */
    RETRY,
}

/**
 * The rule for a copied password, 30 s later: removed only when the clipboard still holds it.
 *
 * [holdsOurClip] is what reading the clipboard said: true/false, or null when it could not be
 * read (from Android 10 on only the focused app may). It is read only in the foreground.
 */
fun clipExpiryAction(pending: Boolean, due: Boolean, foreground: Boolean, holdsOurClip: Boolean?): ClipAction = when {
    !pending || !due -> ClipAction.NOTHING
    !foreground -> ClipAction.WAIT_FOR_FOREGROUND
    holdsOurClip == null -> ClipAction.RETRY
    holdsOurClip -> ClipAction.CLEAR
    else -> ClipAction.FORGET
}

/** From Android 13 on the system shows its own "copied" confirmation; before, Krunk shows one. */
fun showsOwnCopyConfirmation(sdk: Int): Boolean = sdk < SDK_TIRAMISU

private const val SDK_TIRAMISU = 33

/**
 * Copies from the vault. A password is marked sensitive (Android 13 and later then hide it in
 * the clipboard preview) and is taken off the clipboard after [CLEAR_AFTER_MS], unless something
 * else was copied in the meantime. The password itself is not kept for the comparison: the clip
 * carries a random label and that is compared.
 *
 * Main thread only.
 */
object VaultClipboard {
    const val CLEAR_AFTER_MS = 30_000L

    /** `ClipDescription.EXTRA_IS_SENSITIVE` (API 33); older versions ignore the extra. */
    private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
    private const val RETRY_MS = 1_000L
    private const val MAX_RETRIES = 5

    private val handler = Handler(Looper.getMainLooper())
    private var pendingLabel: String? = null
    private var dueAt = 0L
    private var retries = 0
    private var observing = false
    private var appContext: Context? = null
    private val check = Runnable { settle() }

    /** A user name: an ordinary copy. Returns whether Krunk should confirm it itself. */
    fun copyText(context: Context, text: String): Boolean {
        val cm = clipboard(context) ?: return false
        // Whatever password was waiting is replaced by this.
        forget()
        return runCatching { cm.setPrimaryClip(ClipData.newPlainText("Krunk", text)) }.isSuccess &&
            showsOwnCopyConfirmation(Build.VERSION.SDK_INT)
    }

    /** A password. Returns whether Krunk should confirm the copy itself. */
    fun copyPassword(context: Context, password: String): Boolean {
        val cm = clipboard(context) ?: return false
        val label = "Krunk " + UUID.randomUUID()
        val clip = ClipData.newPlainText(label, password)
        clip.description.extras = PersistableBundle().apply { putBoolean(EXTRA_IS_SENSITIVE, true) }
        if (runCatching { cm.setPrimaryClip(clip) }.isFailure) return false
        appContext = context.applicationContext
        pendingLabel = label
        dueAt = android.os.SystemClock.elapsedRealtime() + CLEAR_AFTER_MS
        retries = 0
        observeForeground()
        handler.removeCallbacks(check)
        handler.postDelayed(check, CLEAR_AFTER_MS)
        return showsOwnCopyConfirmation(Build.VERSION.SDK_INT)
    }

    private fun settle() {
        val context = appContext
        val label = pendingLabel
        if (context == null || label == null) return
        val cm = clipboard(context)
        if (cm == null) {
            forget()
            return
        }
        val foreground = ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)
        val left = dueAt - android.os.SystemClock.elapsedRealtime()
        val due = left <= 0
        if (!due) {
            // Back in the foreground before the time is up: the timer goes on.
            handler.removeCallbacks(check)
            handler.postDelayed(check, left)
            return
        }
        val holds = if (foreground && due) holdsLabel(cm, label) else null
        when (clipExpiryAction(pending = true, due = due, foreground = foreground, holdsOurClip = holds)) {
            ClipAction.CLEAR -> {
                clear(cm)
                forget()
            }

            ClipAction.FORGET -> forget()

            // Each return to the foreground gets its own tries; between them nothing runs.
            ClipAction.RETRY -> if (retries++ < MAX_RETRIES) handler.postDelayed(check, RETRY_MS)

            ClipAction.WAIT_FOR_FOREGROUND, ClipAction.NOTHING -> Unit
        }
    }

    /** True/false when the clipboard said what it holds; null when it would not. */
    private fun holdsLabel(cm: ClipboardManager, label: String): Boolean? = runCatching {
        if (!cm.hasPrimaryClip()) {
            // "Empty" is also what an app without focus is told.
            null
        } else {
            cm.primaryClipDescription?.label?.toString()?.let { it == label }
        }
    }.getOrNull()

    private fun clear(cm: ClipboardManager) {
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                cm.clearPrimaryClip()
            } else {
                // Before Android 9 there is no clear call: an empty clip replaces ours.
                cm.setPrimaryClip(ClipData.newPlainText("", ""))
            }
        }
    }

    private fun forget() {
        pendingLabel = null
        handler.removeCallbacks(check)
    }

    private fun observeForeground() {
        if (observing) return
        observing = true
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) {
                if (pendingLabel == null) return
                retries = 0
                handler.removeCallbacks(check)
                // The window has no focus yet at this point; the clipboard answers once it has.
                handler.postDelayed(check, RETRY_MS)
            }
        })
    }

    private fun clipboard(context: Context): ClipboardManager? =
        context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
}
