package io.sujiu.app.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.KeyboardArrowRight
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.R
import io.sujiu.app.platform.AppearanceMode
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent
import io.sujiu.app.ui.SujiuNavigation

/**
 * Settings, first level: a category list and nothing else.
 *
 * The previous version laid the whole surface out at once — appearance chips, a
 * Model row, a Character row, platform and runtime — so the screen read as one
 * long form with no structure. The platform settings idiom, and
 * `docs/UI_ARCHITECTURE.md` §1.9, both say the first level is a list of
 * destinations and that anything larger than one screen gets its own page.
 *
 * Only categories this frontend can actually open are listed. Provider & API and
 * Diagnostics are part of the spec, and the Android bridge exposes no endpoint
 * configuration and no diagnostics log at all, so drawing those rows would be a
 * button that leads nowhere — `apps/android/TODO.md` records them as missing
 * capability rather than this file inventing a page for them.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(controller: ChatController) {
    val state by controller.state.collectAsStateWithLifecycle()
    var page by remember { mutableStateOf(SettingsPage.Categories) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        stringResource(
                            when (page) {
                                SettingsPage.Appearance -> R.string.settings_appearance
                                SettingsPage.About -> R.string.settings_about
                                SettingsPage.Categories -> R.string.settings
                            },
                        ),
                    )
                },
                navigationIcon = {
                    IconButton(onClick = {
                        if (page == SettingsPage.Categories) SujiuNavigation.back(controller)
                        else page = SettingsPage.Categories
                    }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.back),
                        )
                    }
                },
            )
        },
    ) { padding ->
        when (page) {
            SettingsPage.Categories -> SettingsCategoryList(
                modifier = Modifier.padding(padding),
                onOpen = { page = it },
            )
            SettingsPage.Appearance -> AppearancePage(
                selected = state.appearanceMode,
                onSelect = { controller.dispatch(ChatIntent.AppearanceSelected(it)) },
                modifier = Modifier.padding(padding),
            )
            SettingsPage.About -> AboutPage(controller, Modifier.padding(padding))
        }
    }
}

/** The destinations the first level can offer, so the level has no free-form list. */
private enum class SettingsPage { Categories, Appearance, About }

@Composable
private fun SettingsCategoryList(onOpen: (SettingsPage) -> Unit, modifier: Modifier = Modifier) {
    Column(modifier = modifier.verticalScroll(rememberScrollState())) {
        SettingsCategoryRow(R.string.settings_appearance, onOpen = { onOpen(SettingsPage.Appearance) })
        SettingsCategoryRow(R.string.settings_about, onOpen = { onOpen(SettingsPage.About) })
    }
}

@Composable
private fun SettingsCategoryRow(labelRes: Int, onOpen: () -> Unit) {
    ListItem(
        headlineContent = { Text(stringResource(labelRes)) },
        trailingContent = { Icon(Icons.Filled.KeyboardArrowRight, contentDescription = null) },
        modifier = Modifier.clickable(onClick = onOpen),
    )
}

/**
 * Appearance as radio rows rather than a row of chips.
 *
 * A segmented choice of three mutually exclusive options is a radio group on
 * every platform; the chips were only carrying custom colours.
 */
@Composable
private fun AppearancePage(
    selected: AppearanceMode,
    onSelect: (AppearanceMode) -> Unit,
    modifier: Modifier = Modifier,
) {
    val group = "sujiu_appearance"
    Column(modifier = modifier.verticalScroll(rememberScrollState())) {
        AppearanceMode.entries.forEach { mode ->
            ListItem(
                headlineContent = { Text(stringResource(mode.labelRes())) },
                // The row owns the tap, so the control only reports selection.
                leadingContent = {
                    RadioButton(selected = selected == mode, onClick = null)
                },
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onSelect(mode) },
            )
        }
    }
}

/** Radio labels come from resources, so a mode is never rendered as `System`. */
private fun AppearanceMode.labelRes(): Int = when (this) {
    AppearanceMode.System -> R.string.appearance_system
    AppearanceMode.Light -> R.string.appearance_light
    AppearanceMode.Dark -> R.string.appearance_dark
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AboutPage(controller: ChatController, modifier: Modifier = Modifier) {
    Column(modifier = modifier.verticalScroll(rememberScrollState())) {
        ListItem(
            headlineContent = { Text(stringResource(R.string.settings_platform)) },
            supportingContent = { Text(controller.platformSummary()) },
        )
        ListItem(
            headlineContent = { Text(stringResource(R.string.settings_runtime)) },
            supportingContent = { Text(controller.runtimeSummary()) },
        )
    }
}
