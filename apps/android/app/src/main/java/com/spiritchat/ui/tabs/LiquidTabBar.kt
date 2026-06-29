package com.spiritchat.ui.tabs

import android.graphics.RuntimeShader
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

private val Inactive = Color(0xFF8E8E93)
private val BarHeight = 64.dp
private val BarRadius = 32.dp

// AGSL glass shader for the selection drop: a clear (mostly transparent) white
// glass with a top sheen and a soft specular band that can track the finger.
private const val GLASS_SHADER = """
uniform float2 iResolution;
uniform float iHighlight;
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution;
    float a = 0.06;
    a += (1.0 - smoothstep(0.0, 0.5, uv.y)) * 0.16;
    a += (1.0 - smoothstep(0.0, 0.22, abs(uv.x - iHighlight))) * 0.20;
    return half4(half3(a), half(a));
}
"""

/**
 * Liquid-Glass-style tab bar (iteration 1, no shaders yet):
 *  - the selection drop is clear glass (no colour fill);
 *  - it is dragged with the finger across the whole bar, snapping on release;
 *  - icons whiten by proximity as the drop passes over them;
 *  - a light haptic tick fires when the drop crosses a tab boundary.
 * Real frosted blur + AGSL refraction/metaball come in iteration 2.
 */
@Composable
fun LiquidTabBar(
    selected: TabItem,
    onSelect: (TabItem) -> Unit,
    modifier: Modifier = Modifier,
) {
    val tabs = TabItem.entries
    val count = tabs.size
    val selectedIndex = tabs.indexOf(selected)

    val density = LocalDensity.current
    val haptic = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()

    // Build the glass shader once; fall back gracefully if it fails to compile.
    val dropShader = remember { runCatching { RuntimeShader(GLASS_SHADER) }.getOrNull() }
    val dropBrush = remember(dropShader) { dropShader?.let { ShaderBrush(it) } }

    BoxWithConstraints(
        modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = 14.dp)
            .padding(bottom = 10.dp),
    ) {
        val itemWidth = maxWidth / count
        val itemPx = with(density) { itemWidth.toPx() }
        val maxX = itemPx * (count - 1)

        val dropX = remember { Animatable(0f) }
        var dragging by remember { mutableStateOf(false) }
        var lastBoundary by remember { mutableIntStateOf(selectedIndex) }

        // Keep the drop on the selected tab when not being dragged.
        LaunchedEffect(selectedIndex, itemPx) {
            if (!dragging && itemPx > 0f) {
                dropX.animateTo(
                    selectedIndex * itemPx,
                    spring(dampingRatio = 0.55f, stiffness = Spring.StiffnessMedium),
                )
            }
        }

        Box(
            Modifier
                .fillMaxWidth()
                .height(BarHeight)
                .clip(RoundedCornerShape(BarRadius))
                .background(Color(0x14FFFFFF))
                .pointerInput(itemPx, count) {
                    if (itemPx <= 0f) return@pointerInput
                    detectDragGestures(
                        onDragStart = {
                            dragging = true
                            lastBoundary = (dropX.value / itemPx).roundToInt()
                        },
                        onDragEnd = {
                            val target = (dropX.value / itemPx).roundToInt().coerceIn(0, count - 1)
                            onSelect(tabs[target])
                            dragging = false
                            scope.launch {
                                dropX.animateTo(
                                    target * itemPx,
                                    spring(dampingRatio = 0.45f, stiffness = Spring.StiffnessMedium),
                                )
                            }
                        },
                        onDragCancel = { dragging = false },
                    ) { change, drag ->
                        change.consume()
                        val next = (dropX.value + drag.x).coerceIn(0f, maxX)
                        scope.launch { dropX.snapTo(next) }
                        val b = (next / itemPx).roundToInt()
                        if (b != lastBoundary) {
                            lastBoundary = b
                            haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        }
                    }
                }
                .pointerInput(itemPx, count) {
                    if (itemPx <= 0f) return@pointerInput
                    detectTapGestures { pos ->
                        val target = (pos.x / itemPx).toInt().coerceIn(0, count - 1)
                        onSelect(tabs[target])
                    }
                },
        ) {
            // Clear "glass" drop.
            Box(
                Modifier
                    .offset { IntOffset(dropX.value.roundToInt(), 0) }
                    .width(itemWidth)
                    .fillMaxHeight()
                    .padding(6.dp)
                    .clip(RoundedCornerShape(BarRadius - 6.dp))
                    .drawBehind {
                        val s = dropShader
                        val b = dropBrush
                        if (s != null && b != null) {
                            s.setFloatUniform("iResolution", size.width, size.height)
                            s.setFloatUniform("iHighlight", 0.5f)
                            drawRect(b)
                        } else {
                            drawRect(Color(0x14FFFFFF))
                        }
                    }
                    .border(0.8.dp, Color(0x4DFFFFFF), RoundedCornerShape(BarRadius - 6.dp)),
            )

            Row(Modifier.fillMaxSize()) {
                tabs.forEachIndexed { index, tab ->
                    // Proximity: 1 when the drop is centred on this tab, 0 when a
                    // full item away — so icons whiten as the drop passes by.
                    val distance = abs(index * itemPx - dropX.value)
                    val nearness = (1f - (distance / itemPx)).coerceIn(0f, 1f)
                    val tint = lerp(Inactive, Color.White, nearness)
                    val isOn = nearness > 0.5f
                    Column(
                        Modifier.width(itemWidth).fillMaxHeight(),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
                    ) {
                        Icon(
                            imageVector = if (isOn) tab.selectedIcon else tab.icon,
                            contentDescription = tab.label,
                            tint = tint,
                            modifier = Modifier.height(24.dp).width(24.dp),
                        )
                        Spacer(Modifier.height(3.dp))
                        Text(tab.label, color = tint, fontSize = 11.sp)
                    }
                }
            }
        }
    }
}
