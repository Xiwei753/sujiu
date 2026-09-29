package io.sujiu.app

import android.content.Context
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import io.sujiu.app.bridge.InMemorySujiuBridge
import io.sujiu.app.bridge.SujiuBridge
import io.sujiu.app.platform.AndroidClipboardService
import io.sujiu.app.platform.AndroidPlatformInfoService
import io.sujiu.app.platform.AndroidSystemAppearanceService
import io.sujiu.app.platform.PlatformServices
import io.sujiu.app.presentation.ChatController
import io.sujiu.app.presentation.ChatIntent
import io.sujiu.app.ui.SujiuApp
import kotlinx.coroutines.launch

/**
 * Composition root.
 *
 * This is the only place that knows concrete implementations: the UI layer
 * receives a [ChatController] and never constructs a bridge, a runtime or a
 * platform service itself. Swapping the preview bridge for the `sujiu-ffi`
 * bridge is a one-line change here.
 */
class AppGraph(context: Context) {
    val bridge: SujiuBridge = InMemorySujiuBridge()

    val platform = PlatformServices(
        clipboard = AndroidClipboardService(context.applicationContext),
        appearance = AndroidSystemAppearanceService(context.applicationContext),
        info = AndroidPlatformInfoService(context.applicationContext),
    )

    val controller = ChatController(
        scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Main.immediate),
        bridge = bridge,
        platform = platform,
    )
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val graph = AppGraph(this)
        graph.controller.refresh()

        // Refreshing on resume is a platform lifecycle concern, so it happens
        // here rather than inside the chat screen.
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                graph.controller.dispatch(ChatIntent.AppForegrounded)
            }
        }

        setContent { SujiuApp(controller = graph.controller) }
    }
}
