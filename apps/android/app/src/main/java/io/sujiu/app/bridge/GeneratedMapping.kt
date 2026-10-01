package io.sujiu.app.bridge

import io.sujiu.app.presentation.CharacterRow
import io.sujiu.app.presentation.MessageRow
import io.sujiu.app.presentation.ModelRow
import io.sujiu.app.presentation.SessionRow
import io.sujiu.app.presentation.SourceKind
import io.sujiu.app.presentation.SourceRow
import io.sujiu.app.presentation.SpeakerRole
import io.sujiu.app.presentation.TurnStep
import io.sujiu.app.presentation.ChatMessageItem
import io.sujiu.app.presentation.ToolCallItem
import uniffi.sujiu.CharacterCard
import uniffi.sujiu.ContextKind
import uniffi.sujiu.ContextSourceRecord
import uniffi.sujiu.ConversationRecord
import uniffi.sujiu.ConversationSnapshotRecord
import uniffi.sujiu.MessageRecord
import uniffi.sujiu.MessageRole
import uniffi.sujiu.ModelRecord
import uniffi.sujiu.TurnEventKind
import uniffi.sujiu.TurnEventRecord

/**
 * Generated binding records to Android view models.
 *
 * This is the only file that names a generated type, which is the whole point:
 * the Rust contract is defined once, in Rust, and everything below it is a
 * conversion. A second Kotlin declaration of the same record would be a second
 * definition to keep in step, and nothing in a build would notice when the two
 * disagree.
 *
 * The mappings are exhaustive `when` expressions over the generated enums on
 * purpose. Rust can add a case; Kotlin then fails to compile here rather than
 * rendering the new case as "other" and hiding it.
 */

/** Who is in a conversation, joined for a row. */
private fun participantLine(participants: List<uniffi.sujiu.ParticipantRecord>): String =
    participants.joinToString(", ") { it.name }

/**
 * A conversation as the history list shows it.
 *
 * Rust reports a first participant id and a first participant name for
 * frontends that only know about one character. Android is not that frontend:
 * it groups and labels conversations, so it reads the participant list and
 * joins the names itself. The convenience fields are ignored on purpose.
 */
fun ConversationRecord.toRow(): SessionRow = SessionRow(
    id = id,
    title = title,
    who = participantLine(participants),
    updatedAt = updatedAtMs.toLong(),
    messageCount = messageCount.toInt(),
)

fun CharacterCard.toRow(): CharacterRow = CharacterRow(id = id, name = name, description = description)

fun ModelRecord.toRow(): ModelRow = ModelRow(
    id = id,
    name = name,
    endpointLabel = endpointLabel,
    configured = configured,
)

fun ContextSourceRecord.toRow(): SourceRow = SourceRow(
    id = id,
    name = name,
    kind = when (kind) {
        ContextKind.WORLD_LORE -> SourceKind.WorldLore
        ContextKind.STORY_EVENT -> SourceKind.StoryEvent
        ContextKind.CHARACTER_MEMORY -> SourceKind.CharacterMemory
        ContextKind.CHAT_HISTORY -> SourceKind.ChatHistory
        ContextKind.PERSONA -> SourceKind.Persona
        ContextKind.NOTE -> SourceKind.Note
        ContextKind.OTHER -> SourceKind.Other
    },
    recordCount = recordCount.toInt(),
)

/**
 * A transcript step as the message list shows it.
 *
 * `speakerId` is resolved against the characters the app already holds rather
 * than being shown as an id. A step that carries no attribution keeps an empty
 * speaker, which is a real answer: deciding who speaks next is a speaking-order
 * policy and this layer does not invent one.
 */
fun MessageRecord.toRow(resolveName: (String) -> String): MessageRow = MessageRow(
    id = id,
    role = when (role) {
        MessageRole.USER -> SpeakerRole.User
        MessageRole.ASSISTANT -> SpeakerRole.Assistant
        MessageRole.SYSTEM -> SpeakerRole.System
        MessageRole.DEVELOPER -> SpeakerRole.Developer
    },
    speaker = speakerId?.let(resolveName).orEmpty(),
    text = text,
    toolCalls = toolCalls.map { call ->
        ToolCallItem(
            id = call.id,
            toolName = call.name,
            statusLabel = call.status,
            summary = call.resultText.ifEmpty { null },
            isError = call.isError,
        )
    },
)

/** A runtime turn event as this screen's turn vocabulary. */
fun TurnEventRecord.toStep(): TurnStep = when (kind) {
    TurnEventKind.TURN_STARTED -> TurnStep.Started
    TurnEventKind.TEXT_DELTA -> TurnStep.TextDelta(text.orEmpty())
    TurnEventKind.THINKING_DELTA -> TurnStep.ThinkingDelta(text.orEmpty())
    TurnEventKind.TOOL_CALL_REQUESTED, TurnEventKind.TOOL_CALL_STARTED ->
        TurnStep.ToolCallStarted(callId = toolCallId.orEmpty(), toolName = toolName.orEmpty())

    TurnEventKind.TOOL_CALL_FINISHED -> TurnStep.ToolCallFinished(
        callId = toolCallId.orEmpty(),
        toolName = toolName.orEmpty(),
        summary = text.orEmpty(),
        isError = isError ?: false,
    )

    TurnEventKind.TURN_COMPLETED -> TurnStep.Completed
    TurnEventKind.TURN_FAILED -> TurnStep.Failed(text.orEmpty())
    TurnEventKind.TURN_CANCELLED -> TurnStep.Cancelled
}

/** A transcript step as a message list item, folding the tool calls in. */
fun MessageRecord.toItem(resolveName: (String) -> String): ChatMessageItem {
    val row = toRow(resolveName)
    return when (row.role) {
        SpeakerRole.User -> ChatMessageItem.User(row.id, row.text)
        SpeakerRole.Assistant -> ChatMessageItem.Assistant(
            id = row.id,
            text = row.text,
            toolCalls = row.toolCalls,
        )

        SpeakerRole.System, SpeakerRole.Developer -> ChatMessageItem.Assistant(row.id, row.text)
    }
}

/**
 * A whole conversation as Android reads it.
 *
 * The snapshot's `character` field is deliberately unused. It is the first
 * participant's card, which the runtime offers to a frontend that only knows
 * about one character, and Android is not that frontend: a three-participant
 * conversation has no primary one, and rendering the first as if it were would
 * be a claim the conversation does not make.
 */
fun ConversationSnapshotRecord.toView(): ConversationView {
    val names = participants.associate { it.characterId to it.name }
    return ConversationView(
        conversationId = conversationId,
        participants = participants.map { record ->
            CharacterRow(
                id = record.characterId,
                name = record.name,
                description = character?.takeIf { it.id == record.characterId }?.description.orEmpty(),
            )
        },
        personaId = personaId,
        worldBookIds = worldbookIds,
        promptProfileId = promptProfileId,
        messages = messages.map { it.toRow { speakerId -> names[speakerId].orEmpty() } },
    )
}