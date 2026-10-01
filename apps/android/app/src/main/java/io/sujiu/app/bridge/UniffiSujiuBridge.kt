package io.sujiu.app.bridge

import io.sujiu.app.presentation.CharacterRow
import io.sujiu.app.presentation.ModelRow
import io.sujiu.app.presentation.SessionRow
import io.sujiu.app.presentation.SourceRow
import io.sujiu.app.presentation.TurnStep
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import uniffi.sujiu.SendTurnRequestRecord
import uniffi.sujiu.SujiuApp
import uniffi.sujiu.SujiuAppInterface
import uniffi.sujiu.TurnEventListener
import uniffi.sujiu.TurnEventRecord
import uniffi.sujiu.create

/**
 * The real bridge: UniFFI bindings, generated from the Rust app-facing API.
 *
 * Every record that crosses this line is a generated one, and every generated
 * type that appears anywhere in the app appears only here or in
 * `GeneratedMapping.kt`. The conversion to Android's own shapes is therefore
 * exhaustive and written once, rather than repeated at each call site and left
 * to rot there.
 *
 * Two things this deliberately does not do:
 *
 * - It does not build a request out of anything the runtime did not produce. A
 *   turn is sent with the endpoint the runtime already has and no API key,
 *   because Android has no credential store wired in yet; passing a key here
 *   would be a way to keep a secret in a string field, not a feature.
 * - It does not catch a native failure into a fake empty answer. An
 *   unavailable runtime is an error the caller should see, and turning it into
 *   "no characters" would let the app look healthy while it is not.
 */
class UniffiSujiuBridge(
    private val app: SujiuAppInterface,
) : SujiuBridge {

    /**
     * Build a bridge over a fresh runtime.
     *
     * Throws when the native library cannot load: a missing or mismatched
     * `libsujiu_uniffi.so` means the whole runtime is absent, and that is not
     * something a chat screen can degrade around.
     */
    constructor() : this(create())

    override suspend fun listSessions(): List<SessionRow> =
        app.listConversations().map { it.toRow() }

    override suspend fun listCharacters(query: String?): List<CharacterRow> =
        app.listCharacters(query).map { it.toRow() }

    override suspend fun listModels(): List<ModelRow> =
        app.listModels().map { it.toRow() }

    override suspend fun listContextSources(): List<SourceRow> =
        app.listContextSources().map { it.toRow() }

    override suspend fun conversationState(conversationId: String): ConversationView =
        app.conversationState(conversationId)?.toView()
            ?: throw IllegalStateException("conversation_not_found: $conversationId")

    /**
     * One turn, as a flow of steps.
     *
     * `callbackFlow` because the runtime reports through a listener it calls on
     * its own task, not by returning a stream. The flow closes when the turn
     * does: the listener sees the terminal event, so the collector is closed
     * there rather than by a timeout. Cancelling collection drops the listener,
     * which is also what makes `cancelTurn` the only way to stop a turn early.
     */
    override fun sendTurn(conversationId: String, userText: String): Flow<TurnStep> = callbackFlow {
        val listener = object : TurnEventListener {
            override fun onTurnEvent(event: TurnEventRecord) {
                val step: TurnStep = event.toStep()
                trySend(step)
                // The runtime reports the outcome as an event rather than as a
                // return value, so the terminal event is where the turn ends.
                // Closing here rather than on sendTurn's return keeps the flow
                // from waiting for a send that will never produce more.
                if (step is TurnStep.Completed || step is TurnStep.Failed ||
                    step is TurnStep.Cancelled
                ) {
                    close()
                }
            }
        }
        val request = SendTurnRequestRecord(
            conversationId = conversationId,
            userText = userText,
            endpoint = app.endpoint(),
            apiKey = null,
        )
        app.sendTurn(request, listener)
        awaitClose { }
    }

    /**
     * Ask the running turn to stop.
     *
     * The conversation id is not checked against the running turn: the runtime
     * keeps one turn at a time, and a stop request that arrived just as a turn
     * ended has nothing left to interrupt anyway. Passing it would mean
     * answering "is this the turn you meant" from the UI, which is a question
     * about runtime state the UI cannot see.
     */
    override fun cancelTurn(conversationId: String) {
        app.cancelTurn()
    }

    override fun runtimeSummary(): String =
        "UniFFI · sujiu-runtime ${app.coreVersion()}"

    /** The generated object, for a screen that needs an operation this bridge does not map. */
    fun native(): SujiuAppInterface = app
}