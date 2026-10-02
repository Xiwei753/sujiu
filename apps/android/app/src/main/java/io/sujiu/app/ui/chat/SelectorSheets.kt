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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent

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
 * A character picker used to live here as well, opened from the conversation
 * title.
 *
 * It was removed rather than moved: `docs/UI_ARCHITECTURE.md` §1.2 makes the
 * title the entry to *this conversation's* contents and bindings, and §1.7 puts
 * character management in the library, where `CharacterLibraryScreen` and
 * `CharacterDetailScreen` already select a character. Keeping a picker bound to
 * the title would keep re-teaching the same wrong idea about what that slot
 * means.
 */
