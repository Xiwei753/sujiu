package io.sujiu.app.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowRight
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.platform.AppearanceMode
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent
import io.sujiu.app.ui.SujiuNavigation

/**
 * Settings follows the platform settings pattern: grouped lists, inline
 * choices, drill-down for anything larger. It is not a dashboard.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(controller: ChatController) {
    val state by controller.state.collectAsStateWithLifecycle()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    TextButton(onClick = { SujiuNavigation.back(controller) }) { Text("Back") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).verticalScroll(rememberScrollState()),
        ) {
            Text(
                text = "APPEARANCE",
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
                AppearanceMode.entries.forEach { mode ->
                    FilterChip(
                        selected = state.appearanceMode == mode,
                        onClick = { controller.dispatch(ChatIntent.AppearanceSelected(mode)) },
                        label = { Text(mode.name) },
                        modifier = Modifier.padding(end = 8.dp),
                    )
                }
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))

            Text(
                text = "CONVERSATION",
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            ListItem(
                headlineContent = { Text("Model") },
                supportingContent = { Text(state.currentModel?.name ?: "Not selected") },
                trailingContent = {
                    Icon(Icons.Filled.KeyboardArrowRight, contentDescription = null)
                },
                modifier = Modifier.clickable { },
            )
            ListItem(
                headlineContent = { Text("Character") },
                supportingContent = { Text(state.currentCharacter?.name ?: "Not selected") },
                trailingContent = {
                    Icon(Icons.Filled.KeyboardArrowRight, contentDescription = null)
                },
                modifier = Modifier.clickable { },
            )

            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))

            Text(
                text = "ABOUT",
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            ListItem(
                headlineContent = { Text("Platform") },
                supportingContent = { Text(controller.platformSummary()) },
            )
            ListItem(
                headlineContent = { Text("Runtime") },
                supportingContent = { Text(controller.runtimeSummary()) },
            )
        }
    }
}
