package io.sujiu.app.bridge

import kotlinx.coroutines.flow.Flow

/**
 * Data handed across the application bridge.
 *
 * These types are provider neutral on purpose: the platform never sees a
 * provider response block, a tool call envelope or an HTTP detail. A future
 * `sujiu-ffi` implementation only has to translate its payload into these
 * shapes; no UI or presentation change is required afterwards.
 */
data class SessionSummary(
    val id: String,
    val title: String,
    val characterId: String,
    val characterName: String,
    val preview: String,
    val updatedAt: Long,
)

data class CharacterSummary(
    val id: String,
    val name: String,
    val tagline: String,
    val tags: List<String>,
)

data class ModelSummary(
    val id: String,
    val name: String,
    val providerLabel: String,
    val contextLabel: String,
)

data class ContextSourceSummary(
    val id: String,
    val name: String,
    val kindLabel: String,
    val recordCount: Int,
)

enum class MessageRole { User, Assistant, System }

data class MessageSummary(
    val id: String,
    val role: MessageRole,
    val text: String,
    val toolName: String? = null,
    val toolStatusLabel: String? = null,
)

data class ConversationSnapshot(
    val session: SessionSummary,
    val messages: List<MessageSummary>,
    val contextSources: List<ContextSourceSummary>,
)

/**
 * Normalized turn events.
 *
 * Provider specific streaming events are mapped onto this set by the bridge
 * implementation, so presentation and UI stay identical across providers.
 */
sealed interface TurnEvent {
    data object Started : TurnEvent

    data class TextDelta(val text: String) : TurnEvent

    data class ThinkingDelta(val text: String) : TurnEvent

    data class ToolCallStarted(val callId: String, val toolName: String) : TurnEvent

    data class ToolCallFinished(
        val callId: String,
        val toolName: String,
        val summary: String,
        val isError: Boolean,
    ) : TurnEvent

    data object Completed : TurnEvent

    data class Failed(val message: String) : TurnEvent

    data object Cancelled : TurnEvent
}

/**
 * The only application facing surface of the shared Rust runtime.
 *
 * The FFI contract is expected to grow these operations; until it does, the
 * in-memory preview bridge is used so the whole frontend stack stays
 * exercisable.
 */
interface SujiuBridge {
    suspend fun listSessions(): List<SessionSummary>

    suspend fun listCharacters(query: String? = null): List<CharacterSummary>

    suspend fun listModels(): List<ModelSummary>

    suspend fun listContextSources(): List<ContextSourceSummary>

    suspend fun conversationState(sessionId: String): ConversationSnapshot

    fun sendTurn(sessionId: String, userText: String): Flow<TurnEvent>

    fun cancelTurn(sessionId: String)

    /** Human readable identity of the runtime behind this bridge, for About. */
    fun runtimeSummary(): String
}
