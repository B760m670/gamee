package com.spiritchat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.lifecycle.viewmodel.compose.viewModel
import com.spiritchat.screens.CallsScreen
import com.spiritchat.screens.ChatsScreen
import com.spiritchat.screens.ContactsScreen
import com.spiritchat.screens.SettingsScreen
import com.spiritchat.ui.tabs.LiquidTabBar
import com.spiritchat.ui.tabs.TabItem
import com.spiritchat.ui.theme.SpiritChatTheme
import com.spiritchat.update.UpdateOverlay
import com.spiritchat.update.UpdateViewModel

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            SpiritChatTheme {
                val updateVm: UpdateViewModel = viewModel()
                LaunchedEffect(Unit) { updateVm.checkForUpdate() }

                // Chats is the default tab.
                var selectedTab by remember { mutableStateOf(TabItem.Chats) }

                Box(Modifier.fillMaxSize().background(Color(0xFF000000))) {
                    when (selectedTab) {
                        TabItem.Contacts -> ContactsScreen()
                        TabItem.Calls -> CallsScreen()
                        TabItem.Chats -> ChatsScreen()
                        TabItem.Settings -> SettingsScreen()
                    }

                    LiquidTabBar(
                        selected = selectedTab,
                        onSelect = { selectedTab = it },
                        modifier = Modifier.align(Alignment.BottomCenter),
                    )

                    UpdateOverlay(updateVm)
                }
            }
        }
    }
}
