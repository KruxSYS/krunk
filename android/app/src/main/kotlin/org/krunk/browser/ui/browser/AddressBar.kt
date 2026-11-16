package org.krunk.browser.ui.browser

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Masks
import androidx.compose.material.icons.outlined.NorthWest
import androidx.compose.material.icons.outlined.Public
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.tabs.TabUi
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.theme.KrunkColors
import org.krunk.browser.ui.theme.LocalKrunkStyle
import org.krunk.ffi.AddressSuggestion
import org.krunk.ffi.SuggestionKind

val AddressPillHeight = 40.dp

/** The pill's layout box: the shield and reload/stop inside it get full 48 dp touch targets. */
val AddressPillTouchHeight = 48.dp

/**
 * The address as a pill: [leading] (the blocker's shield), private mask, lock / "not secure"
 * sign, favicon or globe, the site (or the search placeholder on the new-tab page), [trailing]
 * (reload/stop). Tapping the pill starts editing. [compact] (a phone-width toolbar) leaves the
 * words "Not secure" out so the site's name fits.
 *
 * The pill is drawn 40 dp high inside a 48 dp layout box, so the buttons inside it are 48 dp
 * targets without the pill growing; [hasLeading] tells whether [leading] draws something (the
 * text then starts right after it).
 */
@Composable
fun AddressPill(
    tab: TabUi?,
    placeholder: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    compact: Boolean = true,
    hasLeading: Boolean = false,
    leading: @Composable () -> Unit = {},
    trailing: @Composable () -> Unit = {},
) {
    val page = tab?.page
    val indicator = securityIndicator(
        url = tab?.url ?: "",
        internal = tab?.internalPage != null,
        secure = page?.secure,
        loading = page?.loading == true,
        loadError = page?.loadError != null,
        pending = page?.pendingUrl != null,
    )
    val parts = if (tab == null || tab.isNewTab) null else pillParts(tab.displayUrl)
    val text = parts?.host.orEmpty()
    val editLabel = stringResource(R.string.address_edit)
    val style = LocalKrunkStyle.current
    Box(modifier.height(AddressPillTouchHeight), contentAlignment = Alignment.Center) {
        // The pill itself: what is drawn, and what a tap on the address hits. The row above it
        // only takes the taps on its two buttons.
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .height(AddressPillHeight)
                .clip(style.pillShape)
                .clickable(onClick = onClick)
                .semantics {
                    role = Role.Button
                    contentDescription = editLabel
                },
            shape = style.pillShape,
            color = style.pill,
        ) {}
        // What lies on the field is drawn in the field's colours, not in the toolbar's (a
        // gradient toolbar has white icons; the field under them is light).
        CompositionLocalProvider(LocalContentColor provides style.pillText) {
            PillContent(
                tab, text, parts?.rest.orEmpty(), placeholder, indicator, compact, hasLeading, leading, trailing,
            )
        }
    }
}

/** The row on the address field: shield, lock or site icon, the site's name, reload or stop. */
@Composable
private fun PillContent(
    tab: TabUi?,
    text: String,
    rest: String,
    placeholder: String,
    indicator: SecurityIndicator,
    compact: Boolean,
    hasLeading: Boolean,
    leading: @Composable () -> Unit,
    trailing: @Composable () -> Unit,
) {
    val style = LocalKrunkStyle.current
    run {
        Row(
            Modifier.fillMaxWidth().padding(start = if (hasLeading) 0.dp else 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            leading()
            if (tab?.private == true) {
                Icon(
                    Icons.Outlined.Masks,
                    contentDescription = stringResource(R.string.address_private_tab),
                    modifier = Modifier.size(18.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
                Spacer(Modifier.width(6.dp))
            }
            when (indicator) {
                SecurityIndicator.SECURE -> Icon(
                    Icons.Filled.Lock,
                    contentDescription = stringResource(R.string.address_secure),
                    modifier = Modifier.size(16.dp),
                    tint = style.secure,
                )

                // On a phone the words "Not secure" would take the room the site's name needs
                // (seen on the device: nothing of the host was left). The red sign says it there;
                // the words are added where the toolbar is wide enough for both.
                SecurityIndicator.INSECURE -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        Icons.Outlined.Info,
                        contentDescription = stringResource(R.string.address_not_secure),
                        modifier = Modifier.size(16.dp),
                        tint = MaterialTheme.colorScheme.error,
                    )
                    if (!compact) {
                        Spacer(Modifier.width(4.dp))
                        Text(
                            stringResource(R.string.address_not_secure),
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.error,
                            maxLines = 1,
                        )
                    }
                }

                SecurityIndicator.NONE -> if (text.isNotEmpty() && tab?.internalPage == null) {
                    SiteIcon(tab)
                }
            }
            if (indicator != SecurityIndicator.NONE || (text.isNotEmpty() && tab?.internalPage == null)) {
                Spacer(Modifier.width(8.dp))
            }
            if (text.isEmpty()) {
                Text(
                    text = placeholder,
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            } else {
                // The whole address, like Firefox (0.7.1): the host in the normal colour, the
                // path and the rest after it in grey. The host is laid out first and gets the room
                // it needs; when even it does not fit, its end (the registrable domain) stays:
                // "…bank.example", never "login.ba…". The rest takes what is left, cut at its end.
                // An address reads left to right in every locale.
                val ltr = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Ltr)
                Row(Modifier.weight(1f)) {
                    Text(
                        text = text,
                        style = ltr,
                        color = style.pillText,
                        maxLines = 1,
                        overflow = TextOverflow.StartEllipsis,
                    )
                    if (rest.isNotEmpty()) {
                        Text(
                            text = rest,
                            modifier = Modifier.weight(1f),
                            style = ltr,
                            color = style.pillText.copy(alpha = PILL_REST_ALPHA),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            trailing()
        }
    }
}

/** How strongly the part of the address after the host is drawn (grey on any pill colour). */
private const val PILL_REST_ALPHA = 0.6f

@Composable
private fun SiteIcon(tab: TabUi?) {
    val favicon = tab?.favicon
    if (favicon != null) {
        Image(
            bitmap = favicon.asImageBitmap(),
            contentDescription = stringResource(R.string.address_site_icon),
            modifier = Modifier.size(16.dp).clip(RoundedCornerShape(3.dp)),
        )
    } else {
        Icon(
            Icons.Outlined.Public,
            contentDescription = stringResource(R.string.address_site_icon),
            modifier = Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/**
 * The address bar while editing: a plain text field (the platform's long-press menu gives
 * cut/copy/paste/select all), everything selected at the start so typing replaces the URL,
 * a clear button, Go on the keyboard.
 */
@Composable
fun AddressEditor(vm: BrowserViewModel, placeholder: String, modifier: Modifier = Modifier) {
    val typed by vm.typed.collectAsStateWithLifecycle()
    var value by remember { mutableStateOf(TextFieldValue(typed, TextRange(0, typed.length))) }
    // What the user typed, without a completed tail, and whether the last edit typed forward.
    var lastTyped by remember { mutableStateOf(typed) }
    var mayComplete by remember { mutableStateOf(false) }
    val inline by vm.inline.collectAsStateWithLifecycle()
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(Unit) {
        focus.requestFocus()
        keyboard?.show()
    }
    // A suggestion's "fill" arrow writes into the field from outside (never on plain typing: a
    // round trip through the view model would lag a keystroke behind and drop characters).
    val fill by vm.fill.collectAsStateWithLifecycle()
    LaunchedEffect(fill.sequence) {
        if (fill.appliesTo(value.text)) {
            value = TextFieldValue(fill.text, TextRange(fill.text.length))
            lastTyped = fill.text
            mayComplete = false
        }
    }
    // Enter: on a completion, to the page it stands for; otherwise what is in the field.
    // Inline completion (0.7.4): the core's answer for exactly what was typed, drawn after the
    // text as a highlighted tail. It is never put into the field itself: the keyboard composes
    // words, and text changed under it loses typed letters (seen with Gboard).
    val fillNow = inline
    val full = completedText(lastTyped, fillNow)
    val atEnd = value.selection.collapsed && value.selection.end == value.text.length
    val tail = if (mayComplete && full != null && value.text == lastTyped &&
        atEnd
    ) {
        full.substring(lastTyped.length)
    } else {
        null
    }
    val tailStyle = SpanStyle(background = MaterialTheme.colorScheme.primary.copy(alpha = 0.25f))
    // Enter: on a completion, to the page it stands for; otherwise what is in the field.
    val submit = {
        if (tail != null && fillNow != null) vm.goTo(fillNow.url) else vm.go(value.text)
    }
    Surface(
        modifier = modifier.height(AddressPillHeight),
        shape = LocalKrunkStyle.current.pillShape,
        color = LocalKrunkStyle.current.pill,
        contentColor = LocalKrunkStyle.current.pillText,
    ) {
        Row(Modifier.padding(start = 14.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(
                Icons.Filled.Search,
                contentDescription = null,
                modifier = Modifier.size(18.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.width(8.dp))
            Box(Modifier.weight(1f)) {
                if (value.text.isEmpty()) {
                    Text(
                        placeholder,
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                BasicTextField(
                    value = value,
                    onValueChange = {
                        val textChanged = it.text != value.text
                        value = it
                        if (textChanged) {
                            mayComplete = typedForward(lastTyped, it.text)
                            lastTyped = it.text
                            vm.setTyped(it.text)
                        }
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(focus)
                        // A hardware Enter would first replace the selected completion (typing
                        // into the selection): it goes like the keyboard's Go key instead.
                        .onPreviewKeyEvent { e ->
                            val enter = e.key == Key.Enter || e.key == Key.NumPadEnter
                            if (enter && e.type == KeyEventType.KeyDown) submit()
                            enter
                        },
                    visualTransformation = if (tail !=
                        null
                    ) {
                        CompletionTail(tail, tailStyle)
                    } else {
                        VisualTransformation.None
                    },
                    textStyle = LocalTextStyle.current.merge(MaterialTheme.typography.bodyLarge)
                        .copy(color = LocalKrunkStyle.current.pillText),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Uri,
                        imeAction = ImeAction.Go,
                        autoCorrectEnabled = false,
                    ),
                    keyboardActions = KeyboardActions(onGo = { submit() }),
                )
            }
            if (value.text.isNotEmpty()) {
                IconButton(onClick = {
                    value = TextFieldValue("")
                    lastTyped = ""
                    mayComplete = false
                    vm.setTyped("")
                }) {
                    Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.address_clear))
                }
            }
        }
    }
}

/** Rows under the address bar while editing. Tapping a row goes there; the arrow copies it into the field. */
@Composable
fun SuggestionsPanel(
    suggestions: List<AddressSuggestion>,
    onPick: (AddressSuggestion) -> Unit,
    onFill: (AddressSuggestion) -> Unit,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
        LazyColumn(Modifier.fillMaxWidth()) {
            items(suggestions, key = { it.kind.name + it.url + it.title }) { row ->
                SuggestionRow(row, onPick = { onPick(row) }, onFill = { onFill(row) })
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            }
        }
        // The rest of the panel: tapping it ends editing.
        Box(
            Modifier.fillMaxSize().clickable(onClick = onDismiss),
        )
    }
}

@Composable
private fun SuggestionRow(row: AddressSuggestion, onPick: () -> Unit, onFill: () -> Unit) {
    val isSearch = row.kind == SuggestionKind.SEARCH || row.kind == SuggestionKind.SEARCH_SUGGESTION
    val (icon, label) = when (row.kind) {
        SuggestionKind.SEARCH, SuggestionKind.SEARCH_SUGGESTION ->
            Icons.Filled.Search to stringResource(R.string.suggestion_search)

        SuggestionKind.HISTORY -> Icons.Outlined.History to stringResource(R.string.suggestion_history)

        SuggestionKind.BOOKMARK -> Icons.Filled.Star to stringResource(R.string.suggestion_bookmark)

        SuggestionKind.URL -> Icons.Outlined.Public to null
    }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(onClick = onPick)
            .padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(
            icon,
            contentDescription = label,
            modifier = Modifier.size(20.dp),
            tint = if (row.kind == SuggestionKind.BOOKMARK) {
                KrunkColors.bookmarkStar
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        )
        Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
            Text(
                row.title.ifEmpty { row.url },
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (!isSearch && row.title.isNotEmpty()) {
                Text(
                    addressText(row.url),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        IconButton(onClick = onFill) {
            Icon(
                Icons.Outlined.NorthWest,
                contentDescription = stringResource(R.string.suggestion_fill),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Draws [tail] after the typed text (display only: offsets in the tail map to the text's end). */
private class CompletionTail(private val tail: String, private val style: SpanStyle) : VisualTransformation {
    override fun filter(text: AnnotatedString): TransformedText {
        val shown = buildAnnotatedString {
            append(text)
            withStyle(style) { append(tail) }
        }
        val end = text.length
        val map = object : OffsetMapping {
            override fun originalToTransformed(offset: Int): Int = offset
            override fun transformedToOriginal(offset: Int): Int = offset.coerceAtMost(end)
        }
        return TransformedText(shown, map)
    }
}
