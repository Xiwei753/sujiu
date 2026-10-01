package io.sujiu.app.bridge

import io.sujiu.app.presentation.CharacterRow
import io.sujiu.app.presentation.MessageRow
import io.sujiu.app.presentation.ModelRow
import io.sujiu.app.presentation.SessionRow
import io.sujiu.app.presentation.SourceRow
import io.sujiu.app.presentation.TurnStep
import kotlinx.coroutines.flow.Flow

/**
 * The application facing surface of the shared Rust runtime.
 *
 * Everything here is a view model from `presentation`, never a Rust record.
 * The Rust app-facing API is defined once, in `crates/sujiu-uniffi`, and
 * UniFFI generates the Kotlin for it at build time;
 * [UniffiSujiuBridge] maps those generated records onto the view models in
 * `GeneratedMapping.kt`.
 *
 * This interface exists so the frontend stack stays exercisable without a
 * built native library, and so the two implementations can be compared. It is
 * an operation set, not a copy of the binding contract: it declares what a chat
 * surface needs and holds no provider wire format, no HTTP detail and no
 * storage document.
 */
interface SujiuBridge {
    suspend fun listSessions(): List<SessionRow>

    suspend fun listCharacters(query: String? = null): List<CharacterRow>

    suspend fun listModels(): List<ModelRow>

    suspend fun listContextSources(): List<SourceRow>

    /** The transcript of one conversation, with the four bindings it uses. */
    suspend fun conversationState(conversationId: String): ConversationView

    fun sendTurn(conversationId: String, userText: String): Flow<TurnStep>

    fun cancelTurn(conversationId: String)

    /** Human readable identity of the runtime behind this bridge, for About. */
    fun runtimeSummary(): String
}

/**
 * One conversation, as a chat screen reads it.
 *
 * Built from the generated snapshot rather than taken from it: the runtime also
 * reports a first participant's card for frontends that only know about one
 * character, and Android is not that frontend.
 */
data class ConversationView(
    val conversationId: String,
    val participants: List<CharacterRow>,
    val personaId: String?,
    val worldBookIds: List<String>,
    val promptProfileId: String?,
    val messages: List<MessageRow>,
)