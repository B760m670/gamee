package com.spiritchat.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val SpiritDarkColors =
    darkColorScheme(
        primary = Color(0xFF2F7BFF),
        background = Color(0xFF000000),
        surface = Color(0xFF1C1C1E),
        onPrimary = Color(0xFFFFFFFF),
        onBackground = Color(0xFFFFFFFF),
        onSurface = Color(0xFFFFFFFF),
    )

@Composable
fun SpiritChatTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = SpiritDarkColors, content = content)
}
