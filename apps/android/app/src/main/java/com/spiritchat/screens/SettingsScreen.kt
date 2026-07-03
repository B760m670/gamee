package com.spiritchat.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.spiritchat.ui.components.Avatar
import com.spiritchat.ui.components.SettingsRow

/**
 * Profile placeholder + settings rows. There is no persisted identity yet
 * (see [com.spiritchat.crypto.CryptoCore] — it only proves the native
 * crypto core loads, it doesn't hold a real per-account key), so this
 * shows an empty profile rather than a QR/fingerprint that would change
 * every time the screen recomposes.
 */
@Composable
fun SettingsScreen() {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
            .padding(horizontal = 16.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 24.dp, bottom = 28.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Avatar(username = null, size = 100.dp)
            Text(
                text = "Без имени",
                color = Color.White,
                fontSize = 28.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(top = 14.dp),
            )
        }

        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            SettingsRow(
                icon = Icons.Filled.Lock,
                iconTint = Color(0xFF8B5CF6),
                label = "Конфиденциальность",
                onClick = {},
            )
        }
    }
}
