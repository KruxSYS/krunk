package org.krunk.browser.ui.permissions

import androidx.compose.runtime.Composable
import org.krunk.browser.ui.BrowserViewModel

/** Sites with a remembered permission answer (placeholder until M4). */
@Composable
fun SitePermissionsScreen(vm: BrowserViewModel) {
    vm.hashCode()
}

/** One site's permissions: Allow, Deny or Ask for each (placeholder until M4). */
@Composable
fun SiteDetailScreen(vm: BrowserViewModel, origin: String) {
    vm.hashCode() + origin.length
}
