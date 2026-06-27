package com.spiritchat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
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
                Box(Modifier.fillMaxSize()) {
                    HomeScreen()
                    UpdateOverlay(updateVm)
                }
            }
        }
    }
}

@Composable
fun HomeScreen() {
    Surface(modifier = Modifier.fillMaxSize(), color = Color(0xFF000000)) {
        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                text = "SpiritChat",
                color = Color.White,
                fontSize = 40.sp,
                fontWeight = FontWeight.ExtraBold,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                text = "Kotlin · Compose · P2P · No cloud",
                color = Color(0xFF8E8E93),
                fontSize = 15.sp,
            )
        }
    }
}
