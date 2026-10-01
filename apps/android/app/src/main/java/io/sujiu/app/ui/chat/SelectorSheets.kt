package io.sujiu.app.ui.chat

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent
import io.sujiu.app.ui.SujiuNavigation

/**
 * Lightweight model picker: recents first, no separate management page.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ModelSelectorSheet(controller: ChatController, onDismiss: () -> Unit) {
    val state by controller.state.collectAsStateWithLifecycle()

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(bottom = 24.dp)) {
            Text(
                text = "Model",
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                style = MaterialTheme.typography.titleMedium,
            )
            LazyColumn(modifier = Modifier.heightIn(max = 420.dp)) {
                items(items = state.models, key = { it.id }) { model ->
                    val selected = model.id == state.currentModel?.id
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                controller.dispatch(ChatIntent.ModelSelected(model.id))
                                onDismiss()
                            }
                            .padding(horizontal = 24.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(model.name, style = MaterialTheme.typography.bodyLarge)
                            Text(
                                text = model.endpointLabel,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        if (selected) {
                            AssistChip(onClick = {}, label = { Text("In use") })
                        }
                    }
                }
            }
        }
    }
}

/**
 * Lightweight character picker with search, plus an entry point into the
 * full character library for management tasks.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CharacterSelectorSheet(
    controller: ChatController,
    onDismiss: () -> Unit,
    onOpenLibrary: () -> Unit,
) {
    val state by controller.state.collectAsStateWithLifecycle()

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(bottom = 24.dp)) {
            OutlinedTextField(
                value = state.characterQuery,
                onValueChange = { controller.dispatch(ChatIntent.CharacterQueryChanged(it)) },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp),
                singleLine = true,
                label = { Text("Search characters") },
            )
            LazyColumn(modifier = Modifier.heightIn(max = 380.dp).padding(top = 8.dp)) {
                items(items = state.characters, key = { it.id }) { character ->
                    CharacterRow(
                        name = character.name,
                        description = character.description,
                        selected = character.id == state.currentCharacter?.id,
                        onClick = {
                            controller.dispatch(ChatIntent.CharacterSelected(character.id))
                            onDismiss()
                        },
                    )
                }
                item {
                    HorizontalDivider()
                    TextButton(
                        onClick = {
                            onDismiss()
                            onOpenLibrary()
                        },
                        modifier = Modifier.padding(horizontal = 16.dp),
                    ) {
                        Text("Open character library")
                    }
                }
            }
        }
    }
}

@Composable
fun CharacterRow(
    name: String,
    description: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 24.dp, vertical = 12.dp),
    ) {
        Text(
            text = name,
            style = if (selected) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyLarge,
        )
        Text(
            text = description,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}
