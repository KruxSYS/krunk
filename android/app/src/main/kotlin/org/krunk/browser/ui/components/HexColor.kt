package org.krunk.browser.ui.components

/** Colours as HTML writes them (`#rrggbb`), read and written without Android. */
object HexColor {
    private val long = Regex("""#?([0-9a-fA-F]{6})""")
    private val short = Regex("""#?([0-9a-fA-F]{3})""")

    /** A plain set to choose from when nobody suggests colours: strong hues, then greys. */
    val PALETTE: List<String> = listOf(
        "#d32f2f", "#e91e63", "#9c27b0", "#673ab7",
        "#3f51b5", "#2563eb", "#0288d1", "#0097a7",
        "#00796b", "#388e3c", "#689f38", "#f9a825",
        "#f57c00", "#e64a19", "#795548", "#607d8b",
        "#000000", "#555555", "#aaaaaa", "#ffffff",
    )

    /** `#rrggbb` in lower case for `#rrggbb`, `rrggbb`, `#rgb` or `rgb` (any case, spaces around); else null. */
    fun parse(text: String?): String? {
        val t = text?.trim().orEmpty()
        long.matchEntire(t)?.let { return "#" + it.groupValues[1].lowercase() }
        short.matchEntire(t)?.let { m ->
            return "#" + m.groupValues[1].lowercase().map { "$it$it" }.joinToString("")
        }
        return null
    }

    /** The colour as an opaque ARGB number (what Compose's `Color(Int)` takes); null when unreadable. */
    fun argb(text: String?): Int? = parse(text)?.let { OPAQUE or it.substring(1).toInt(HEX) }

    /**
     * The colours to offer: the readable ones of [suggested], each once, at most [MAX_SWATCHES],
     * or [PALETTE] when there are none; [current] is always among them.
     */
    fun swatches(suggested: List<String>, current: String?): List<String> {
        val own = suggested.mapNotNull(::parse).distinct().take(MAX_SWATCHES)
        val base = own.ifEmpty { PALETTE }
        val now = parse(current) ?: return base
        return if (now in base) base else listOf(now) + base.take(MAX_SWATCHES - 1)
    }

    const val MAX_SWATCHES = 24
    private const val HEX = 16
    private const val OPAQUE = 0xFF000000.toInt()
}
