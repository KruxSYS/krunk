package org.krunk.browser.ui

import android.app.Application
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.KrunkApp
import org.krunk.browser.tabs.DebugProps
import org.krunk.browser.ui.bookmarks.BookmarksScreen
import org.krunk.browser.ui.browser.BrowserScreen
import org.krunk.browser.ui.browser.FullScreenEffect
import org.krunk.browser.ui.browser.LocalLoginUi
import org.krunk.browser.ui.browser.LoginUiHost
import org.krunk.browser.ui.browser.SecureWindowEffect
import org.krunk.browser.ui.browser.SystemBarsEffect
import org.krunk.browser.ui.downloads.DownloadsHost
import org.krunk.browser.ui.downloads.DownloadsScreen
import org.krunk.browser.ui.history.HistoryScreen
import org.krunk.browser.ui.permissions.SiteDetailScreen
import org.krunk.browser.ui.permissions.SitePermissionsScreen
import org.krunk.browser.ui.settings.AboutScreen
import org.krunk.browser.ui.settings.CertExceptionsScreen
import org.krunk.browser.ui.settings.SettingsScreen
import org.krunk.browser.ui.settings.VpnScreen
import org.krunk.browser.ui.sitedata.SiteDataScreen
import org.krunk.browser.ui.sync.SyncScreen
import org.krunk.browser.ui.tabs.TabSwitcherScreen
import org.krunk.browser.ui.theme.KrunkTheme
import org.krunk.browser.ui.theme.LocalKrunkDark
import org.krunk.browser.ui.tv.TvScreen
import org.krunk.browser.ui.vault.SecureVaultSurface
import org.krunk.browser.ui.vault.UnlockVaultDialog
import org.krunk.browser.ui.vault.VaultScreen
import org.krunk.browser.ui.vault.VaultSecureFlag
import org.krunk.browser.ui.vault.vaultPort

/** The app: theme, the screen stack and back handling. */
@Composable
fun KrunkRoot(vm: BrowserViewModel) {
    val settings by vm.settings.collectAsStateWithLifecycle()
    val activeTab by vm.activeTab.collectAsStateWithLifecycle()
    val screens by vm.screens.collectAsStateWithLifecycle()
    val top = screens.last()
    val hasPrivateTabs by vm.hasPrivateTabs.collectAsStateWithLifecycle()

    // Private pages stay out of the Recents thumbnail, screenshots and screen recordings.
    val showPrivate = remember { DebugProps.flag(DebugProps.SHOW_PRIVATE) } // debug builds only
    val privateOnScreen = activeTab?.private == true || (top == Screen.TabSwitcher && hasPrivateTabs)
    // So does everything of the vault: its screen and each unlock, picker and save dialog (they
    // count themselves in VaultSecureFlag). The debug switch above is for private pages only.
    val vaultOnScreen by VaultSecureFlag.secure.collectAsStateWithLifecycle()
    SecureWindowEffect((!showPrivate && privateOnScreen) || top == Screen.Vault || vaultOnScreen)

    // Saved logins in pages: the fill picker, the save offer and the HTTP sign-in dialog reach the
    // vault, its unlock dialog and the vault screen through this.
    val krunkApp = LocalContext.current.applicationContext as? KrunkApp
    val loginUi = remember(krunkApp) {
        krunkApp?.let { app ->
            val port = vaultPort(app)
            LoginUiHost(
                vault = app.loginVault,
                now = System::currentTimeMillis,
                openVault = { vm.push(Screen.Vault) },
                keepForNewVault = { origin, secret ->
                    app.engine.logins.keepForNewVault(origin, secret, System.currentTimeMillis())
                },
                unlockDialog = { onUnlocked, onDismiss -> UnlockVaultDialog(port, null, onUnlocked, onDismiss) },
                secure = { SecureVaultSurface() },
            )
        }
    }

    // Full screen belongs to the page on screen: ends for a covered or background tab, keeps
    // the screen on, follows the page's orientation lock.
    FullScreenEffect(vm, browserOnTop = top == Screen.Browser)

    CompositionLocalProvider(LocalLoginUi provides loginUi) {
        KrunkTheme(
            mode = settings.theme,
            privateMode = activeTab?.private == true && top == Screen.Browser,
            appearance = settings.appearance,
        ) {
            ScreenHost(screens, key = { it.stateKey }, onPop = { vm.pop() }) { screen ->
                // The browser screen sets the system bar icons for its own (possibly private) theme;
                // every other screen needs them set for the theme it is drawn in.
                if (screen != Screen.Browser) SystemBarsEffect(fullScreen = false, darkTheme = LocalKrunkDark.current)
                // On TV every screen but the browser keeps inside the overscan safe area, and its
                // first element takes the focus (the remote needs one to start from).
                TvScreen(enabled = screen != Screen.Browser) {
                    KrunkScreen(vm, screen)
                }
            }
            // Downloads ask and report over whatever screen is up. Outside ScreenHost: its content
            // runs for the covered browser and the screen on top, this must run once.
            DownloadsHost(vm.downloads)
        }
    }
}

@Composable
private fun KrunkScreen(vm: BrowserViewModel, screen: Screen) {
    when (screen) {
        Screen.Browser -> BrowserScreen(vm)

        Screen.TabSwitcher -> TabSwitcherScreen(vm)

        Screen.Bookmarks -> BookmarksScreen(vm)

        Screen.History -> HistoryScreen(vm)

        Screen.Settings -> SettingsScreen(vm)

        Screen.About -> AboutScreen(vm)

        Screen.Downloads -> DownloadsScreen(vm)

        Screen.SitePermissions -> SitePermissionsScreen(vm)

        is Screen.SiteDetail -> SiteDetailScreen(vm, screen.origin)

        Screen.SiteData -> SiteDataScreen(vm)

        Screen.CertExceptions -> CertExceptionsScreen(vm)

        Screen.Sync -> SyncScreen(vm)

        Screen.Vpn -> VpnScreen(vm)

        Screen.Vault -> {
            val app = LocalContext.current.applicationContext as? Application
            if (app != null) VaultScreen(vm, vaultPort(app))
        }
    }
}
