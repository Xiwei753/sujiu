package io.sujiu.app.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.togetherWith
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent
import io.sujiu.app.presentation.Screen
import io.sujiu.app.ui.chat.ChatScreen
import io.sujiu.app.ui.library.CharacterDetailScreen
import io.sujiu.app.ui.library.CharacterLibraryScreen
import io.sujiu.app.ui.settings.SettingsScreen
import io.sujiu.app.ui.theme.SujiuTheme

/**
 * Application shell: owns navigation transitions and screen composition only.
 * Every decision about *what* is shown comes from presentation state.
 */
@Composable
fun SujiuApp(controller: ChatController) {
    val state by controller.state.collectAsStateWithLifecycle()
    val backStack by controller.navigation.backStack.collectAsStateWithLifecycle()

    SujiuTheme(mode = state.appearanceMode) {
        Surface(modifier = Modifier.fillMaxSize()) {
            AnimatedContent(
                targetState = backStack,
                transitionSpec = { fadeIn() togetherWith fadeOut() },
                label = "sujiu-navigation",
            ) { stack ->
                when (val screen = stack.last()) {
                    Screen.Chat -> ChatScreen(controller)
                    Screen.Library -> CharacterLibraryScreen(controller)
                    is Screen.CharacterDetail -> CharacterDetailScreen(
                        controller = controller,
                        characterId = screen.characterId,
                    )
                    Screen.Settings -> SettingsScreen(controller)
                }
            }
        }
    }
}

/** Navigation helpers shared by the screens; they only mutate the navigator. */
object SujiuNavigation {
    fun openLibrary(controller: ChatController) = controller.navigation.push(Screen.Library)

    fun openSettings(controller: ChatController) = controller.navigation.push(Screen.Settings)

    fun openCharacter(controller: ChatController, characterId: String) =
        controller.navigation.push(Screen.CharacterDetail(characterId))

    fun back(controller: ChatController) {
        controller.navigation.pop()
    }

    fun selectCharacterAndReturn(controller: ChatController, characterId: String) {
        controller.dispatch(ChatIntent.CharacterSelected(characterId))
        controller.navigation.popToRoot()
    }
}
