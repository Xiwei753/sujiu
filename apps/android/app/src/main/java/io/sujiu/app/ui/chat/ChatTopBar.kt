package io.sujiu.app.ui.chat

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.List
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.sujiu.app.R

/**
 * Chat top bar: exactly five slots, in the order `docs/UI_ARCHITECTURE.md` §1.2
 * fixes.
 *
 * ```text
 * 历史入口 | 会话标题 | 模型            资料库 | 设置
 * ```
 *
 * There is deliberately no overflow menu. It used to hold library, settings and
 * context while the two most-used destinations were hidden behind three dots,
 * and it was the reason the same three destinations existed twice on this
 * surface. The spec does not name a fifth top-level place, so adding one is not
 * something this file gets to decide.
 *
 * Each slot names one intent, and the bar fetches nothing:
 *
 * - history — the drawer, or nothing at all on an expanded layout where history
 *   is a permanent column
 * - conversation title — this conversation's own contents and bindings
 * - model — model selection
 * - library — Character / Persona / WorldBook / PromptProfile
 * - settings — app and service configuration
 *
 * The title takes the flexible width and the model does not: with two flexible
 * labels both truncate, and a long model id squeezed the conversation name down
 * to an ellipsis.
 *
 * `onOpenConversationContents` is a parameter even though this frontend cannot
 * yet honour it, so the call site states the intent instead of the bar quietly
 * reinterpreting the title as a character picker.
 */
@Composable
fun ChatTopBar(
    conversationTitle: String,
    modelName: String,
    /** Null on expanded layouts, where history is a permanent column. */
    onOpenHistory: (() -> Unit)?,
    onOpenModel: () -> Unit,
    onOpenLibrary: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        if (onOpenHistory != null) {
            IconButton(onClick = onOpenHistory) {
                Icon(
                    Icons.Filled.Menu,
                    contentDescription = stringResource(R.string.chat_history),
                )
            }
        }

        Text(
            text = conversationTitle,
            style = MaterialTheme.typography.titleSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )

        TextButton(onClick = onOpenModel) {
            Text(
                text = modelName,
                style = MaterialTheme.typography.labelMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }

        IconButton(onClick = onOpenLibrary) {
            Icon(
                Icons.Filled.List,
                contentDescription = stringResource(R.string.library),
            )
        }

        IconButton(onClick = onOpenSettings) {
            Icon(
                Icons.Filled.Settings,
                contentDescription = stringResource(R.string.settings),
            )
        }
    }
}
