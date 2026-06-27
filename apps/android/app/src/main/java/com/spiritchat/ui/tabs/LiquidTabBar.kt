package com.spiritchat.ui.tabs

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Accent = Color(0xFF2F7BFF)
private val Inactive = Color(0xFF8E8E93)
private val BarHeight = 64.dp
private val BarRadius = 32.dp

/**
 * Liquid-Glass-style tab bar: a floating translucent capsule with a sliding
 * pill that marks the selected tab. (Real frosted blur via Haze comes next;
 * for now the glass is approximated with translucency.)
 */
@Composable
fun LiquidTabBar(
    selected: TabItem,
    onSelect: (TabItem) -> Unit,
    modifier: Modifier = Modifier,
) {
    val tabs = TabItem.entries
    val selectedIndex = tabs.indexOf(selected)

    BoxWithConstraints(
        modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = 14.dp)
            .padding(bottom = 10.dp),
    ) {
        val itemWidth = maxWidth / tabs.size
        val pillOffset by animateDpAsState(
            targetValue = itemWidth * selectedIndex,
            label = "pillOffset",
        )

        Box(
            Modifier
                .fillMaxWidth()
                .height(BarHeight)
                .clip(RoundedCornerShape(BarRadius))
                .background(Color(0x1FFFFFFF))
                .border(0.7.dp, Color(0x40FFFFFF), RoundedCornerShape(BarRadius)),
        ) {
            // Sliding selection pill.
            Box(
                Modifier
                    .offset(x = pillOffset)
                    .width(itemWidth)
                    .fillMaxHeight()
                    .padding(6.dp)
                    .clip(RoundedCornerShape(BarRadius - 6.dp))
                    .background(Color(0x382F7BFF))
                    .border(0.7.dp, Color(0x662F7BFF), RoundedCornerShape(BarRadius - 6.dp)),
            )

            Row(Modifier.fillMaxSize()) {
                tabs.forEach { tab ->
                    val isSelected = tab == selected
                    val tint = if (isSelected) Color.White else Inactive
                    Column(
                        Modifier
                            .weight(1f)
                            .fillMaxHeight()
                            .clickable(
                                interactionSource = remember { MutableInteractionSource() },
                                indication = null,
                            ) { onSelect(tab) },
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
                    ) {
                        Icon(
                            imageVector = if (isSelected) tab.selectedIcon else tab.icon,
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
