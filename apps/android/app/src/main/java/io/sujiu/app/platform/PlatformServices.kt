package io.sujiu.app.platform

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.res.Configuration
import android.os.Build

/**
 * Platform capability services.
 *
 * Each service exposes capability verbs (`copyText`, `systemPrefersDark`) and
 * never page verbs. UI code must not reach for these directly: it asks the
 * presentation layer, which calls the capability it needs. Only services that
 * have a real caller are implemented today; the remaining catalogue entries
 * from docs/UI_ARCHITECTURE.md are added when a feature needs them.
 */

enum class AppearanceMode { System, Light, Dark }

/** Clipboard access. */
interface ClipboardService {
    fun copyText(label: String, text: String)
}

class AndroidClipboardService(private val context: Context) : ClipboardService {
    override fun copyText(label: String, text: String) {
        val manager = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
        manager.setPrimaryClip(ClipData.newPlainText(label, text))
    }
}

/** System light/dark preference. */
interface SystemAppearanceService {
    fun systemPrefersDark(): Boolean
}

class AndroidSystemAppearanceService(private val context: Context) : SystemAppearanceService {
    override fun systemPrefersDark(): Boolean {
        val nightFlags = context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
        return nightFlags == Configuration.UI_MODE_NIGHT_YES
    }
}

/** Static facts about the host platform, for the settings/about surface. */
interface PlatformInfoService {
    fun platformSummary(): String
}

class AndroidPlatformInfoService(private val context: Context) : PlatformInfoService {
    override fun platformSummary(): String {
        val configuration = context.resources.configuration
        val size = when {
            configuration.smallestScreenWidthDp >= 600 -> "tablet"
            else -> "phone"
        }
        return "Android ${Build.VERSION.RELEASE} · API ${Build.VERSION.SDK_INT} · $size layout"
    }
}

/**
 * The set of platform capabilities the presentation layer is allowed to use.
 */
class PlatformServices(
    val clipboard: ClipboardService,
    val appearance: SystemAppearanceService,
    val info: PlatformInfoService,
)
