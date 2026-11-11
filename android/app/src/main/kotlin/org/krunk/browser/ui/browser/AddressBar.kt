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
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.tabs.TabUi
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.theme.KrunkColors
import org.krunk.ffi.AddressSuggestion
import org.krunk.ffi.SuggestionKind

val AddressPillHeight = 40.dp
private val PillShape = RoundedCornerShape(20.dp)

/**
 * The address as a pill: private mask, lock / "Not secure", favicon or globe, the URL (or the
 * search placeholder on the new-tab page). Tapping starts editing. [trailing] is the
 * reload/stop button.
 */
@Composable
fun AddressPill(
    tab: TabUi?,
    placeholder: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
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
    val text = if (tab == null || tab.isNewTab) "" else pillText(tab.displayUrl)
    val editLabel = stringResource(R.string.address_edit)
    Surface(
        modifier = modifier
            .height(AddressPillHeight)
            .clip(PillShape)
            .clickable(onClick = onClick)
            .semantics {
                role = Role.Button
                contentDescription = editLabel
            },
        shape = PillShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Row(Modifier.padding(start = 12.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
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
                    tint = KrunkColors.secure,
                )

                SecurityIndicator.INSECURE -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        Icons.Outlined.Info,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                        tint = MaterialTheme.colorScheme.error,
                    )
                    Spacer(Modifier.width(4.dp))
                    Text(
                        stringResource(R.string.address_not_secure),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.error,
                        maxLines = 1,
                    )
                }

                SecurityIndicator.NONE -> if (text.isNotEmpty() && tab?.internalPage == null) {
                    SiteIcon(tab)
                }
            }
            if (indicator != SecurityIndicator.NONE || (text.isNotEmpty() && tab?.internalPage == null)) {
                Spacer(Modifier.width(8.dp))
            }
            Text(
                text = text.ifEmpty { placeholder },
                modifier = Modifier.weight(1f),
                // An address reads left to right in every locale, and when it does not fit its end
                // (the registrable domain) stays: "…bank.example", never "login.ba…".
                style = MaterialTheme.typography.bodyLarge.copy(
                    textDirection = if (text.isEmpty()) TextDirection.Content else TextDirection.Ltr,
                ),
                color = if (text.isEmpty()) {
                    MaterialTheme.colorScheme.onSurfaceVariant
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
                maxLines = 1,
                overflow = if (text.isEmpty()) TextOverflow.Ellipsis else TextOverflow.StartEllipsis,
            )
            trailing()
        }
    }
}

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
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(Unit) {
        focus.requestFocus()
        keyboard?.show()
    }
    // A suggestion's "fill" arrow writes into the field from outside (never on plain typing: a
    // round trip through the view model would lag a keystroke behind and drop characters).
    val fill by vm.fill.collectAsStateWithLifecycle()
    LaunchedEffect(fill.first) {
        if (fill.first > 0 &&
            fill.second != value.text
        ) {
            value = TextFieldValue(fill.second, TextRange(fill.second.length))
        }
    }
    Surface(
        modifier = modifier.height(AddressPillHeight),
        shape = PillShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
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
                        value = it
                        vm.setTyped(it.text)
                    },
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                    textStyle = LocalTextStyle.current.merge(MaterialTheme.typography.bodyLarge)
                        .copy(color = MaterialTheme.colorScheme.onSurface),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Uri,
                        imeAction = ImeAction.Go,
                        autoCorrectEnabled = false,
                    ),
                    keyboardActions = KeyboardActions(onGo = { vm.go(value.text) }),
                )
            }
            if (value.text.isNotEmpty()) {
                IconButton(onClick = {
                    value = TextFieldValue("")
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
