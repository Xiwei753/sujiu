package io.sujiu.app.ui.chat

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.presentation.ChatController

/**
 * On-demand context browser.
 *
 * The list of context sources is a *list*, not a dump: the runtime decides what
 * to surface here, and the model decides what to actually read. Nothing on
 * this screen runs a tool itself.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContextSheet(controller: ChatController, onDismiss: () -> Unit) {
    val state by controller.state.collectAsStateWithLifecycle()

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(bottom = 24.dp)) {
            Text(
                text = "Context",
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                style = MaterialTheme.typography.titleMedium,
            )
            if (state.contextSources.isEmpty()) {
                Text(
                    text = "No context sources in this session yet.",
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 12.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            LazyColumn(modifier = Modifier.heightIn(max = 420.dp)) {
                items(items = state.contextSources, key = { it.id }) { source ->
                    Column(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 10.dp),
                    ) {
                        Text(source.name, style = MaterialTheme.typography.bodyLarge)
                        Text(
                            text = "${source.kind.name} · ${source.recordCount} records",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
    }
}
