package org.krunk.browser.engine

import org.krunk.browser.engine.logins.HttpAuthOffer
import org.krunk.browser.engine.logins.LoginPolicy
import org.krunk.browser.vault.VaultSecret

/**
 * The site named in a prompt's title: host and port of the page that asks, without the scheme
 * and without any `user@` part (so `https://bank.example@evil.example/` reads "evil.example").
 * Empty for a page without a site (a local file, `about:blank`, `data:`): the title then says
 * "This page".
 */
fun promptSiteOf(url: String): String = originOf(url)?.substringAfter("://").orEmpty()

/**
 * A question a page asks through the browser: `alert`, `confirm`, `prompt`, a `<select>` list or
 * an HTTP sign-in. The engine waits for the answer; the UI gives exactly one, through the
 * prompt's own function or [dismiss]. Pure data: the engine side lives in `KrunkSession`.
 *
 * Since 0.2.0 also a file field, the date, time and colour fields, "leave this page?", "send the form
 * again?" and `navigator.share()`. A folder upload is not handled (the engine dismisses it).
 */
sealed class PagePrompt {
    /** The tab whose page asks. */
    abstract val tabId: UInt

    /** The site that asks, for the dialog's title, so a page cannot speak in Krunk's name. */
    abstract val site: String

    /** Whether the dialog offers to block further dialogs from this page (see [PromptThrottle]). */
    open val canBlock: Boolean = false

    /** Closes the question without an answer (Back, a tap outside, the tab going away). */
    abstract fun dismiss()

    /** Dismisses this one and every further dialog of the page until it loads another document. */
    open fun block() = dismiss()

    class Alert(
        override val tabId: UInt,
        override val site: String,
        val message: String,
        override val canBlock: Boolean,
        private val onAnswer: (Answer) -> Unit,
    ) : PagePrompt() {
        fun ok() = onAnswer(Answer.OK)

        override fun dismiss() = onAnswer(Answer.DISMISS)

        override fun block() = onAnswer(Answer.BLOCK)
    }

    class Confirm(
        override val tabId: UInt,
        override val site: String,
        val message: String,
        override val canBlock: Boolean,
        private val onAnswer: (Answer) -> Unit,
    ) : PagePrompt() {
        fun answer(yes: Boolean) = onAnswer(if (yes) Answer.OK else Answer.CANCEL)

        override fun dismiss() = onAnswer(Answer.DISMISS)

        override fun block() = onAnswer(Answer.BLOCK)
    }

    class Text(
        override val tabId: UInt,
        override val site: String,
        val message: String,
        val default: String,
        override val canBlock: Boolean,
        private val onAnswer: (Answer, String) -> Unit,
    ) : PagePrompt() {
        fun answer(text: String) = onAnswer(Answer.OK, text)

        override fun dismiss() = onAnswer(Answer.DISMISS, "")

        override fun block() = onAnswer(Answer.BLOCK, "")
    }

    /** HTTP authentication. [site] is the server asking, [insecure] says the password would travel unencrypted. */
    class SignIn(
        override val tabId: UInt,
        override val site: String,
        val message: String,
        val username: String,
        val onlyPassword: Boolean,
        val insecure: Boolean,
        private val onAnswer: (user: String, password: String) -> Unit,
        private val onDismiss: () -> Unit,
        /** What the dialog shows of the vault (0.3.0), for the server at [authUrl]. */
        val saved: HttpAuthOffer.Saved = HttpAuthOffer.Saved.NONE,
        val authUrl: String = "",
    ) : PagePrompt() {
        fun answer(user: String, password: String) = onAnswer(user, password)

        override fun dismiss() = onDismiss()
    }

    /**
     * A sign-in field was tapped on a page the vault has logins for (0.3.0). The engine holds no
     * password at this point: the picker hands it the one login the user chooses. With
     * [unlockFirst] the vault password is asked first; with [warnInsecure] the page was loaded
     * without encryption and the user must confirm before anything is offered.
     */
    class LoginFill(
        override val tabId: UInt,
        override val site: String,
        val pageUrl: String,
        val unlockFirst: Boolean,
        val warnInsecure: Boolean,
        private val onPick: (VaultSecret) -> Unit,
        private val onDismiss: () -> Unit,
    ) : PagePrompt() {
        fun pick(secret: VaultSecret) = onPick(secret)

        override fun dismiss() = onDismiss()
    }

    /**
     * Offer to save the login a page (or an HTTP sign-in) just sent (0.3.0). Krunk's own
     * question: the engine's request is answered at once and nothing is stored by the engine.
     * The password stays in this object until the user answers; it is never shown.
     */
    class LoginSave(
        override val tabId: UInt,
        override val site: String,
        val origin: String,
        val username: String,
        private val password: String,
        val kind: LoginPolicy.SaveKind,
        val warnInsecure: Boolean,
        private val onClosed: (LoginSave) -> Unit,
    ) : PagePrompt() {
        private var closed = false

        /** The login, for the one who stores it. */
        fun login(): VaultSecret = VaultSecret(username, password)

        /** Answered (saved, never, not now): the dialog goes away. */
        fun close() {
            if (!closed) {
                closed = true
                onClosed(this)
            }
        }

        override fun dismiss() = close()
    }

    /** A `<select>` list (or a menu): one option, or several when [multiple]. */
    class Choice(
        override val tabId: UInt,
        override val site: String,
        val message: String,
        val multiple: Boolean,
        val options: List<Option>,
        private val onAnswer: (ids: List<String>) -> Unit,
        private val onDismiss: () -> Unit,
    ) : PagePrompt() {
        fun answer(ids: List<String>) = onAnswer(ids)

        override fun dismiss() = onDismiss()
    }

    /**
     * A file field (`<input type=file>`). Answered with the picked documents' addresses
     * (`content:` or `file:`), or dismissed, which gives the page no file.
     */
    class File(
        override val tabId: UInt,
        override val site: String,
        val request: FileRequest,
        private val onAnswer: (uris: List<String>) -> Unit,
        private val onDismiss: () -> Unit,
    ) : PagePrompt() {
        /**
         * The system's picker (or the camera) was started for this prompt. The UI may be built
         * again while it is open (rotation); it must not be started twice.
         */
        var launched = false

        fun answer(uris: List<String>) =
            if (uris.isEmpty()) onDismiss() else onAnswer(if (request.multiple) uris else uris.take(1))

        override fun dismiss() = onDismiss()
    }

    /**
     * A date or time field. [value], [min] and [max] are in the field's own format (see
     * [DateValues]) and may be empty or unreadable. Answering "" clears the field; dismissing
     * leaves it as it was.
     */
    class DateTime(
        override val tabId: UInt,
        override val site: String,
        val kind: DateKind,
        val value: String,
        val min: String,
        val max: String,
        private val onAnswer: (String) -> Unit,
        private val onDismiss: () -> Unit,
    ) : PagePrompt() {
        fun answer(value: String) = onAnswer(value)

        fun clear() = onAnswer("")

        override fun dismiss() = onDismiss()
    }

    /**
     * A colour field. [value] is the field's colour (`#rrggbb`), [suggested] the colours the
     * page lists for it, possibly none. Dismissing leaves the field as it was.
     */
    class Colour(
        override val tabId: UInt,
        override val site: String,
        val value: String,
        val suggested: List<String>,
        private val onAnswer: (String) -> Unit,
        private val onDismiss: () -> Unit,
    ) : PagePrompt() {
        fun answer(colour: String) = onAnswer(colour)

        override fun dismiss() = onDismiss()
    }

    /** "Leave this page?" (`beforeunload`). Dismissing stays on the page. */
    class LeavePage(
        override val tabId: UInt,
        override val site: String,
        private val onAnswer: (leave: Boolean) -> Unit,
    ) : PagePrompt() {
        fun answer(leave: Boolean) = onAnswer(leave)

        override fun dismiss() = onAnswer(false)
    }

    /** Going back or reloading would send a form again. Dismissing does not send it. */
    class Repost(override val tabId: UInt, override val site: String, private val onAnswer: (send: Boolean) -> Unit) :
        PagePrompt() {
        fun answer(send: Boolean) = onAnswer(send)

        override fun dismiss() = onAnswer(false)
    }

    /** `navigator.share()`: [text] is what goes to Android's share sheet (see [Shares.text]). */
    class Share(
        override val tabId: UInt,
        override val site: String,
        val title: String,
        val text: String,
        private val onResult: (ShareOutcome) -> Unit,
    ) : PagePrompt() {
        /** The share sheet was opened for this prompt (see [File.launched]). */
        var launched = false

        fun result(outcome: ShareOutcome) = onResult(outcome)

        override fun dismiss() = onResult(ShareOutcome.ABORTED)
    }

    /**
     * One row of a [Choice]. A [header] is a group label (not selectable); [indented] rows belong
     * to the group above them.
     */
    data class Option(
        val id: String,
        val label: String,
        val selected: Boolean = false,
        val disabled: Boolean = false,
        val header: Boolean = false,
        val indented: Boolean = false,
        val separator: Boolean = false,
    )

    enum class Answer { OK, CANCEL, DISMISS, BLOCK }
}

/**
 * Keeps a page from trapping the user in dialogs (`while (true) alert()`): from the second
 * script dialog of one document on, the dialog offers to block the rest, and once blocked every
 * further one is dismissed unseen until another document loads. Lists and sign-ins the user
 * asked for by tapping are not counted.
 */
class PromptThrottle {
    private var shown = 0

    var blocked = false
        private set

    /** A script dialog is about to show: null when it is blocked, else whether it offers blocking. */
    fun next(): Boolean? {
        if (blocked) return null
        shown++
        return shown >= OFFER_FROM
    }

    fun block() {
        blocked = true
    }

    /** Another document loaded: it starts with a clean slate. */
    fun newDocument() {
        shown = 0
        blocked = false
    }

    private companion object {
        const val OFFER_FROM = 2
    }
}

/**
 * The prompts waiting for an answer, oldest first. Only a prompt of the tab on screen is
 * shown; one of a background tab waits until the user goes there, and a closed tab's prompts
 * are dismissed.
 */
class PromptQueue {
    private val waiting = ArrayList<PagePrompt>()

    fun add(prompt: PagePrompt) {
        waiting += prompt
    }

    /** The prompt to show while [activeTab] is on screen. */
    fun current(activeTab: UInt?): PagePrompt? = waiting.firstOrNull { it.tabId == activeTab }

    /** [prompt] was answered or dismissed. */
    fun remove(prompt: PagePrompt) {
        waiting.remove(prompt)
    }

    /** A tab went away (closed, crashed, unloaded): its prompts are dismissed and forgotten. */
    fun dropTab(tabId: UInt) {
        val gone = waiting.filter { it.tabId == tabId }
        waiting.removeAll(gone.toSet())
        gone.forEach { it.dismiss() }
    }

    val size: Int get() = waiting.size
}
