package io.sujiu.app.ui.chat

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * Chat top bar: history, character, model, and an overflow for the rest.
 * Buttons only express intents; no data is fetched here.
 */
@Composable
fun ChatTopBar(
    characterName: String,
    modelName: String,
    /** Null on expanded layouts, where history is a permanent column. */
    onOpenHistory: (() -> Unit)?,
    onOpenCharacter: () -> Unit,
    onOpenModel: () -> Unit,
    onOpenContext: () -> Unit,
    onOpenLibrary: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    var overflowOpen by remember { mutableStateOf(false) }

    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        if (onOpenHistory != null) {
            IconButton(onClick = onOpenHistory) {
                Icon(Icons.Filled.Menu, contentDescription = "Conversation history")
            }
        }
        TextButton(onClick = onOpenCharacter) {
            Icon(Icons.Filled.Person, contentDescription = null, modifier = Modifier.padding(end = 6.dp))
            Text(characterName, style = MaterialTheme.typography.titleSmall)
        }
        TextButton(onClick = onOpenModel) { Text(modelName, style = MaterialTheme.typography.labelMedium) }

        Row(modifier = Modifier.weight(1f), horizontalArrangement = Arrangement.End) {
            DropdownMenu(
                expanded = overflowOpen,
                onDismissRequest = { overflowOpen = false },
            ) {
                DropdownMenuItem(
                    text = { Text("Context & tools") },
                    onClick = { overflowOpen = false; onOpenContext() },
                )
                DropdownMenuItem(
                    text = { Text("Character library") },
                    onClick = { overflowOpen = false; onOpenLibrary() },
                )
                DropdownMenuItem(
                    text = { Text("Settings") },
                    onClick = { overflowOpen = false; onOpenSettings() },
                )
            }
            IconButton(onClick = { overflowOpen = true }) {
                Icon(Icons.Filled.MoreVert, contentDescription = "More")
            }
        }
    }
}
