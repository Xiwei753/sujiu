package io.sujiu.app.bridge

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow

/**
 * Preview data source used until `sujiu-ffi` exposes the conversation surface.
 *
 * It implements exactly the same contract the FFI bridge will implement, so
 * the presentation and UI layers need no change when the real runtime lands.
 * The scripted turn intentionally walks the full normalized event set:
 * thinking, text, a context tool call, more text, completion, plus the
 * cancelled and failed paths.
 */
class InMemorySujiuBridge(
    private val deltaDelayMillis: Long = 24L,
) : SujiuBridge {

    private val models = listOf(
        ModelSummary("model-balanced", "Sujiu Chat 3", "Sujiu", "200k context"),
        ModelSummary("model-reasoning", "Sujiu Reason 3", "Sujiu", "200k context · thinking"),
        ModelSummary("model-local", "Local Qwen 3", "On device", "32k context"),
    )

    private val characters = listOf(
        CharacterSummary(
            id = "char-lin",
            name = "Lin",
            tagline = "Night-shift radio host who never says what she means",
            tags = listOf("Modern", "Slow burn", "Radio"),
        ),
        CharacterSummary(
            id = "char-qi",
            name = "Qi",
            tagline = "Archivist of a city that rewrote its own name",
            tags = listOf("Fantasy", "Mystery"),
        ),
        CharacterSummary(
            id = "char-ayan",
            name = "Ayan",
            tagline = "Courier with a debt and a bicycle",
            tags = listOf("Modern", "Adventure"),
        ),
        CharacterSummary(
            id = "char-mira",
            name = "Mira",
            tagline = "Cartographer mapping places that are not there yet",
            tags = listOf("Fantasy", "Travel"),
        ),
    )

    private val contextSources = listOf(
        ContextSourceSummary("world-lore", "Rain City lorebook", "World lore", 42),
        ContextSourceSummary("story-event", "Night of the broadcast", "Story event", 8),
        ContextSourceSummary("memory-lin", "Lin · long term memory", "Character memory", 17),
        ContextSourceSummary("chat-history", "Older conversations", "Chat history", 96),
    )

    private val now = System.currentTimeMillis()

    private val sessions = listOf(
        SessionSummary(
            id = "session-1",
            title = "The 2 a.m. frequency",
            characterId = "char-lin",
            characterName = "Lin",
            preview = "…and the signal answers back.",
            updatedAt = now - 12 * 60_000L,
        ),
        SessionSummary(
            id = "session-2",
            title = "Archive of the drowned district",
            characterId = "char-qi",
            characterName = "Qi",
            preview = "The map is older than the street it describes.",
            updatedAt = now - 26 * 60 * 60_000L,
        ),
        SessionSummary(
            id = "session-3",
            title = "A package, no return address",
            characterId = "char-ayan",
            characterName = "Ayan",
            preview = "I counted the seals twice. Both times: nine.",
            updatedAt = now - 3 * 24 * 60 * 60_000L,
        ),
        SessionSummary(
            id = "session-4",
            title = "Coastline that keeps moving",
            characterId = "char-mira",
            characterName = "Mira",
            preview = "Every tide redraws the border.",
            updatedAt = now - 9 * 24 * 60 * 60_000L,
        ),
    )

    private val messagesBySession = mapOf(
        "session-1" to listOf(
            MessageSummary(
                id = "m-1",
                role = MessageRole.User,
                text = "You said the frequency was dead. Why is the light on the console blinking?",
            ),
            MessageSummary(
                id = "m-2",
                role = MessageRole.Assistant,
                text = "Because the console and I are arguing about who owns the night shift. " +
                    "The blinking is a caller who has not decided to speak yet.",
                toolName = "search_context",
                toolStatusLabel = "Searched 3 records",
            ),
            MessageSummary(
                id = "m-3",
                role = MessageRole.User,
                text = "And if they decide to speak while I am on the line?",
            ),
        ),
    )

    private var cancelledSessionId: String? = null

    override suspend fun listSessions(): List<SessionSummary> = sessions

    override suspend fun listCharacters(query: String?): List<CharacterSummary> {
        val trimmed = query?.trim().orEmpty()
        if (trimmed.isEmpty()) return characters
        return characters.filter {
            it.name.contains(trimmed, ignoreCase = true) ||
                it.tagline.contains(trimmed, ignoreCase = true) ||
                it.tags.any { tag -> tag.contains(trimmed, ignoreCase = true) }
        }
    }

    override suspend fun listModels(): List<ModelSummary> = models

    override suspend fun listContextSources(): List<ContextSourceSummary> = contextSources

    override suspend fun conversationState(sessionId: String): ConversationSnapshot {
        val session = sessions.firstOrNull { it.id == sessionId } ?: sessions.first()
        return ConversationSnapshot(
            session = session,
            messages = messagesBySession[session.id].orEmpty(),
            contextSources = contextSources,
        )
    }

    override fun sendTurn(sessionId: String, userText: String): Flow<TurnEvent> = flow {
        cancelledSessionId = null
        emit(TurnEvent.Started)

        val script = scriptFor(userText)
        for (step in script) {
            if (cancelledSessionId == sessionId) {
                emit(TurnEvent.Cancelled)
                return@flow
            }
            delay(deltaDelayMillis)
            emit(step)
        }
        if (cancelledSessionId == sessionId) {
            emit(TurnEvent.Cancelled)
        } else {
            emit(TurnEvent.Completed)
        }
    }

    override fun cancelTurn(sessionId: String) {
        cancelledSessionId = sessionId
    }

    override fun runtimeSummary(): String = "Preview bridge · sujiu-ffi conversation API pending"

    private fun scriptFor(userText: String): List<TurnEvent> {
        if (userText.contains("fail", ignoreCase = true)) {
            return listOf(
                TurnEvent.TextDelta("The provider rejected the request"),
                TurnEvent.Failed("Request failed: provider returned 429"),
            )
        }
        return listOf(
            TurnEvent.ThinkingDelta("Recall the broadcast record, then answer in character."),
            TurnEvent.TextDelta("The console blinks because the night is not over yet. "),
            TurnEvent.TextDelta("You asked earlier whether I would let a caller in. "),
            TurnEvent.ToolCallStarted("call-1", "search_context"),
            TurnEvent.ToolCallFinished(
                callId = "call-1",
                toolName = "search_context",
                summary = "3 records · 1 world lore, 1 story event, 1 memory",
                isError = false,
            ),
            TurnEvent.TextDelta("I checked the night log, the lorebook and what I still owe you. "),
            TurnEvent.TextDelta("The answer is no, and I am going to let you stay anyway."),
        )
    }
}
