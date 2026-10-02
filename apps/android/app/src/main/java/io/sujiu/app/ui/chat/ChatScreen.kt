package io.sujiu.app.ui.chat

import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.Surface
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.R
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent
import io.sujiu.app.ui.SujiuNavigation
import kotlinx.coroutines.launch

/** Width at which the history rail becomes a permanent sidebar (layout promotion). */
private val ExpandedWidth = 840.dp

/**
 * Chat is the home surface. On a narrow window history lives in a drawer and
 * the context inspector in a sheet; on an expanded window the same panels are
 * promoted to permanent columns. It is the same screen either way.
 */
@Composable
fun ChatScreen(controller: ChatController) {
    val state by controller.state.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val drawerState = rememberDrawerState(DrawerValue.Closed)

    val send: (String) -> Unit = { text ->
        controller.dispatch(ChatIntent.DraftChanged(text))
        controller.dispatch(ChatIntent.SendClicked)
    }

    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val expanded = maxWidth >= ExpandedWidth

        if (!expanded) {
            ModalNavigationDrawer(
                drawerState = drawerState,
                drawerContent = {
                    ModalDrawerSheet {
                        HistoryPanel(
                            controller = controller,
                            onSessionPicked = {
                                scope.launch { drawerState.close() }
                            },
                        )
                    }
                },
            ) {
                ChatCanvas(
                    controller = controller,
                    onOpenHistory = { scope.launch { drawerState.open() } },
                    onSend = send,
                    modifier = Modifier.fillMaxSize(),
                )
            }
        } else {
            Row(modifier = Modifier.fillMaxSize()) {
                Surface(modifier = Modifier.width(300.dp).fillMaxHeight()) {
                    HistoryPanel(controller = controller, onSessionPicked = {})
                }
                ChatCanvas(
                    controller = controller,
                    onOpenHistory = {},
                    onSend = send,
                    modifier = Modifier.weight(1f).fillMaxHeight(),
                )
            }
        }
    }

    // `ContextSheet` is deliberately not opened from here. Its one spec'd entry
    // is the conversation title → this conversation's contents, and this
    // frontend has no contents page yet; `apps/android/TODO.md` records that.
    // Opening it from the top bar would be the second, wrong entry point.
}

@Composable
private fun ChatCanvas(
    controller: ChatController,
    onOpenHistory: (() -> Unit)?,
    onSend: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val state by controller.state.collectAsStateWithLifecycle()
    var showModelSheet by remember { mutableStateOf(false) }

    Column(modifier = modifier) {
        ChatTopBar(
            conversationTitle = state.currentCharacter?.name ?: stringResource(R.string.new_chat),
            modelName = state.currentModel?.name ?: stringResource(R.string.select_model),
            onOpenHistory = onOpenHistory,
            onOpenModel = { showModelSheet = true },
            onOpenLibrary = { SujiuNavigation.openLibrary(controller) },
            onOpenSettings = { SujiuNavigation.openSettings(controller) },
        )
        ConversationList(
            messages = state.messages,
            generationState = state.generationState,
            errorMessage = state.errorMessage,
            onCopy = { controller.copyMessage(it) },
            modifier = Modifier.weight(1f),
        )
        Composer(
            draft = state.draft,
            canSend = state.canSend,
            busy = state.busy,
            onDraftChange = { controller.dispatch(ChatIntent.DraftChanged(it)) },
            onSend = onSend,
            onStop = { controller.dispatch(ChatIntent.StopClicked) },
        )
    }

    if (showModelSheet) {
        ModelSelectorSheet(controller = controller, onDismiss = { showModelSheet = false })
    }
}
