package com.spiritchat.update

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Accent = Color(0xFF2F7BFF)
private val CardBg = Color(0xF21C1C20)
private val Muted = Color(0xFF8E8E93)

private fun mb(bytes: Long): String = String.format("%.1f", bytes / 1048576f)

@Composable
fun UpdateOverlay(vm: UpdateViewModel) {
    val active =
        vm.status == UpdateStatus.Downloading ||
            vm.status == UpdateStatus.Paused ||
            vm.status == UpdateStatus.Error
    Box(
        Modifier.fillMaxSize().statusBarsPadding().padding(12.dp),
        contentAlignment = Alignment.TopCenter,
    ) {
        when {
            active -> ProgressCard(vm)
            vm.info != null -> BannerCard(vm)
        }
    }
}

@Composable
private fun BannerCard(vm: UpdateViewModel) {
    val v = vm.info ?: return
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(CardBg).padding(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text("Доступно обновление", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Bold)
            Text("Версия ${v.versionName}", color = Muted, fontSize = 13.sp)
        }
        TextButton(onClick = { vm.dismiss() }) { Text("Позже", color = Muted) }
        Button(
            onClick = { vm.download() },
            colors = ButtonDefaults.buttonColors(containerColor = Accent),
        ) {
            Text("Обновить", color = Color.White)
        }
    }
}

@Composable
private fun ProgressCard(vm: UpdateViewModel) {
    val pct = if (vm.totalBytes > 0) (vm.bytesDownloaded.toFloat() / vm.totalBytes) else 0f
    val label =
        when {
            vm.status == UpdateStatus.Error -> "Ошибка загрузки"
            vm.status == UpdateStatus.Paused -> "Пауза — ждём сеть…"
            vm.totalBytes > 0 -> "${mb(vm.bytesDownloaded)} из ${mb(vm.totalBytes)} МБ"
            else -> "Подготовка…"
        }
    val barColor = if (vm.status == UpdateStatus.Error) Color(0xFFF87171) else Accent

    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(CardBg).padding(14.dp)) {
        Text("Обновление", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(10.dp))
        Box(
            Modifier.fillMaxWidth().height(6.dp).clip(RoundedCornerShape(3.dp)).background(Color(0x33FFFFFF)),
        ) {
            if (pct > 0f) {
                Box(
                    Modifier.fillMaxWidth(pct.coerceIn(0.01f, 1f))
                        .height(6.dp)
                        .clip(RoundedCornerShape(3.dp))
                        .background(barColor),
                )
            }
        }
        Spacer(Modifier.height(8.dp))
        Text(label, color = Muted, fontSize = 13.sp)
    }
}
