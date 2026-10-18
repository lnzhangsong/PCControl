package com.workbuddy.pccontrol.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.Send
import androidx.compose.material.icons.rounded.Smartphone
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.workbuddy.pccontrol.Session
import com.workbuddy.pccontrol.net.PcApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * 剪贴板历史记录：点进来直接看到两边复制过的东西，按时间倒序。
 * 点任意一条就复制回手机（同时自动同步到电脑），左下的垃圾桶删单条。
 */
@Composable
fun ClipboardView(session: Session, onBack: () -> Unit) {

    val api = remember(session) { PcApi(session.host, session.port, session.pin) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var items by remember { mutableStateOf(ClipboardHistory.list()) }
    var draft by remember { mutableStateOf("") }
    var msg by remember { mutableStateOf("已复制，可直接粘贴") }
    var msgIsError by remember { mutableStateOf(false) }

    fun flash(text: String, error: Boolean = false) {
        msg = text
        msgIsError = error
    }

    fun copy(text: String) {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        cm.setPrimaryClip(ClipData.newPlainText("PC Remote", text))
        flash("已复制，可直接粘贴")
    }

    fun sendToPc() {
        val text = draft.trim()
        if (text.isEmpty()) {
            flash("先输入点内容", true)
            return
        }
        scope.launch {
            try {
                api.writeClipboard(text)
                ClipboardHistory.add(text, fromPc = false)
                draft = ""
                items = ClipboardHistory.list()
                flash("已发送到电脑")
            } catch (e: Exception) {
                flash(e.message ?: "发送失败", true)
            }
        }
    }

    // 自动同步随时可能新增记录，这里跟着刷新列表
    LaunchedEffect(Unit) {
        while (true) {
            items = ClipboardHistory.list()
            delay(800)
        }
    }

    Column(Modifier.fillMaxSize()) {

        Spacer(Modifier.height(14.dp))

        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack, modifier = Modifier.size(32.dp)) {
                Icon(Icons.Rounded.ArrowBack, "返回", tint = TextSub)
            }
            Spacer(Modifier.size(2.dp))
            Column(Modifier.weight(1f)) {
                Text("剪贴板", fontSize = 17.sp, fontWeight = FontWeight.Bold, color = TextMain)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(6.dp).background(OkGreen, CircleShape))
                    Spacer(Modifier.size(5.dp))
                    Text(
                        "自动同步中 · 共 ${items.size} 条",
                        fontSize = 11.sp,
                        color = TextSub,
                    )
                }
            }
            if (items.isNotEmpty()) {
                TextButton(onClick = {
                    ClipboardHistory.clear()
                    items = ClipboardHistory.list()
                    flash("已清空历史")
                }) {
                    Text("清空", fontSize = 12.5.sp, color = TextSub)
                }
            }
        }

        Spacer(Modifier.height(8.dp))

        if (items.isEmpty()) {
            Box(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(
                        Icons.Rounded.ContentCopy, null,
                        tint = Line, modifier = Modifier.size(40.dp),
                    )
                    Spacer(Modifier.height(10.dp))
                    Text("还没有记录", fontSize = 13.sp, color = TextSub)
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "在手机或电脑上复制任意文字，会自动出现在这里",
                        fontSize = 11.5.sp,
                        color = TextSub,
                    )
                }
            }
        } else {
            LazyColumn(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = 14.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(items, key = { it.id }) { item ->
                    HistoryRow(
                        item = item,
                        onCopy = { copy(item.text) },
                        onDelete = {
                            ClipboardHistory.remove(item.id)
                            items = ClipboardHistory.list()
                            flash("已删除")
                        },
                    )
                }
                item { Spacer(Modifier.height(6.dp)) }
            }
        }

        // ---------- 手动发一条到电脑 ----------
        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp)
                .padding(bottom = 12.dp, top = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                modifier = Modifier.weight(1f).heightIn(min = 48.dp, max = 96.dp),
                placeholder = { Text("发一条到电脑…", fontSize = 12.5.sp, color = TextSub) },
                textStyle = androidx.compose.ui.text.TextStyle(fontSize = 12.5.sp, color = TextMain),
                shape = RoundedCornerShape(12.dp),
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = Color.White,
                    unfocusedContainerColor = Color.White,
                    focusedIndicatorColor = Accent,
                    unfocusedIndicatorColor = Line,
                ),
            )
            Spacer(Modifier.width(8.dp))
            Surface(
                onClick = { sendToPc() },
                modifier = Modifier.size(48.dp),
                shape = RoundedCornerShape(12.dp),
                color = Accent,
            ) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Icon(Icons.Rounded.Send, null, tint = Color.White, modifier = Modifier.size(19.dp))
                }
            }
        }

        if (msg.isNotEmpty()) {
            Box(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 14.dp)
                    .padding(bottom = 10.dp)
                    .background(
                        if (msgIsError) Color(0xFFFDF1EF) else AccentSoft,
                        RoundedCornerShape(10.dp),
                    )
                    .padding(horizontal = 12.dp, vertical = 8.dp)
            ) {
                Text(
                    msg,
                    fontSize = 11.5.sp,
                    color = if (msgIsError) Color(0xFFD9483B) else Accent,
                )
            }
        }
    }
}

@Composable
private fun HistoryRow(item: ClipItem, onCopy: () -> Unit, onDelete: () -> Unit) {
    Surface(
        onClick = onCopy,
        shape = RoundedCornerShape(14.dp),
        color = Color.White,
        border = BorderStroke(1.dp, Line),
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    if (item.fromPc) Icons.Rounded.Computer else Icons.Rounded.Smartphone,
                    null,
                    tint = if (item.fromPc) Accent else OkGreen,
                    modifier = Modifier.size(14.dp),
                )
                Spacer(Modifier.size(5.dp))
                Text(
                    if (item.fromPc) "电脑 → 手机" else "手机 → 电脑",
                    fontSize = 11.sp,
                    color = TextSub,
                )
                Spacer(Modifier.weight(1f))
                Text(fmtTime(item.time), fontSize = 10.5.sp, color = TextSub)
                IconButton(onClick = onDelete, modifier = Modifier.size(26.dp)) {
                    Icon(
                        Icons.Rounded.DeleteOutline, "删除",
                        tint = TextSub, modifier = Modifier.size(15.dp),
                    )
                }
            }
            Spacer(Modifier.height(6.dp))
            Text(
                item.text,
                fontSize = 12.5.sp,
                color = TextMain,
                lineHeight = 17.sp,
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

private fun fmtTime(t: Long): String {
    val cal = java.util.Calendar.getInstance()
    val nowDay = cal.get(java.util.Calendar.DAY_OF_YEAR)
    cal.time = Date(t)
    val day = cal.get(java.util.Calendar.DAY_OF_YEAR)
    val hm = SimpleDateFormat("HH:mm", Locale.getDefault()).format(Date(t))
    return when {
        nowDay == day -> hm
        nowDay - day == 1 -> "昨天 $hm"
        else -> SimpleDateFormat("M-d HH:mm", Locale.getDefault()).format(Date(t))
    }
}
