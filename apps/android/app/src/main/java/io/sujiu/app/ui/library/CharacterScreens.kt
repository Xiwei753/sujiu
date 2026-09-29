package io.sujiu.app.ui.library

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent
import io.sujiu.app.ui.SujiuNavigation

/**
 * Full character library: management lives here, not in the chat surface.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CharacterLibraryScreen(controller: ChatController) {
    val state by controller.state.collectAsStateWithLifecycle()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Characters") },
                navigationIcon = {
                    TextButton(onClick = { SujiuNavigation.back(controller) }) { Text("Back") }
                },
            )
        },
    ) { padding ->
        LazyColumn(
            modifier = Modifier.padding(padding),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            items(items = state.characters, key = { it.id }) { character ->
                Card(
                    modifier = Modifier.fillMaxWidth().clickable {
                        SujiuNavigation.openCharacter(controller, character.id)
                    },
                ) {
                    Row(
                        modifier = Modifier.padding(16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(Icons.Filled.Person, contentDescription = null)
                        Column(modifier = Modifier.padding(start = 12.dp)) {
                            Text(character.name, style = MaterialTheme.typography.titleSmall)
                            Text(
                                text = character.tagline,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CharacterDetailScreen(controller: ChatController, characterId: String) {
    val state by controller.state.collectAsStateWithLifecycle()
    val character = state.characters.firstOrNull { it.id == characterId }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(character?.name ?: "Character") },
                navigationIcon = {
                    TextButton(onClick = { SujiuNavigation.back(controller) }) { Text("Back") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(character?.tagline.orEmpty(), style = MaterialTheme.typography.bodyLarge)
            if (!character?.tags.isNullOrEmpty()) {
                Text(
                    text = character!!.tags.joinToString(" · "),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            TextButton(
                onClick = { SujiuNavigation.selectCharacterAndReturn(controller, characterId) },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Chat with this character")
            }
        }
    }
}
