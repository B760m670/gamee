package com.spiritchat.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * A circular avatar. No image loading yet — there is no user data to load
 * an image *of* until the identity/session work lands — so this is always
 * the initials-on-a-colored-circle fallback for now, matching what the iOS
 * Avatar component shows before a profile photo exists.
 */
@Composable
fun Avatar(username: String?, size: Dp = 40.dp, modifier: Modifier = Modifier) {
    val initials = username?.take(2)?.uppercase() ?: "?"
    Box(
        modifier = modifier
            .size(size)
            .clip(CircleShape)
            .background(Color(0xFF27272A)),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = initials,
            color = Color(0xFFA1A1AA),
            fontWeight = FontWeight.SemiBold,
            fontSize = (size.value * 0.35f).sp,
        )
    }
}
