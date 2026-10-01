package io.sujiu.app.presentation

import io.sujiu.app.bridge.ConversationView
import io.sujiu.app.bridge.SujiuBridge
import io.sujiu.app.platform.AppearanceMode
import io.sujiu.app.platform.ClipboardService
import io.sujiu.app.platform.PlatformInfoService
import io.sujiu.app.platform.PlatformServices
import io.sujiu.app.platform.SystemAppearanceService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ChatControllerTest {

    private fun TestScope.scope(): CoroutineScope = CoroutineScope(StandardTestDispatcher(testScheduler))

    private fun build(scope: CoroutineScope, clipboard: ClipboardService = RecordingClipboard()): ChatController =
        ChatController(
            scope = scope,
            bridge = ScriptedBridge(),
            platform = PlatformServices(
                clipboard = clipboard,
                appearance = object : SystemAppearanceService { override fun systemPrefersDark() = false },
                info = object : PlatformInfoService { override fun platformSummary() = "test" },
            ),
        )

    @Test
    fun `refresh loads the first session as the current conversation`() = runTest {
        val controller = build(scope())
        controller.refresh()
        advanceUntilIdle()

        val state = controller.state.value
        assertEquals("session-1", state.currentSessionId)
        assertEquals(1, state.messages.size)
        assertEquals("Lin", state.currentCharacter?.name)
        assertNotNull(state.currentModel)
        assertEquals(GenerationState.Idle, state.generationState)
        assertTrue(state.sessionGroups.isNotEmpty())
        assertTrue(state.contextSources.isNotEmpty())
    }

    @Test
    fun `a full turn walks submitting streaming tool and completion`() = runTest {
        val controller = build(scope())
        controller.refresh()
        advanceUntilIdle()

        controller.dispatch(ChatIntent.DraftChanged("hello"))
        controller.dispatch(ChatIntent.SendClicked)
        assertEquals(GenerationState.Submitting, controller.state.value.generationState)

        advanceUntilIdle()

        val state = controller.state.value
        assertEquals(GenerationState.Completed, state.generationState)
        assertFalse(state.busy)

        val assistant = state.messages.last() as ChatMessageItem.Assistant
        assertTrue(assistant.text.contains("The night is not over"))
        assertTrue(assistant.text.contains("neither is this answer"))
        assertEquals(1, assistant.toolCalls.size)
        assertEquals("search_context", assistant.toolCalls.first().toolName)
        assertEquals("done", assistant.toolCalls.first().statusLabel)
        assertEquals("3 records", assistant.toolCalls.first().summary)
        assertTrue(assistant.thinking.isNotEmpty())
    }

    @Test
    fun `tool activity is reported before the answer continues`() = runTest {
        val bridge = ScriptedBridge()
        val controller = ChatController(
            scope = scope(),
            bridge = bridge,
            platform = PlatformServices(
                clipboard = RecordingClipboard(),
                appearance = object : SystemAppearanceService { override fun systemPrefersDark() = false },
                info = object : PlatformInfoService { override fun platformSummary() = "test" },
            ),
        )
        controller.refresh()
        advanceUntilIdle()

        controller.dispatch(ChatIntent.DraftChanged("hello"))
        controller.dispatch(ChatIntent.SendClicked)
        advanceUntilIdle()

        assertEquals(
            listOf("text-1", "tool-start", "tool-finish", "text-2"),
            bridge.emittedMarkers,
        )
    }

    @Test
    fun `stopping a turn cancels it and clears the busy state`() = runTest {
        val controller = build(scope())
        controller.refresh()
        advanceUntilIdle()

        controller.dispatch(ChatIntent.DraftChanged("hello"))
        controller.dispatch(ChatIntent.SendClicked)
        controller.dispatch(ChatIntent.StopClicked)

        val state = controller.state.value
        assertEquals(GenerationState.Cancelled, state.generationState)
        assertFalse(state.busy)
    }

    @Test
    fun `a failed turn keeps the error visible`() = runTest {
        val controller = build(scope())
        controller.refresh()
        advanceUntilIdle()

        controller.dispatch(ChatIntent.DraftChanged("please fail"))
        controller.dispatch(ChatIntent.SendClicked)
        advanceUntilIdle()

        val state = controller.state.value
        assertEquals(GenerationState.Failed, state.generationState)
        assertNotNull(state.errorMessage)
    }

    @Test
    fun `the draft is cleared on send and send stays disabled while busy`() = runTest {
        val controller = build(scope())
        controller.refresh()
        advanceUntilIdle()

        controller.dispatch(ChatIntent.DraftChanged("hello"))
        controller.dispatch(ChatIntent.SendClicked)

        assertEquals("", controller.state.value.draft)
        assertTrue(controller.state.value.busy)
        assertFalse(controller.state.value.canSend)
    }

    @Test
    fun `selecting a character and model only changes presentation state`() = runTest {
        val controller = build(scope())
        controller.refresh()
        advanceUntilIdle()

        controller.dispatch(ChatIntent.CharacterQueryChanged("qi"))
        advanceUntilIdle()
        assertEquals(listOf("Qi"), controller.state.value.characters.map { it.name })

        controller.dispatch(ChatIntent.CharacterSelected("char-qi"))
        controller.dispatch(ChatIntent.ModelSelected("model-local"))
        controller.dispatch(ChatIntent.AppearanceSelected(AppearanceMode.Dark))

        val state = controller.state.value
        assertEquals("Qi", state.currentCharacter?.name)
        assertEquals("Local Qwen 3", state.currentModel?.name)
        assertEquals(AppearanceMode.Dark, state.appearanceMode)
    }

    @Test
    fun `copying a message goes through the clipboard capability`() = runTest {
        val clipboard = RecordingClipboard()
        val controller = build(scope(), clipboard)
        controller.refresh()
        advanceUntilIdle()

        controller.copyMessage(controller.state.value.messages.first())
        assertEquals(1, clipboard.copyCount)
    }

    @Test
    fun `navigator pops back to the chat root`() {
        val navigator = Navigator()
        assertFalse(navigator.canGoBack)

        navigator.push(Screen.Settings)
        assertTrue(navigator.canGoBack)
        assertEquals(Screen.Settings, navigator.current)

        assertTrue(navigator.pop())
        assertEquals(Screen.Chat, navigator.current)

        navigator.push(Screen.Library)
        navigator.push(Screen.CharacterDetail("char-lin"))
        navigator.popToRoot()
        assertEquals(Screen.Chat, navigator.current)
    }
}

private class RecordingClipboard : ClipboardService {
    var copyCount = 0

    override fun copyText(label: String, text: String) {
        copyCount++
    }
}

/** Deterministic bridge: the same normalized step sequence a provider would produce. */
private class ScriptedBridge : SujiuBridge {
    val emittedMarkers = mutableListOf<String>()

    private val sessions = listOf(
        SessionRow("session-1", "First", "Lin", System.currentTimeMillis(), 1),
        SessionRow("session-2", "Second", "Qi", 0L, 1),
    )

    private val characters = listOf(
        CharacterRow("char-lin", "Lin", "Radio host"),
        CharacterRow("char-qi", "Qi", "Archivist"),
    )

    private val models = listOf(
        ModelRow("model-balanced", "Sujiu Chat 3", "Sujiu", true),
        ModelRow("model-local", "Local Qwen 3", "On device", false),
    )

    private val sources = listOf(SourceRow("world-lore", "Lorebook", SourceKind.WorldLore, 3))

    override suspend fun listSessions() = sessions

    override suspend fun listCharacters(query: String?): List<CharacterRow> {
        val trimmed = query?.trim().orEmpty()
        if (trimmed.isEmpty()) return characters
        return characters.filter { it.name.contains(trimmed, ignoreCase = true) }
    }

    override suspend fun listModels() = models

    override suspend fun listContextSources() = sources

    override suspend fun conversationState(conversationId: String) = ConversationView(
        conversationId = conversationId,
        participants = emptyList(),
        personaId = null,
        worldBookIds = emptyList(),
        promptProfileId = null,
        messages = listOf(MessageRow("m-1", SpeakerRole.User, "", "hi", emptyList())),
    )

    override fun sendTurn(conversationId: String, userText: String): Flow<TurnStep> = flow {
        emit(TurnStep.Started)
        emit(TurnStep.ThinkingDelta("considering"))
        emit(TurnStep.TextDelta("The night is not over"))
        emittedMarkers.add("text-1")
        emit(TurnStep.ToolCallStarted("call-1", "search_context"))
        emittedMarkers.add("tool-start")
        emit(TurnStep.ToolCallFinished("call-1", "search_context", "3 records", false))
        emittedMarkers.add("tool-finish")
        emit(TurnStep.TextDelta(" — and neither is this answer."))
        emittedMarkers.add("text-2")
        if (userText.contains("fail", ignoreCase = true)) {
            emit(TurnStep.Failed("the endpoint returned 429"))
        } else {
            emit(TurnStep.Completed)
        }
    }

    override fun cancelTurn(conversationId: String) = Unit

    override fun runtimeSummary() = "test bridge"
}
