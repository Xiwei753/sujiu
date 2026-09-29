package io.sujiu.app.presentation

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Page navigation.
 *
 * Navigation state is a presentation concern: Rust does not know which page
 * is on top, and the platform never drives navigation from the runtime.
 */
sealed interface Screen {
    data object Chat : Screen
    data object Library : Screen
    data class CharacterDetail(val characterId: String) : Screen
    data object Settings : Screen
}

class Navigator {
    private val _backStack = MutableStateFlow<List<Screen>>(listOf(Screen.Chat))

    val backStack: StateFlow<List<Screen>> = _backStack.asStateFlow()

    val current: Screen get() = _backStack.value.last()

    val canGoBack: Boolean get() = _backStack.value.size > 1

    fun push(screen: Screen) {
        _backStack.value = _backStack.value + screen
    }

    fun pop(): Boolean {
        val stack = _backStack.value
        if (stack.size <= 1) return false
        _backStack.value = stack.dropLast(1)
        return true
    }

    fun popToRoot() {
        _backStack.value = listOf(Screen.Chat)
    }
}
