package io.sujiu.app.ui.chat

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.sujiu.app.ui.components.CollapsibleSection
import io.sujiu.app.presentation.ChatMessageItem
import io.sujiu.app.presentation.GenerationState

/**
 * The conversation canvas.
 *
 * Tool calls and thinking are collapsed by default; the busy indicator and
 * the failure message are the only generation feedback the chat surface needs.
 */
@Composable
fun ConversationList(
    messages: List<ChatMessageItem>,
    generationState: GenerationState,
    errorMessage: String?,
    onCopy: (ChatMessageItem) -> Unit,
    modifier: Modifier = Modifier,
) {
    val listState = rememberLazyListState()

    LaunchedEffect(messages.size, generationState) {
        if (messages.isNotEmpty()) {
            listState.animateScrollToItem(messages.lastIndex)
        }
    }

    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        items(items = messages, key = { it.id }) { message ->
            MessageItem(message = message, onCopy = { onCopy(message) })
        }
        if (generationState.busy) {
            item(key = "busy") {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp))
                    Text(
                        text = generationState.statusLabel(),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        if (errorMessage != null) {
            item(key = "error") {
                Surface(
                    color = MaterialTheme.colorScheme.errorContainer,
                    shape = RoundedCornerShape(12.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text = errorMessage,
                        modifier = Modifier.padding(12.dp),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}

private fun GenerationState.statusLabel(): String = when (this) {
    GenerationState.Submitting -> "Sending…"
    GenerationState.Streaming -> "Writing…"
    GenerationState.WaitingForTool -> "Waiting for tool…"
    GenerationState.ExecutingTool -> "Using a tool…"
    GenerationState.ContinuingAfterTool -> "Continuing…"
    else -> ""
}

@Composable
private fun MessageItem(message: ChatMessageItem, onCopy: () -> Unit) {
    when (message) {
        is ChatMessageItem.User -> Column(horizontalAlignment = Alignment.End) {
            Surface(
                color = MaterialTheme.colorScheme.primaryContainer,
                shape = RoundedCornerShape(16.dp),
                modifier = Modifier.fillMaxWidth(0.82f),
            ) {
                Text(
                    text = message.text,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            Text(
                text = "Copy",
                modifier = Modifier.padding(top = 2.dp).clickable { onCopy() },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        is ChatMessageItem.Assistant -> Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            if (message.thinking.isNotEmpty()) {
                CollapsibleSection(title = "Thinking", body = message.thinking.joinToString(""))
            }
            if (message.toolCalls.isNotEmpty()) {
                CollapsibleSection(
                    title = "Tools (${message.toolCalls.size})",
                    body = message.toolCalls.joinToString("\n") { call ->
                        val summary = call.summary?.let { " — $it" } ?: ""
                        "${call.toolName}: ${call.statusLabel}$summary"
                    },
                )
            }
            if (message.text.isNotBlank()) {
                Text(
                    text = message.text,
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            Text(
                text = "Copy",
                modifier = Modifier.clickable { onCopy() },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
