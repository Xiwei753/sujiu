package io.sujiu.app.presentation

/**
 * What a chat surface renders.
 *
 * These are Android's own shapes. They are not a second copy of the Rust
 * app-facing records: the bridge converts generated binding records into them,
 * and nothing else in the app sees a generated type. A field appears here
 * because a screen needs it, not because Rust has a matching one, which is why
 * [SessionRow] composes a participant line instead of carrying a first
 * character id and making the UI resolve it.
 *
 * When a view model and a Rust record happen to have the same fields today,
 * that is not permission to edit one from the other. Edit the Rust record,
 * regenerate, and map the new field here or leave it out.
 */

/** A conversation as the history list shows it. */
data class SessionRow(
    val id: String,
    val title: String,
    /** Who is in this conversation, already joined for display. */
    val who: String,
    val updatedAt: Long,
    val messageCount: Int,
)

/** A character as a picker row shows it. */
data class CharacterRow(
    val id: String,
    val name: String,
    val description: String,
)

/** A model as a picker row shows it. */
data class ModelRow(
    val id: String,
    val name: String,
    /** Who answers, for a person to read. Never read by behaviour. */
    val endpointLabel: String,
    val configured: Boolean,
)

/** A context source as the sources sheet shows it. */
data class SourceRow(
    val id: String,
    val name: String,
    val kind: SourceKind,
    val recordCount: Int,
)

/**
 * The kinds of readable information the runtime can hold.
 *
 * Android keeps this enum instead of importing the generated one so that a
 * screen can localize it and presentation never depends on a generated type.
 * The mapping is in `bridge/GeneratedMapping.kt`; a kind added in Rust that is
 * not mapped here fails to compile rather than silently rendering as "other".
 */
enum class SourceKind {
    WorldLore,
    StoryEvent,
    CharacterMemory,
    ChatHistory,
    Persona,
    Note,
    Other,
}

/**
 * Who a transcript step is attributed to.
 *
 * [Developer] is a fixed instruction to the model rather than a person
 * speaking. It is kept as its own case instead of being folded into [System]
 * here: the runtime distinguishes them, and collapsing them at the mapping
 * layer is a rendering decision that has not been asked for yet.
 */
enum class SpeakerRole {
    User,
    Assistant,
    System,
    Developer,
}

/** One transcript step, shaped for the message list. */
data class MessageRow(
    val id: String,
    val role: SpeakerRole,
    /** The participant this step is attributed to, empty when it carries none. */
    val speaker: String,
    val text: String,
    val toolCalls: List<ToolCallItem>,
)

/**
 * One step of a turn in flight.
 *
 * Android folds the runtime's turn events into these, because a streaming
 * screen cares about "what is happening now" rather than about the runtime's
 * event vocabulary. The folding is one-way and loses nothing the model saw: the
 * transcript keeps every step, this only decides what is on screen.
 */
sealed interface TurnStep {
    data object Started : TurnStep

    data class TextDelta(val text: String) : TurnStep

    data class ThinkingDelta(val text: String) : TurnStep

    data class ToolCallStarted(val callId: String, val toolName: String) : TurnStep

    data class ToolCallFinished(
        val callId: String,
        val toolName: String,
        val summary: String,
        val isError: Boolean,
    ) : TurnStep

    data object Completed : TurnStep

    data class Failed(val message: String) : TurnStep

    data object Cancelled : TurnStep
}