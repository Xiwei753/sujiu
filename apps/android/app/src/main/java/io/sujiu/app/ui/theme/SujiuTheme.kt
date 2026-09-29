package io.sujiu.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import io.sujiu.app.platform.AppearanceMode

private val LightColors = lightColorScheme(
    primary = Color(0xFF3D5AFE),
    onPrimary = Color.White,
    surface = Color(0xFFFCFCFF),
    background = Color(0xFFF7F7FB),
    surfaceVariant = Color(0xFFE8E8F0),
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFF9FB0FF),
    onPrimary = Color(0xFF12225C),
    surface = Color(0xFF131318),
    background = Color(0xFF0C0C10),
    surfaceVariant = Color(0xFF2A2A33),
)

@Composable
fun SujiuTheme(mode: AppearanceMode, content: @Composable () -> Unit) {
    val dark = when (mode) {
        AppearanceMode.System -> isSystemInDarkTheme()
        AppearanceMode.Light -> false
        AppearanceMode.Dark -> true
    }
    MaterialTheme(
        colorScheme = if (dark) DarkColors else LightColors,
        content = content,
    )
}
