package org.krunk.browser.ui.browser

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material.icons.outlined.Masks
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.krunk.browser.R
import org.krunk.browser.engine.ContextTarget

private const val MAX_SHOWN = 500

/** The link / image long-press menu as a bottom sheet. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContextMenuSheet(
    target: ContextTarget,
    privateTab: Boolean,
    onAction: (ContextAction) -> Unit,
    onDismiss: () -> Unit,
) {
    val actions = contextActions(target, privateTab)
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        // Scrolls: a link around an image has nine rows, more than a landscape phone shows.
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState())) {
            // Bounded: a page can put megabytes into a link's text or a data: address.
            val headline = (target.copyableText ?: "").take(MAX_SHOWN)
            val url = (target.linkUri ?: target.srcUri ?: "").take(MAX_SHOWN)
            Column(Modifier.padding(horizontal = 24.dp, vertical = 4.dp)) {
                if (headline.isNotEmpty()) {
                    Text(
                        headline,
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (url.isNotEmpty()) {
                    Text(
                        url,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            for (action in actions) {
                val (label, icon) = describe(action)
                ListItem(
                    headlineContent = { Text(label) },
                    leadingContent = { Icon(icon, contentDescription = null) },
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    modifier = Modifier.clickable { onAction(action) },
                )
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun describe(action: ContextAction): Pair<String, ImageVector> = when (action) {
    ContextAction.OPEN_NEW_TAB -> stringResource(R.string.ctx_open_new_tab) to Icons.AutoMirrored.Outlined.OpenInNew
    ContextAction.OPEN_PRIVATE_TAB -> stringResource(R.string.ctx_open_private_tab) to Icons.Outlined.Masks
    ContextAction.COPY_LINK -> stringResource(R.string.ctx_copy_link) to Icons.Outlined.Link
    ContextAction.SHARE_LINK -> stringResource(R.string.ctx_share_link) to Icons.Outlined.Share
    ContextAction.COPY_LINK_TEXT -> stringResource(R.string.ctx_copy_link_text) to Icons.Outlined.ContentCopy
    ContextAction.DOWNLOAD_LINK -> stringResource(R.string.ctx_download_link) to Icons.Outlined.Download
    ContextAction.OPEN_IMAGE_NEW_TAB -> stringResource(R.string.ctx_open_image_new_tab) to Icons.Outlined.Image
    ContextAction.SAVE_IMAGE -> stringResource(R.string.ctx_save_image) to Icons.Outlined.Download
    ContextAction.COPY_IMAGE_LINK -> stringResource(R.string.ctx_copy_image_link) to Icons.Outlined.Link
    ContextAction.SAVE_MEDIA -> stringResource(R.string.ctx_save_media) to Icons.Outlined.Movie
}
