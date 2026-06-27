package com.spiritchat.ui.tabs

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Chat
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.Call
import androidx.compose.material.icons.outlined.Chat
import androidx.compose.material.icons.outlined.People
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.ui.graphics.vector.ImageVector

// The four tabs, in order. Chats is the default.
enum class TabItem(
    val label: String,
    val selectedIcon: ImageVector,
    val icon: ImageVector,
) {
    Contacts("Контакты", Icons.Filled.People, Icons.Outlined.People),
    Calls("Звонки", Icons.Filled.Call, Icons.Outlined.Call),
    Chats("Чаты", Icons.Filled.Chat, Icons.Outlined.Chat),
    Settings("Настройки", Icons.Filled.Settings, Icons.Outlined.Settings),
}
