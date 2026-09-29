package io.sujiu.app.presentation

import io.sujiu.app.bridge.CharacterSummary
import io.sujiu.app.bridge.ContextSourceSummary
import io.sujiu.app.bridge.ModelSummary
import io.sujiu.app.bridge.SessionSummary
import io.sujiu.app.platform.AppearanceMode

/**
 * The turn lifecycle a chat surface needs to render.
 *
 * These are Sujiu states, not provider states: the bridge normalizes every
 * provider event into them. The full set is declared even when a particular
 * backend does not reach every branch (for example [WaitingForTool] is used
 * by runtimes that require confirmation before executing a tool).
 */
enum class GenerationState {
    Idle,
    Submitting,
    Streaming,
    WaitingForTool,
    ExecutingTool,
    ContinuingAfterTool,
    Completed,
    Failed,
    Cancelled,
    ;

    val busy: Boolean
        get() = this == Submitting || this == Streaming || this == WaitingForTool ||
            this == ExecutingTool || this == ContinuingAfterTool
}

data class ToolCallItem(
    val id: String,
    val toolName: String,
    val statusLabel: String,
    val summary: String? = null,
    val isError: Boolean = false,
)

sealed interface ChatMessageItem {
    val id: String
    val text: String

    data class User(override val id: String, override val text: String) : ChatMessageItem

    data class Assistant(
        override val id: String,
        override val text: String,
        val thinking: List<String> = emptyList(),
        val toolCalls: List<ToolCallItem> = emptyList(),
    ) : ChatMessageItem
}

data class SessionGroup(
    val title: String,
    val sessions: List<SessionSummary>,
)

data class ChatUiState(
    val sessionGroups: List<SessionGroup> = emptyList(),
    val messages: List<ChatMessageItem> = emptyList(),
    val characters: List<CharacterSummary> = emptyList(),
    val characterQuery: String = "",
    val models: List<ModelSummary> = emptyList(),
    val contextSources: List<ContextSourceSummary> = emptyList(),
    val currentSessionId: String? = null,
    val currentCharacter: CharacterSummary? = null,
    val currentModel: ModelSummary? = null,
    val draft: String = "",
    val generationState: GenerationState = GenerationState.Idle,
    val errorMessage: String? = null,
    val appearanceMode: AppearanceMode = AppearanceMode.System,
) {
    val busy: Boolean get() = generationState.busy
    val canSend: Boolean get() = !busy && draft.isNotBlank()
}

sealed interface ChatIntent {
    data class DraftChanged(val text: String) : ChatIntent
    data object SendClicked : ChatIntent
    data object StopClicked : ChatIntent
    data class SessionSelected(val sessionId: String) : ChatIntent
    data object NewSessionClicked : ChatIntent
    data class CharacterSelected(val characterId: String) : ChatIntent
    data class CharacterQueryChanged(val query: String) : ChatIntent
    data class ModelSelected(val modelId: String) : ChatIntent
    data class AppearanceSelected(val mode: AppearanceMode) : ChatIntent
    data object AppForegrounded : ChatIntent
}
