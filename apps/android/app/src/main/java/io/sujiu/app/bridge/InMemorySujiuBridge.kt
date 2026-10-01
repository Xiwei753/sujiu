package io.sujiu.app.bridge

import io.sujiu.app.presentation.CharacterRow
import io.sujiu.app.presentation.MessageRow
import io.sujiu.app.presentation.ModelRow
import io.sujiu.app.presentation.SessionRow
import io.sujiu.app.presentation.SourceKind
import io.sujiu.app.presentation.SourceRow
import io.sujiu.app.presentation.SpeakerRole
import io.sujiu.app.presentation.ToolCallItem
import io.sujiu.app.presentation.TurnStep
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow

/**
 * Preview data source used until the Android app is wired to the native runtime.
 *
 * It builds the same view models `UniffiSujiuBridge` builds from generated
 * binding records, so the presentation and UI layers need no change when the
 * real bridge replaces it. That is the point of keeping [SujiuBridge] an
 * operation set over view models rather than a copy of the binding contract:
 * this file names no generated type, and neither does anything above it.
 *
 * The scripted turn intentionally walks the full turn-step set: thinking, text,
 * a tool call, more text, completion, plus the cancelled and failed paths.
 */
class InMemorySujiuBridge(
    private val deltaDelayMillis: Long = 24L,
) : SujiuBridge {

    private val models = listOf(
        ModelRow("model-balanced", "Sujiu Chat 3", "Sujiu", true),
        ModelRow("model-reasoning", "Sujiu Reason 3", "Sujiu", true),
        ModelRow("model-local", "Local Qwen 3", "On device", false),
    )

    private val characters = listOf(
        CharacterRow("char-lin", "Lin", "Night-shift radio host who never says what she means"),
        CharacterRow("char-qi", "Qi", "Archivist of a city that rewrote its own name"),
        CharacterRow("char-ayan", "Ayan", "Courier with a debt and a bicycle"),
        CharacterRow("char-mira", "Mira", "Cartographer mapping places that are not there yet"),
    )

    private val contextSources = listOf(
        SourceRow("world-lore", "Rain City lorebook", SourceKind.WorldLore, 42),
        SourceRow("story-event", "Night of the broadcast", SourceKind.StoryEvent, 8),
        SourceRow("memory-lin", "Lin · long term memory", SourceKind.CharacterMemory, 17),
        SourceRow("chat-history", "Older conversations", SourceKind.ChatHistory, 96),
    )

    private val now = System.currentTimeMillis()

    private val sessions = listOf(
        SessionRow(
            id = "session-1",
            title = "The 2 a.m. frequency",
            who = "Lin",
            updatedAt = now - 12 * 60_000L,
            messageCount = 3,
        ),
        SessionRow(
            id = "session-2",
            title = "Archive of the drowned district",
            who = "Qi",
            updatedAt = now - 26 * 60 * 60_000L,
            messageCount = 12,
        ),
        SessionRow(
            id = "session-3",
            title = "A package, no return address",
            who = "Ayan",
            updatedAt = now - 3 * 24 * 60 * 60_000L,
            messageCount = 7,
        ),
        SessionRow(
            id = "session-4",
            title = "Coastline that keeps moving",
            who = "Mira",
            updatedAt = now - 9 * 24 * 60 * 60_000L,
            messageCount = 21,
        ),
    )

    private val messagesBySession = mapOf(
        "session-1" to listOf(
            MessageRow(
                id = "m-1",
                role = SpeakerRole.User,
                speaker = "You",
                text = "You said the frequency was dead. Why is the light on the console blinking?",
                toolCalls = emptyList(),
            ),
            MessageRow(
                id = "m-2",
                role = SpeakerRole.Assistant,
                speaker = "Lin",
                text = "Because the console and I are arguing about who owns the night shift. " +
                    "The blinking is a caller who has not decided to speak yet.",
                toolCalls = listOf(
                    ToolCallItem(
                        id = "call-1",
                        toolName = "search_context",
                        statusLabel = "done",
                        summary = "3 records · 1 world lore, 1 story event, 1 memory",
                    ),
                ),
            ),
            MessageRow(
                id = "m-3",
                role = SpeakerRole.User,
                speaker = "You",
                text = "And if they decide to speak while I am on the line?",
                toolCalls = emptyList(),
            ),
        ),
    )

    private var cancelledConversationId: String? = null

    override suspend fun listSessions(): List<SessionRow> = sessions

    override suspend fun listCharacters(query: String?): List<CharacterRow> {
        val trimmed = query?.trim().orEmpty()
        if (trimmed.isEmpty()) return characters
        return characters.filter {
            it.name.contains(trimmed, ignoreCase = true) ||
                it.description.contains(trimmed, ignoreCase = true)
        }
    }

    override suspend fun listModels(): List<ModelRow> = models

    override suspend fun listContextSources(): List<SourceRow> = contextSources

    override suspend fun conversationState(conversationId: String): ConversationView {
        val session = sessions.firstOrNull { it.id == conversationId } ?: sessions.first()
        return ConversationView(
            conversationId = session.id,
            participants = characters.filter { it.id == session.who },
            // The preview binds nothing, and says so rather than inventing a
            // persona to look as though it does.
            personaId = null,
            worldBookIds = emptyList(),
            promptProfileId = null,
            messages = messagesBySession[session.id].orEmpty(),
        )
    }

    override fun sendTurn(conversationId: String, userText: String): Flow<TurnStep> = flow {
        cancelledConversationId = null
        emit(TurnStep.Started)

        val script = scriptFor(userText)
        for (step in script) {
            if (cancelledConversationId == conversationId) {
                emit(TurnStep.Cancelled)
                return@flow
            }
            delay(deltaDelayMillis)
            emit(step)
        }
        if (cancelledConversationId == conversationId) {
            emit(TurnStep.Cancelled)
        } else {
            emit(TurnStep.Completed)
        }
    }

    override fun cancelTurn(conversationId: String) {
        cancelledConversationId = conversationId
    }

    override fun runtimeSummary(): String = "Preview bridge · no native runtime linked yet"

    private fun scriptFor(userText: String): List<TurnStep> {
        if (userText.contains("fail", ignoreCase = true)) {
            return listOf(
                TurnStep.TextDelta("The provider rejected the request"),
                TurnStep.Failed("Request failed: the endpoint answered 429"),
            )
        }
        return listOf(
            TurnStep.ThinkingDelta("Recall the broadcast record, then answer in character."),
            TurnStep.TextDelta("The console blinks because the night is not over yet. "),
            TurnStep.TextDelta("You asked earlier whether I would let a caller in. "),
            TurnStep.ToolCallStarted("call-1", "search_context"),
            TurnStep.ToolCallFinished(
                callId = "call-1",
                toolName = "search_context",
                summary = "3 records · 1 world lore, 1 story event, 1 memory",
                isError = false,
            ),
            TurnStep.TextDelta("I checked the night log, the lorebook and what I still owe you. "),
            TurnStep.TextDelta("The answer is no, and I am going to let you stay anyway."),
        )
    }
}