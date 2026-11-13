package org.krunk.browser.ui.settings

import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.roundToInt
import kotlinx.coroutines.launch
import org.krunk.browser.BuildConfig
import org.krunk.browser.R
import org.krunk.browser.core.Wallpapers
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.Screen
import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.LinkTabsMode
import org.krunk.ffi.NewTabMode
import org.krunk.ffi.NtpBackgroundMode
import org.krunk.ffi.SearchEngineId
import org.krunk.ffi.SettingsException
import org.krunk.ffi.StartupMode
import org.krunk.ffi.ThemeMode
import org.krunk.ffi.ToolbarPosition
import org.krunk.ffi.searchEngines
import org.krunk.ffi.themeWithAccent
import org.krunk.ffi.themeWithPreset
import org.krunk.ffi.uaPresets
import org.krunk.ffi.wallpapers

/** The sections of the settings screen (one list on phones, list + detail on tablets). */
enum class SettingsSection(val titleRes: Int) {
    GENERAL(R.string.settings_section_general),
    APPEARANCE(R.string.settings_section_appearance),
    NEW_TAB(R.string.settings_section_newtab),
    PRIVACY(R.string.settings_section_privacy),
    TABS(R.string.settings_section_tabs),
    ADVANCED(R.string.settings_section_advanced),
    ABOUT(R.string.settings_section_about),
}

/**
 * Settings on the shared core fields (validated by the core; a rejected value shows the error and
 * keeps the old one) plus the Android-only ones. Every change applies immediately.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(vm: BrowserViewModel) {
    val settings by vm.settings.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val failed = stringResource(R.string.settings_saved_failed)
    val invalid = stringResource(R.string.settings_invalid)
    val wide = LocalConfiguration.current.screenWidthDp >= 600
    var section by rememberSaveable { mutableStateOf(SettingsSection.GENERAL) }

    val update: ((AndroidSettings) -> AndroidSettings) -> Unit = { transform ->
        scope.launch {
            vm.updateSettings(transform).onFailure { e ->
                val msg = when (e) {
                    is SettingsException.Validation -> e.errors.joinToString("; ") {
                        "${it.field}: ${it.message}"
                    }.ifEmpty { invalid }

                    else -> e.message ?: invalid
                }
                snackbar.showSnackbar(failed.format(msg))
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings_title)) },
                navigationIcon = {
                    IconButton(onClick = { vm.pop() }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.toolbar_back),
                        )
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        if (wide) {
            Row(Modifier.fillMaxSize().padding(padding)) {
                LazyColumn(Modifier.width(280.dp).fillMaxHeight()) {
                    items(SettingsSection.entries) { s ->
                        ListItem(
                            headlineContent = { Text(stringResource(s.titleRes)) },
                            colors = ListItemDefaults.colors(
                                containerColor = if (s ==
                                    section
                                ) {
                                    MaterialTheme.colorScheme.surfaceContainerHigh
                                } else {
                                    Color.Transparent
                                },
                            ),
                            modifier = Modifier.selectable(selected = s == section, role = Role.Tab) {
                                if (s == SettingsSection.ABOUT) vm.push(Screen.About) else section = s
                            },
                        )
                    }
                }
                VerticalDivider(Modifier.fillMaxHeight().padding(vertical = 8.dp))
                LazyColumn(Modifier.weight(1f).fillMaxHeight()) {
                    item { SectionBody(section, vm, settings, update, onAbout = { vm.push(Screen.About) }) }
                }
            }
        } else {
            LazyColumn(Modifier.fillMaxSize().padding(padding)) {
                for (s in SettingsSection.entries) {
                    item(key = s.name) {
                        SectionHeader(stringResource(s.titleRes))
                        SectionBody(s, vm, settings, update, onAbout = { vm.push(Screen.About) })
                    }
                }
                item { Spacer(Modifier.height(24.dp)) }
            }
        }
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 4.dp),
    )
}

@Composable
private fun SectionBody(
    section: SettingsSection,
    vm: BrowserViewModel,
    s: AndroidSettings,
    update: ((AndroidSettings) -> AndroidSettings) -> Unit,
    onAbout: () -> Unit,
) {
    when (section) {
        SettingsSection.GENERAL -> GeneralSection(s, update)

        SettingsSection.APPEARANCE -> AppearanceSection(s, update)

        SettingsSection.NEW_TAB -> NewTabSection(s, update)

        SettingsSection.PRIVACY -> PrivacySection(vm, s, update)

        SettingsSection.TABS -> TabsSection(s, update)

        SettingsSection.ADVANCED -> AdvancedSection(s, update)

        SettingsSection.ABOUT -> Row(
            title = stringResource(R.string.settings_section_about),
            summary = stringResource(R.string.settings_about_help),
            onClick = onAbout,
        )
    }
}

// ----- sections -----

@Composable
private fun GeneralSection(s: AndroidSettings, update: ((AndroidSettings) -> AndroidSettings) -> Unit) {
    val engines = remember { searchEngines() }
    var pick by remember { mutableStateOf<String?>(null) }
    val engineName = engines.firstOrNull { it.id == s.searchEngine }?.name ?: ""

    Row(stringResource(R.string.settings_search_engine), engineName) { pick = "engine" }
    if (s.searchEngine == SearchEngineId.CUSTOM) {
        Row(
            stringResource(R.string.settings_custom_search_url),
            s.customSearchUrl.ifEmpty {
                stringResource(R.string.settings_custom_search_hint)
            },
        ) {
            pick =
                "customUrl"
        }
    }
    Row(
        stringResource(R.string.settings_homepage),
        s.homepage.ifEmpty {
            stringResource(R.string.settings_homepage_none)
        },
    ) {
        pick =
            "homepage"
    }
    Row(
        stringResource(R.string.settings_startup),
        when (s.startup) {
            StartupMode.NEW_TAB -> stringResource(R.string.settings_startup_newtab)
            StartupMode.RESTORE_SESSION -> stringResource(R.string.settings_startup_restore)
            StartupMode.HOMEPAGE -> stringResource(R.string.settings_startup_homepage)
        },
    ) { pick = "startup" }
    Row(
        stringResource(R.string.settings_newtab_opens),
        when (s.newTabPage) {
            NewTabMode.KRUNK -> stringResource(R.string.settings_newtab_krunk)
            NewTabMode.HOMEPAGE -> stringResource(R.string.settings_newtab_homepage)
            NewTabMode.BLANK -> stringResource(R.string.settings_newtab_blank)
            NewTabMode.CUSTOM -> stringResource(R.string.settings_newtab_custom)
        },
    ) { pick = "newtab" }
    if (s.newTabPage == NewTabMode.CUSTOM) {
        Row(stringResource(R.string.settings_newtab_url), s.newTabUrl) { pick = "newtabUrl" }
    }
    SwitchRow(
        stringResource(R.string.settings_search_suggestions),
        stringResource(R.string.settings_search_suggestions_help),
        s.searchSuggestions,
    ) { v ->
        update { it.copy(searchSuggestions = v) }
    }

    when (pick) {
        "engine" -> ChoiceDialog(
            title = stringResource(R.string.settings_search_engine),
            options = engines.map { it.id to it.name },
            selected = s.searchEngine,
            // "Custom" is only valid together with its address: without one stored, ask for it
            // first; the address dialog then saves both.
            onPick = { e ->
                if (e == SearchEngineId.CUSTOM && s.customSearchUrl.isBlank()) {
                    pick = "customUrl"
                } else {
                    update { it.copy(searchEngine = e) }
                }
            },
            onDismiss = { if (pick == "engine") pick = null },
        )

        "customUrl" -> TextDialog(
            stringResource(
                R.string.settings_custom_search_url,
            ),
            s.customSearchUrl,
            stringResource(
                R.string.settings_custom_search_hint,
            ),
            stringResource(R.string.settings_custom_search_help),
            { v ->
                update { it.copy(searchEngine = SearchEngineId.CUSTOM, customSearchUrl = v.trim()) }
            },
        ) {
            pick =
                null
        }

        "homepage" -> TextDialog(stringResource(R.string.settings_homepage), s.homepage, "https://", null, { v ->
            update { it.copy(homepage = v) }
        }) {
            pick =
                null
        }

        "startup" -> ChoiceDialog(
            title = stringResource(R.string.settings_startup),
            options = listOf(
                StartupMode.NEW_TAB to stringResource(R.string.settings_startup_newtab),
                StartupMode.RESTORE_SESSION to stringResource(R.string.settings_startup_restore),
                StartupMode.HOMEPAGE to stringResource(R.string.settings_startup_homepage),
            ),
            selected = s.startup,
            onPick = { m -> update { it.copy(startup = m) } },
            onDismiss = { pick = null },
        )

        "newtab" -> ChoiceDialog(
            title = stringResource(R.string.settings_newtab_opens),
            options = listOf(
                NewTabMode.KRUNK to stringResource(R.string.settings_newtab_krunk),
                NewTabMode.HOMEPAGE to stringResource(R.string.settings_newtab_homepage),
                NewTabMode.BLANK to stringResource(R.string.settings_newtab_blank),
                NewTabMode.CUSTOM to stringResource(R.string.settings_newtab_custom),
            ),
            selected = s.newTabPage,
            // As for the search engine: "Custom" needs its address in the same save.
            onPick = { m ->
                if (m == NewTabMode.CUSTOM && s.newTabUrl.isBlank()) {
                    pick = "newtabUrl"
                } else {
                    update { it.copy(newTabPage = m) }
                }
            },
            onDismiss = { if (pick == "newtab") pick = null },
        )

        "newtabUrl" -> TextDialog(stringResource(R.string.settings_newtab_url), s.newTabUrl, "https://", null, { v ->
            update { it.copy(newTabPage = NewTabMode.CUSTOM, newTabUrl = v.trim()) }
        }) {
            pick =
                null
        }
    }
}

@Composable
private fun AppearanceSection(s: AndroidSettings, update: ((AndroidSettings) -> AndroidSettings) -> Unit) {
    var pick by remember { mutableStateOf<String?>(null) }
    Row(
        stringResource(R.string.settings_theme),
        when (s.theme) {
            ThemeMode.SYSTEM -> stringResource(R.string.settings_theme_system)
            ThemeMode.LIGHT -> stringResource(R.string.settings_theme_light)
            ThemeMode.DARK -> stringResource(R.string.settings_theme_dark)
        },
    ) { pick = "theme" }
    Row(stringResource(R.string.settings_style), styleName(s.appearance)) { pick = "style" }
    Row(stringResource(R.string.settings_accent), accentName(s.appearance)) { pick = "accent" }
    Row(
        stringResource(R.string.settings_toolbar_position),
        if (s.toolbarPosition ==
            ToolbarPosition.TOP
        ) {
            stringResource(R.string.settings_toolbar_top)
        } else {
            stringResource(R.string.settings_toolbar_bottom)
        },
    ) { pick = "toolbar" }
    SwitchRow(
        stringResource(R.string.settings_follow_system_font),
        stringResource(R.string.settings_text_size),
        s.followSystemFont,
    ) { v ->
        update { it.copy(followSystemFont = v) }
    }
    if (!s.followSystemFont) {
        SliderRow(
            title = stringResource(R.string.settings_text_scale, (s.textScale * 100).toInt()),
            value = s.textScale.toFloat(),
            range = 0.5f..2f,
            steps = 14,
        ) { v -> update { it.copy(textScale = (v * 20).roundToInt() / 20.0) } }
    }
    SwitchRow(
        stringResource(R.string.settings_force_zoom),
        stringResource(R.string.settings_force_zoom_help),
        s.forceZoom,
    ) { v ->
        update { it.copy(forceZoom = v) }
    }
    when (pick) {
        "theme" -> ChoiceDialog(
            title = stringResource(R.string.settings_theme),
            options = listOf(
                ThemeMode.SYSTEM to stringResource(R.string.settings_theme_system),
                ThemeMode.LIGHT to stringResource(R.string.settings_theme_light),
                ThemeMode.DARK to stringResource(R.string.settings_theme_dark),
            ),
            selected = s.theme,
            onPick = { m -> update { it.copy(theme = m) } },
            onDismiss = { pick = null },
        )

        "style" -> StylePickerDialog(
            current = s.appearance,
            onPick = { id -> update { it.copy(appearance = themeWithPreset(it.appearance, id)) } },
            onDismiss = { pick = null },
        )

        "accent" -> AccentPickerDialog(
            current = s.appearance,
            onPick = { colour -> update { it.copy(appearance = themeWithAccent(it.appearance, colour)) } },
            onDismiss = { pick = null },
        )

        "toolbar" -> ChoiceDialog(
            title = stringResource(R.string.settings_toolbar_position),
            options = listOf(
                ToolbarPosition.TOP to stringResource(R.string.settings_toolbar_top),
                ToolbarPosition.BOTTOM to stringResource(R.string.settings_toolbar_bottom),
            ),
            selected = s.toolbarPosition,
            onPick = { m -> update { it.copy(toolbarPosition = m) } },
            onDismiss = { pick = null },
        )
    }
}

@Composable
private fun NewTabSection(s: AndroidSettings, update: ((AndroidSettings) -> AndroidSettings) -> Unit) {
    var pick by remember { mutableStateOf<String?>(null) }
    val all = remember { wallpapers() }
    Row(
        stringResource(R.string.settings_ntp_background),
        when (s.ntpBackground) {
            NtpBackgroundMode.NONE, NtpBackgroundMode.IMAGE -> stringResource(R.string.settings_ntp_background_none)
            NtpBackgroundMode.COLOR -> stringResource(R.string.settings_ntp_background_color)
            NtpBackgroundMode.WALLPAPER -> stringResource(R.string.settings_ntp_background_wallpaper)
        },
    ) { pick = "bg" }
    if (s.ntpBackground == NtpBackgroundMode.COLOR) {
        Row(
            stringResource(R.string.settings_ntp_color),
            s.ntpBackgroundColor.ifEmpty {
                stringResource(R.string.settings_ntp_color_hint)
            },
        ) {
            pick =
                "color"
        }
    }
    if (s.ntpBackground == NtpBackgroundMode.WALLPAPER) {
        Row(stringResource(R.string.settings_ntp_wallpaper), all.firstOrNull { it.id == s.ntpWallpaper }?.title ?: "") {
            pick =
                "wallpaper"
        }
        SliderRow(
            stringResource(R.string.settings_ntp_dim, s.ntpImageDim.toInt()),
            s.ntpImageDim.toFloat(),
            0f..80f,
            15,
        ) { v ->
            update { it.copy(ntpImageDim = ((v / 5).roundToInt() * 5).toUByte()) }
        }
    }
    SwitchRow(stringResource(R.string.settings_ntp_show_search), null, s.ntpShowSearch) { v ->
        update { it.copy(ntpShowSearch = v) }
    }
    SwitchRow(stringResource(R.string.settings_ntp_show_tiles), null, s.ntpShowTiles) { v ->
        update { it.copy(ntpShowTiles = v) }
    }
    SwitchRow(stringResource(R.string.settings_ntp_show_shortcuts), null, s.ntpShowShortcuts) { v ->
        update { it.copy(ntpShowShortcuts = v) }
    }
    Row(stringResource(R.string.settings_ntp_grid, s.ntpColumns.toInt(), s.ntpRows.toInt()), null) { pick = "grid" }

    when (pick) {
        "bg" -> ChoiceDialog(
            title = stringResource(R.string.settings_ntp_background),
            options = listOf(
                NtpBackgroundMode.NONE to stringResource(R.string.settings_ntp_background_none),
                NtpBackgroundMode.COLOR to stringResource(R.string.settings_ntp_background_color),
                NtpBackgroundMode.WALLPAPER to stringResource(R.string.settings_ntp_background_wallpaper),
            ),
            selected = if (s.ntpBackground == NtpBackgroundMode.IMAGE) NtpBackgroundMode.NONE else s.ntpBackground,
            onPick = { m ->
                update {
                    it.copy(
                        ntpBackground = m,
                        ntpWallpaper = if (m == NtpBackgroundMode.WALLPAPER &&
                            it.ntpWallpaper.isEmpty()
                        ) {
                            all.firstOrNull()?.id ?: ""
                        } else {
                            it.ntpWallpaper
                        },
                        ntpBackgroundColor = if (m == NtpBackgroundMode.COLOR &&
                            it.ntpBackgroundColor.isEmpty()
                        ) {
                            "#2563eb"
                        } else {
                            it.ntpBackgroundColor
                        },
                    )
                }
            },
            onDismiss = { pick = null },
        )

        "color" -> TextDialog(
            stringResource(
                R.string.settings_ntp_color,
            ),
            s.ntpBackgroundColor,
            stringResource(R.string.settings_ntp_color_hint),
            null,
            { v ->
                update { it.copy(ntpBackgroundColor = v.trim()) }
            },
        ) {
            pick =
                null
        }

        "wallpaper" -> WallpaperDialog(s.ntpWallpaper, { id -> update { it.copy(ntpWallpaper = id) } }) { pick = null }

        "grid" -> GridDialog(s.ntpColumns.toInt(), s.ntpRows.toInt(), { c, r ->
            update { it.copy(ntpColumns = c.toUByte(), ntpRows = r.toUByte()) }
        }) {
            pick =
                null
        }
    }
}

@Composable
private fun PrivacySection(
    vm: BrowserViewModel,
    s: AndroidSettings,
    update: ((AndroidSettings) -> AndroidSettings) -> Unit,
) {
    var clear by remember { mutableStateOf(false) }
    var allowedOpen by remember { mutableStateOf(false) }
    val allowHosts by vm.allowHosts.collectAsStateWithLifecycle()
    val updateState by vm.filterUpdate.collectAsStateWithLifecycle()
    val listsUpdated by vm.listsUpdated.collectAsStateWithLifecycle()

    // What is shown is what is stored, whoever changed it last.
    LaunchedEffect(Unit) { vm.reloadBlocking() }

    // ----- ad and tracker blocking -----
    SwitchRow(
        stringResource(R.string.settings_adblock),
        stringResource(R.string.settings_adblock_help),
        s.adblockEnabled,
    ) { v ->
        update { it.copy(adblockEnabled = v) }
    }
    Row(
        stringResource(R.string.settings_allowed_sites),
        if (allowHosts.isEmpty()) {
            stringResource(R.string.settings_allowed_sites_none)
        } else {
            pluralStringResource(R.plurals.settings_allowed_sites_count, allowHosts.size, allowHosts.size)
        },
    ) {
        allowedOpen = true
    }
    Row(
        stringResource(R.string.settings_update_lists),
        updateLineText(updateLine(updateState, listsUpdated)),
        enabled = updateEnabled(updateState, s.adblockEnabled),
    ) {
        vm.updateFilterLists()
    }
    SwitchRow(
        stringResource(R.string.settings_auto_update),
        stringResource(R.string.settings_auto_update_help),
        s.filterAutoUpdate,
    ) { v ->
        update { it.copy(filterAutoUpdate = v) }
    }
    // The default's price, said plainly (plan decision 1).
    ListItem(
        headlineContent = {},
        supportingContent = { Text(stringResource(R.string.settings_update_tradeoff)) },
    )
    HorizontalDivider()

    SwitchRow(
        stringResource(R.string.settings_https_only),
        stringResource(R.string.settings_https_only_help),
        s.httpsOnly,
    ) { v ->
        update { it.copy(httpsOnly = v) }
    }
    SwitchRow(
        stringResource(R.string.settings_strip_tracking),
        stringResource(R.string.settings_strip_tracking_help),
        s.stripTrackingParams,
    ) { v ->
        update { it.copy(stripTrackingParams = v) }
    }
    Row(stringResource(R.string.settings_clear_data), stringResource(R.string.settings_clear_data_help)) {
        clear = true
    }
    Row(stringResource(R.string.settings_passwords), stringResource(R.string.settings_passwords_help)) {
        vm.push(Screen.Vault)
    }
    if (clear) ClearDataDialog(vm) { clear = false }
    if (allowedOpen) AllowedSitesDialog(allowHosts, onRemove = vm::removeAllowedSite) { allowedOpen = false }
}

/** The line under "Update filter lists": where the lists are from, or how the update goes. */
@Composable
private fun updateLineText(line: UpdateLine): String {
    val format =
        remember { java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.MEDIUM, java.text.DateFormat.SHORT) }
    val fromPackage = stringResource(R.string.settings_update_from_package, BuildConfig.UBO_VERSION)
    return when (line) {
        UpdateLine.FromPackage -> fromPackage

        is UpdateLine.UpdatedAt -> stringResource(R.string.settings_update_last, format.format(java.util.Date(line.at)))

        UpdateLine.Updating -> stringResource(R.string.settings_update_running)

        is UpdateLine.Failed -> stringResource(
            R.string.settings_update_failed,
            line.lastUpdated?.let { stringResource(R.string.settings_update_last, format.format(java.util.Date(it))) }
                ?: fromPackage,
        )
    }
}

/** The sites where blocking is switched off; each can be taken off the list. */
@Composable
private fun AllowedSitesDialog(hosts: List<String>, onRemove: (String) -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.settings_allowed_sites)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                if (hosts.isEmpty()) {
                    Text(stringResource(R.string.settings_allowed_sites_empty))
                }
                for (host in hosts) {
                    ListItem(
                        headlineContent = { Text(host, maxLines = 1) },
                        trailingContent = {
                            TextButton(onClick = { onRemove(host) }) {
                                Text(stringResource(R.string.settings_allowed_sites_remove))
                            }
                        },
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.settings_done)) } },
    )
}

@Composable
private fun TabsSection(s: AndroidSettings, update: ((AndroidSettings) -> AndroidSettings) -> Unit) {
    var pick by remember { mutableStateOf<String?>(null) }
    val never = stringResource(R.string.settings_tab_sleep_off)
    fun minutesLabel(m: Int): String = if (m == 0) {
        never
    } else if (m == 1) {
        "1 min"
    } else {
        "$m min"
    }
    Row(stringResource(R.string.settings_tab_sleep), minutesLabel(s.tabSleepMinutes.toInt())) { pick = "sleep" }
    SwitchRow(
        stringResource(R.string.settings_tab_discard),
        stringResource(R.string.settings_tab_discard_help),
        s.tabDiscardEnabled,
    ) { v ->
        update { it.copy(tabDiscardEnabled = v) }
    }
    if (s.tabDiscardEnabled) {
        Row(stringResource(R.string.settings_tab_discard_after), minutesLabel(s.tabDiscardMinutes.toInt())) {
            pick =
                "discard"
        }
    }
    val linkTabs = listOf(
        LinkTabsMode.AUTO to stringResource(R.string.settings_link_tabs_auto),
        LinkTabsMode.SWITCH to stringResource(R.string.settings_link_tabs_front),
        LinkTabsMode.BACKGROUND to stringResource(R.string.settings_link_tabs_behind),
    )
    Row(stringResource(R.string.settings_link_tabs), linkTabs.first { it.first == s.linkTabs }.second) {
        pick =
            "linkTabs"
    }
    val choices = listOf(0, 1, 5, 15, 30, 60)
    when (pick) {
        "linkTabs" -> ChoiceDialog(
            stringResource(R.string.settings_link_tabs),
            linkTabs,
            s.linkTabs,
            { m -> update { it.copy(linkTabs = m) } },
        ) {
            pick = null
        }

        "sleep" -> ChoiceDialog(
            stringResource(R.string.settings_tab_sleep),
            choices.map {
                it to minutesLabel(it)
            },
            s.tabSleepMinutes.toInt(),
            { m -> update { it.copy(tabSleepMinutes = m.toUInt()) } },
        ) {
            pick =
                null
        }

        "discard" -> ChoiceDialog(
            stringResource(R.string.settings_tab_discard_after),
            choices.drop(1).map {
                it to
                    minutesLabel(it)
            },
            s.tabDiscardMinutes.toInt(),
            { m -> update { it.copy(tabDiscardMinutes = m.toUInt()) } },
        ) {
            pick =
                null
        }
    }
}

@Composable
private fun AdvancedSection(s: AndroidSettings, update: ((AndroidSettings) -> AndroidSettings) -> Unit) {
    var pick by remember { mutableStateOf<String?>(null) }
    val presets = remember { uaPresets() }
    val current = when {
        s.userAgent.isEmpty() -> stringResource(R.string.settings_user_agent_default)

        else -> presets.firstOrNull { it.userAgent == s.userAgent }?.label
            ?: stringResource(R.string.settings_user_agent_custom)
    }
    Row(stringResource(R.string.settings_user_agent), current) { pick = "ua" }
    when (pick) {
        "ua" -> ChoiceDialog(
            title = stringResource(R.string.settings_user_agent),
            options = listOf("" to stringResource(R.string.settings_user_agent_default)) +
                presets.map { it.userAgent to it.label } +
                listOf("custom" to stringResource(R.string.settings_user_agent_custom)),
            selected = if (s.userAgent.isEmpty() ||
                presets.any { it.userAgent == s.userAgent }
            ) {
                s.userAgent
            } else {
                "custom"
            },
            onPick = { v -> if (v == "custom") pick = "uaCustom" else update { it.copy(userAgent = v) } },
            onDismiss = { if (pick == "ua") pick = null },
        )

        "uaCustom" -> TextDialog(
            stringResource(
                R.string.settings_user_agent_custom,
            ),
            s.userAgent,
            stringResource(R.string.settings_user_agent_custom_hint),
            null,
            { v ->
                update { it.copy(userAgent = v) }
            },
        ) {
            pick =
                null
        }
    }
}

// ----- rows and dialogs -----

@Composable
private fun Row(title: String, summary: String?, enabled: Boolean = true, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = summary?.let { { Text(it, maxLines = 3) } },
        modifier = Modifier.clickable(
            enabled = enabled,
            onClick = onClick,
        ).alpha(if (enabled) 1f else DISABLED_ROW_ALPHA),
    )
}

private const val DISABLED_ROW_ALPHA = 0.5f

@Composable
private fun SwitchRow(title: String, summary: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = summary?.let { { Text(it, maxLines = 3) } },
        trailingContent = { Switch(checked = checked, onCheckedChange = null) },
        // A switch is "on" or "off" to a screen reader, not "selected".
        modifier = Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onChange),
    )
}

@Composable
private fun SliderRow(
    title: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    steps: Int,
    onChange: (Float) -> Unit,
) {
    var v by remember(value) { mutableStateOf(value) }
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
        Text(title, style = MaterialTheme.typography.bodyLarge)
        Slider(value = v, onValueChange = {
            v = it
        }, onValueChangeFinished = { onChange(v) }, valueRange = range, steps = steps)
    }
}

@Composable
private fun <T> ChoiceDialog(
    title: String,
    options: List<Pair<T, String>>,
    selected: T,
    onPick: (T) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            // Scrolls: the user-agent list has twelve rows, more than a landscape phone shows.
            Column(Modifier.verticalScroll(rememberScrollState())) {
                for ((value, label) in options) {
                    androidx.compose.foundation.layout.Row(
                        Modifier
                            .fillMaxWidth()
                            .selectable(selected = value == selected, role = Role.RadioButton) {
                                onPick(value)
                                onDismiss()
                            }
                            .padding(vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = value == selected, onClick = null)
                        Spacer(Modifier.width(12.dp))
                        Text(label, style = MaterialTheme.typography.bodyLarge)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}

@Composable
private fun TextDialog(
    title: String,
    initial: String,
    hint: String,
    help: String?,
    onSave: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var text by remember { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column {
                OutlinedTextField(value = text, onValueChange = {
                    text = it
                }, singleLine = true, placeholder = { Text(hint) }, modifier = Modifier.fillMaxWidth())
                if (help !=
                    null
                ) {
                    Text(help, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 8.dp))
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onSave(text)
                onDismiss()
            }) { Text(stringResource(android.R.string.ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}

@Composable
private fun GridDialog(columns: Int, rows: Int, onSave: (Int, Int) -> Unit, onDismiss: () -> Unit) {
    var c by remember { mutableStateOf(columns.toFloat()) }
    var r by remember { mutableStateOf(rows.toFloat()) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.settings_ntp_grid, c.toInt(), r.toInt())) },
        text = {
            Column {
                Text(stringResource(R.string.settings_ntp_columns))
                Slider(value = c, onValueChange = { c = it }, valueRange = 1f..12f, steps = 10)
                Text(stringResource(R.string.settings_ntp_rows))
                Slider(value = r, onValueChange = { r = it }, valueRange = 1f..6f, steps = 4)
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onSave(c.toInt(), r.toInt())
                onDismiss()
            }) { Text(stringResource(android.R.string.ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}

@Composable
private fun WallpaperDialog(selected: String, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val all = remember { wallpapers() }
    val assets = LocalContext.current.assets
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.settings_ntp_wallpaper)) },
        text = {
            LazyVerticalGrid(
                columns = GridCells.Fixed(2),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(all, key = { it.id }) { w ->
                    val thumb by produceState<android.graphics.Bitmap?>(null, w.id) {
                        value =
                            Wallpapers.loadThumb(assets, w.id)
                    }
                    Column(
                        Modifier
                            .clip(RoundedCornerShape(8.dp))
                            .selectable(selected = w.id == selected, role = Role.RadioButton) {
                                onPick(w.id)
                                onDismiss()
                            },
                    ) {
                        Box(Modifier.fillMaxWidth().height(72.dp).clip(RoundedCornerShape(8.dp))) {
                            thumb?.let {
                                Image(
                                    it.asImageBitmap(),
                                    contentDescription = w.title,
                                    contentScale = ContentScale.Crop,
                                    modifier = Modifier.fillMaxSize(),
                                )
                            }
                        }
                        Text(
                            w.title,
                            style = MaterialTheme.typography.labelMedium,
                            color = if (w.id ==
                                selected
                            ) {
                                MaterialTheme.colorScheme.primary
                            } else {
                                MaterialTheme.colorScheme.onSurface
                            },
                            modifier = Modifier.padding(4.dp),
                        )
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}

@Composable
private fun ClearDataDialog(vm: BrowserViewModel, onDismiss: () -> Unit) {
    var history by remember { mutableStateOf(true) }
    var cookies by remember { mutableStateOf(true) }
    var cache by remember { mutableStateOf(true) }
    var siteData by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.settings_clear_data)) },
        text = {
            Column {
                CheckRow(stringResource(R.string.settings_clear_history), history) { history = it }
                CheckRow(stringResource(R.string.settings_clear_cookies), cookies) { cookies = it }
                CheckRow(stringResource(R.string.settings_clear_cache), cache) { cache = it }
                CheckRow(stringResource(R.string.settings_clear_site_data), siteData) { siteData = it }
                if (siteData) {
                    Text(
                        stringResource(R.string.settings_clear_site_data_help),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                // Runs in the view model: it must finish although this dialog is gone.
                vm.clearBrowsingDataAndReport(history, cookies, cache, siteData)
                onDismiss()
            }) { Text(stringResource(R.string.settings_clear_data)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}

@Composable
private fun CheckRow(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    androidx.compose.foundation.layout.Row(
        Modifier.fillMaxWidth().toggleable(value = checked, role = Role.Checkbox, onValueChange = onChange)
            .padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = checked, onCheckedChange = null)
        Spacer(Modifier.width(12.dp))
        Text(label)
    }
}
