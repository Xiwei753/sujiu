package io.sujiu.app.ui.chat

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.getValue
import io.sujiu.app.bridge.SessionSummary
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent

/**
 * History list, used both inside the narrow-screen drawer and as the permanent
 * sidebar on wide screens. The grouping (Today / Yesterday / Earlier) comes
 * from presentation; this composable only renders it.
 */
@Composable
fun HistoryPanel(
    controller: ChatController,
    onSessionPicked: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val state by controller.state.collectAsStateWithLifecycle()

    Column(modifier = modifier.fillMaxWidth().fillMaxHeight()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Chats",
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.titleMedium,
            )
            IconButton(onClick = {
                controller.dispatch(ChatIntent.NewSessionClicked)
                onSessionPicked()
            }) {
                Icon(Icons.Filled.Add, contentDescription = "New chat")
            }
        }
        HorizontalDivider()
        LazyColumn(
            modifier = Modifier.weight(1f),
            contentPadding = PaddingValues(vertical = 8.dp),
        ) {
            state.sessionGroups.forEach { group ->
                item(key = "group-${group.title}") {
                    Text(
                        text = group.title.uppercase(),
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                items(items = group.sessions, key = { it.id }) { session ->
                    SessionRow(
                        session = session,
                        selected = session.id == state.currentSessionId,
                        onClick = {
                            controller.dispatch(ChatIntent.SessionSelected(session.id))
                            onSessionPicked()
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun SessionRow(
    session: SessionSummary,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(
            text = session.title,
            style = if (selected) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text = session.characterName,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}
