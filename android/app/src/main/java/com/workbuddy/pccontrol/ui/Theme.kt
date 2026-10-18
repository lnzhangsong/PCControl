package com.workbuddy.pccontrol.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

val Bg = Color(0xFFF4F5F7)
val CardBg = Color(0xFFFFFFFF)
val Line = Color(0xFFE6E8EC)
val TextMain = Color(0xFF14181F)
val TextSub = Color(0xFF7B8494)
val Accent = Color(0xFF3B6EF5)
val AccentSoft = Color(0xFFE8EEFC)
val OkGreen = Color(0xFF1FB673)
val WarnAmber = Color(0xFFE0902A)
val WarnSoft = Color(0xFFFDF4E6)
val Violet = Color(0xFF7B5CC4)

@Composable
fun AppTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = lightColorScheme(
            primary = Accent,
            onPrimary = Color.White,
            background = Bg,
            onBackground = TextMain,
            surface = CardBg,
            onSurface = TextMain,
            surfaceVariant = AccentSoft,
            outline = Line,
        ),
        content = content
    )
}

/** 白色圆角卡片 */
@Composable
fun RemoteCard(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        color = CardBg,
        border = androidx.compose.foundation.BorderStroke(1.dp, Line),
        shadowElevation = 1.dp,
    ) {
        Column(Modifier.padding(horizontal = 13.dp, vertical = 12.dp), content = content)
    }
}

@Composable
fun CardLabel(text: String, trailing: @Composable (() -> Unit)? = null) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, fontSize = 13.sp, color = TextSub, fontWeight = FontWeight.Medium)
        trailing?.invoke()
    }
}

@Composable
fun BigValue(value: String, unit: String = "%") {
    Row(verticalAlignment = Alignment.Bottom) {
        Text(value, fontSize = 25.sp, fontWeight = FontWeight.SemiBold, color = TextMain)
        Text(unit, fontSize = 12.sp, color = TextSub, modifier = Modifier.padding(start = 2.dp, bottom = 3.dp))
    }
}

@Composable
fun Hint(text: String, color: Color = TextSub) {
    if (text.isNotEmpty()) {
        Spacer(Modifier.height(8.dp))
        Text(text, fontSize = 11.5.sp, color = color, lineHeight = 16.sp)
    }
}
