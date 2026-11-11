package org.krunk.browser.engine

/**
 * A question a page asks through the browser: `alert`, `confirm`, `prompt`, a `<select>` list or
 * an HTTP sign-in. The engine waits for the answer; the UI gives exactly one, through the
 * prompt's own function or [dismiss]. Pure data: the engine side lives in `KrunkSession`.
 *
 * File, date/time and colour pickers are not handled yet (they are dismissed by the engine).
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
    ) : PagePrompt() {
        fun answer(user: String, password: String) = onAnswer(user, password)

        override fun dismiss() = onDismiss()
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
