package io.sujiu.app.presentation

import io.sujiu.app.bridge.SujiuBridge
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

    fun characterById(id: String): CharacterRow? =
        _state.value.characters.firstOrNull { it.id == id }

    fun sessionById(id: String): SessionRow? =
        _state.value.sessionGroups.asSequence()
            .flatMap { it.sessions.asSequence() }
            .firstOrNull { it.id == id }

    private suspend fun loadConversation(conversationId: String) {
        val conversation = bridge.conversationState(conversationId)
        val models = if (_state.value.models.isEmpty()) bridge.listModels() else _state.value.models
        val characters =
            if (_state.value.characters.isEmpty()) bridge.listCharacters() else _state.value.characters
        // A participant is who is in the conversation. The character list is
        // where the description lives, so a participant found in both uses the
        // richer row. A conversation with three participants has no primary one;
        // the composer is not where that is decided, so this stays the first and
        // the participant list is still there to read.
        val lead = conversation.participants.firstOrNull()
        messageCounter = 0
        _state.update {
            it.copy(
                currentSessionId = conversation.conversationId,
                messages = conversation.messages.map { it.toItem() },
                currentCharacter = lead?.let { participant ->
                    characters.firstOrNull { it.id == participant.id } ?: participant
                },
                currentModel = models.firstOrNull(),
                generationState = GenerationState.Idle,
                errorMessage = null,
            )
        }
        loadContextSources()
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

    private fun applyTurnEvent(step: TurnStep) {
        when (step) {
            TurnStep.Started -> ensureAssistant()

            is TurnStep.TextDelta -> {
                ensureAssistant()
                _state.update { it.copy(generationState = GenerationState.Streaming) }
                appendToAssistant { assistant -> assistant.copy(text = assistant.text + step.text) }
            }

            is TurnStep.ThinkingDelta -> {
                ensureAssistant()
                appendToAssistant { assistant ->
                    assistant.copy(thinking = assistant.thinking + step.text)
                }
            }

            is TurnStep.ToolCallStarted -> {
                ensureAssistant()
                _state.update { it.copy(generationState = GenerationState.ExecutingTool) }
                appendToAssistant { assistant ->
                    assistant.copy(
                        toolCalls = assistant.toolCalls + ToolCallItem(
                            id = step.callId,
                            toolName = step.toolName,
                            statusLabel = "running",
                        ),
                    )
                }
            }

            is TurnStep.ToolCallFinished -> {
                _state.update { it.copy(generationState = GenerationState.ContinuingAfterTool) }
                appendToAssistant { assistant ->
                    assistant.copy(
                        toolCalls = assistant.toolCalls.map { call ->
                            if (call.id == step.callId) {
                                call.copy(
                                    statusLabel = if (step.isError) "failed" else "done",
                                    summary = step.summary,
                                    isError = step.isError,
                                )
                            } else {
                                call
                            }
                        },
                    )
                }
            }

            TurnStep.Completed -> {
                assistantDraftIndex = null
                _state.update { it.copy(generationState = GenerationState.Completed) }
            }

            is TurnStep.Failed -> {
                assistantDraftIndex = null
                _state.update {
                    it.copy(
                        generationState = GenerationState.Failed,
                        errorMessage = step.message,
                    )
                }
            }

            TurnStep.Cancelled -> {
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

    /**
     * A stored step becomes a message item.
     *
     * A fixed instruction to the model renders like other assistant text
     * because that is all it is on screen: it is context, not a turn. Folding
     * it here loses nothing, because the transcript keeps every step and only
     * this decides what the list draws.
     */
    private fun MessageRow.toItem(): ChatMessageItem = when (role) {
        SpeakerRole.User -> ChatMessageItem.User(id, text)
        // A MessageRow already carries ToolCallItems, so this is a pass-through.
        // Converting it again here would be a second mapping of the same shape.
        SpeakerRole.Assistant -> ChatMessageItem.Assistant(
            id = id,
            text = text,
            toolCalls = toolCalls,
        )
        SpeakerRole.System, SpeakerRole.Developer -> ChatMessageItem.Assistant(id, text)
    }

    private fun groupSessions(sessions: List<SessionRow>): List<SessionGroup> {
        val zone = ZoneId.systemDefault()
        val today = LocalDate.now(zone).atStartOfDay(zone).toInstant().toEpochMilli()
        val yesterday = today - 24 * 60 * 60_000L
        val buckets = linkedMapOf(
            "Today" to mutableListOf<SessionRow>(),
            "Yesterday" to mutableListOf<SessionRow>(),
            "Earlier" to mutableListOf<SessionRow>(),
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
