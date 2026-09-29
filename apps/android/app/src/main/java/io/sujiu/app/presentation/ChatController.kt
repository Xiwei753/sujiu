package io.sujiu.app.presentation

import io.sujiu.app.bridge.CharacterSummary
import io.sujiu.app.bridge.MessageRole
import io.sujiu.app.bridge.MessageSummary
import io.sujiu.app.bridge.SessionSummary
import io.sujiu.app.bridge.SujiuBridge
import io.sujiu.app.bridge.TurnEvent
import io.sujiu.app.platform.AppearanceMode
import io.sujiu.app.platform.PlatformServices
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.LocalDate
import java.time.ZoneId

/**
 * Presentation controller for the chat surface.
 *
 * It owns page-level state (which session is open, the draft, the generation
 * state, the data behind drawers and sheets) and talks to the application
 * bridge plus platform capability services. It performs no provider, tool or
 * OS work of its own, and it is deliberately a plain class so it can be unit
 * tested without a device.
 */
class ChatController(
    private val scope: CoroutineScope,
    private val bridge: SujiuBridge,
    private val platform: PlatformServices,
    private val navigator: Navigator = Navigator(),
) {
    private val _state = MutableStateFlow(ChatUiState())
    val state: StateFlow<ChatUiState> = _state.asStateFlow()

    val navigation: Navigator get() = navigator

    private var turnJob: Job? = null
    private var assistantDraftIndex: Int? = null
    private var messageCounter = 0

    fun dispatch(intent: ChatIntent) {
        when (intent) {
            is ChatIntent.DraftChanged -> _state.update { it.copy(draft = intent.text) }
            ChatIntent.SendClicked -> submit()
            ChatIntent.StopClicked -> cancel()
            is ChatIntent.SessionSelected -> openSession(intent.sessionId)
            ChatIntent.NewSessionClicked -> startNewSession()
            is ChatIntent.CharacterSelected -> selectCharacter(intent.characterId)
            is ChatIntent.CharacterQueryChanged -> searchCharacters(intent.query)
            is ChatIntent.ModelSelected -> selectModel(intent.modelId)
            is ChatIntent.AppearanceSelected -> setAppearance(intent.mode)
            ChatIntent.AppForegrounded -> refresh()
        }
    }

    /** Initial load plus a cheap refresh when the app comes back to the foreground. */
    fun refresh() {
        scope.launch {
            val sessions = bridge.listSessions()
            val characters = bridge.listCharacters(_state.value.characterQuery)
            val models = bridge.listModels()
            _state.update { current ->
                current.copy(
                    sessionGroups = groupSessions(sessions),
                    characters = characters,
                    models = models,
                )
            }
            val sessionId = _state.value.currentSessionId
            if (sessionId == null) {
                sessions.firstOrNull()?.let { loadConversation(it.id) }
            } else {
                loadContextSources()
            }
        }
    }

    fun openSession(sessionId: String) {
        navigator.popToRoot()
        scope.launch { loadConversation(sessionId) }
    }

    fun startNewSession() {
        navigator.popToRoot()
        turnJob?.cancel()
        assistantDraftIndex = null
        _state.update {
            it.copy(
                currentSessionId = null,
                messages = emptyList(),
                generationState = GenerationState.Idle,
                errorMessage = null,
            )
        }
    }

    fun copyMessage(message: ChatMessageItem) {
        val text = message.text
        if (text.isNotBlank()) {
            platform.clipboard.copyText("Sujiu", text)
        }
    }

    fun platformSummary(): String = platform.info.platformSummary()

    fun runtimeSummary(): String = bridge.runtimeSummary()

    fun characterById(id: String): CharacterSummary? =
        _state.value.characters.firstOrNull { it.id == id }

    fun sessionById(id: String): SessionSummary? =
        _state.value.sessionGroups.asSequence()
            .flatMap { it.sessions.asSequence() }
            .firstOrNull { it.id == id }

    private suspend fun loadConversation(sessionId: String) {
        val snapshot = bridge.conversationState(sessionId)
        val models = if (_state.value.models.isEmpty()) bridge.listModels() else _state.value.models
        val characters =
            if (_state.value.characters.isEmpty()) bridge.listCharacters() else _state.value.characters
        messageCounter = 0
        _state.update {
            it.copy(
                currentSessionId = snapshot.session.id,
                messages = snapshot.messages.map { it.toItem() },
                contextSources = snapshot.contextSources,
                currentCharacter = characters.firstOrNull { c -> c.id == snapshot.session.characterId },
                currentModel = models.firstOrNull(),
                generationState = GenerationState.Idle,
                errorMessage = null,
            )
        }
    }

    private suspend fun loadContextSources() {
        _state.update { it.copy(contextSources = bridge.listContextSources()) }
    }

    private fun selectCharacter(characterId: String) {
        _state.update { it.copy(currentCharacter = characterById(characterId)) }
    }

    private fun searchCharacters(query: String) {
        _state.update { it.copy(characterQuery = query) }
        scope.launch {
            val characters = bridge.listCharacters(query)
            _state.update { it.copy(characters = characters) }
        }
    }

    private fun selectModel(modelId: String) {
        _state.update { it.copy(currentModel = it.models.firstOrNull { m -> m.id == modelId }) }
    }

    private fun setAppearance(mode: AppearanceMode) {
        _state.update { it.copy(appearanceMode = mode) }
    }

    private fun submit() {
        val current = _state.value
        val text = current.draft.trim()
        if (text.isEmpty() || current.busy) return
        val sessionId = current.currentSessionId ?: return

        assistantDraftIndex = null
        _state.update {
            it.copy(
                draft = "",
                errorMessage = null,
                generationState = GenerationState.Submitting,
                messages = it.messages + ChatMessageItem.User(nextId("user"), text),
            )
        }

        turnJob = scope.launch {
            bridge.sendTurn(sessionId, text).collect { event -> applyTurnEvent(event) }
        }
    }

    private fun cancel() {
        val sessionId = _state.value.currentSessionId ?: return
        bridge.cancelTurn(sessionId)
        turnJob?.cancel()
        turnJob = null
        assistantDraftIndex = null
        _state.update { it.copy(generationState = GenerationState.Cancelled) }
    }

    private fun applyTurnEvent(event: TurnEvent) {
        when (event) {
            TurnEvent.Started -> ensureAssistant()

            is TurnEvent.TextDelta -> {
                ensureAssistant()
                _state.update { it.copy(generationState = GenerationState.Streaming) }
                appendToAssistant { assistant -> assistant.copy(text = assistant.text + event.text) }
            }

            is TurnEvent.ThinkingDelta -> {
                ensureAssistant()
                appendToAssistant { assistant ->
                    assistant.copy(thinking = assistant.thinking + event.text)
                }
            }

            is TurnEvent.ToolCallStarted -> {
                ensureAssistant()
                _state.update { it.copy(generationState = GenerationState.ExecutingTool) }
                appendToAssistant { assistant ->
                    assistant.copy(
                        toolCalls = assistant.toolCalls + ToolCallItem(
                            id = event.callId,
                            toolName = event.toolName,
                            statusLabel = "running",
                        ),
                    )
                }
            }

            is TurnEvent.ToolCallFinished -> {
                _state.update { it.copy(generationState = GenerationState.ContinuingAfterTool) }
                appendToAssistant { assistant ->
                    assistant.copy(
                        toolCalls = assistant.toolCalls.map { call ->
                            if (call.id == event.callId) {
                                call.copy(
                                    statusLabel = if (event.isError) "failed" else "done",
                                    summary = event.summary,
                                    isError = event.isError,
                                )
                            } else {
                                call
                            }
                        },
                    )
                }
            }

            TurnEvent.Completed -> {
                assistantDraftIndex = null
                _state.update { it.copy(generationState = GenerationState.Completed) }
            }

            is TurnEvent.Failed -> {
                assistantDraftIndex = null
                _state.update {
                    it.copy(
                        generationState = GenerationState.Failed,
                        errorMessage = event.message,
                    )
                }
            }

            TurnEvent.Cancelled -> {
                assistantDraftIndex = null
                _state.update { it.copy(generationState = GenerationState.Cancelled) }
            }
        }
    }

    private fun ensureAssistant() {
        if (assistantDraftIndex != null) return
        val index = _state.value.messages.size
        _state.update {
            it.copy(messages = it.messages + ChatMessageItem.Assistant(nextId("assistant"), ""))
        }
        assistantDraftIndex = index
    }

    private fun appendToAssistant(transform: (ChatMessageItem.Assistant) -> ChatMessageItem.Assistant) {
        val index = assistantDraftIndex ?: return
        _state.update { current ->
            val target = current.messages.getOrNull(index) as? ChatMessageItem.Assistant ?: return@update current
            current.copy(messages = current.messages.toMutableList().apply { set(index, transform(target)) })
        }
    }

    private fun nextId(prefix: String): String = "$prefix-${messageCounter++}"

    private fun MessageSummary.toItem(): ChatMessageItem = when (role) {
        MessageRole.User -> ChatMessageItem.User(id, text)
        MessageRole.Assistant -> ChatMessageItem.Assistant(
            id = id,
            text = text,
            toolCalls = toolName?.let {
                listOf(ToolCallItem(id = "tool-$id", toolName = it, statusLabel = "done"))
            } ?: emptyList(),
        )
        MessageRole.System -> ChatMessageItem.Assistant(id, text)
    }

    private fun groupSessions(sessions: List<SessionSummary>): List<SessionGroup> {
        val zone = ZoneId.systemDefault()
        val today = LocalDate.now(zone).atStartOfDay(zone).toInstant().toEpochMilli()
        val yesterday = today - 24 * 60 * 60_000L
        val buckets = linkedMapOf(
            "Today" to mutableListOf<SessionSummary>(),
            "Yesterday" to mutableListOf<SessionSummary>(),
            "Earlier" to mutableListOf<SessionSummary>(),
        )
        sessions.sortedByDescending { it.updatedAt }.forEach { session ->
            val title = when {
                session.updatedAt >= today -> "Today"
                session.updatedAt >= yesterday -> "Yesterday"
                else -> "Earlier"
            }
            buckets.getValue(title).add(session)
        }
        return buckets.filterValues { it.isNotEmpty() }
            .map { (title, items) -> SessionGroup(title, items) }
    }
}
