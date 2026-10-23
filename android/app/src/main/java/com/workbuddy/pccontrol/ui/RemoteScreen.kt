package com.workbuddy.pccontrol.ui

import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.CleaningServices
import androidx.compose.material.icons.rounded.Bolt
import androidx.compose.material.icons.rounded.ContentPaste
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.Monitor
import androidx.compose.material.icons.rounded.PowerSettingsNew
import androidx.compose.material.icons.rounded.UploadFile
import androidx.compose.material.icons.rounded.VolumeDown
import androidx.compose.material.icons.rounded.VolumeOff
import androidx.compose.material.icons.rounded.VolumeUp
import androidx.compose.material.icons.rounded.WbSunny
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.workbuddy.pccontrol.Session
import com.workbuddy.pccontrol.data.Store
import com.workbuddy.pccontrol.net.PcApi
import com.workbuddy.pccontrol.net.PcStatus
import com.workbuddy.pccontrol.net.Wol
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** 危险操作的强调色（关机） */
private val Danger = Color(0xFFD9483B)
private val DangerSoft = Color(0xFFFDF1EF)
private val DangerLine = Color(0xFFF2D5D1)

@Composable
fun RemoteScreen(
    session: Session,
    store: Store,
    onExit: () -> Unit,
    onScreen: () -> Unit,
    onClipboard: () -> Unit,
) {

    val api = remember(session) { PcApi(session.host, session.port, session.pin) }
    val scope = rememberCoroutineScope()

    var status by remember { mutableStateOf<PcStatus?>(null) }
    var online by remember { mutableStateOf(true) }
    var volDrag by remember { mutableStateOf<Float?>(null) }
    var briDrag by remember { mutableStateOf<Float?>(null) }
    var notice by remember { mutableStateOf("") }
    var confirmShutdown by remember { mutableStateOf(false) }
    var confirmTidy by remember { mutableStateOf(false) }
    var uploading by remember { mutableStateOf(false) }
    val context = LocalContext.current

    // 选完文件直接把流怼到电脑上，不经过内存
    val picker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri ->
        if (uri != null) {
            scope.launch {
                uploading = true
                notice = "正在上传…"
                try {
                    val resolver = context.contentResolver
                    var name = ""
                    var size = -1L
                    resolver.query(uri, null, null, null, null)?.use { c ->
                        if (c.moveToFirst()) {
                            val ni = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                            if (ni >= 0) name = c.getString(ni).orEmpty()
                            val si = c.getColumnIndex(OpenableColumns.SIZE)
                            if (si >= 0 && !c.isNull(si)) size = c.getLong(si)
                        }
                    }
                    if (name.isBlank()) {
                        name = uri.lastPathSegment?.substringAfterLast('/') ?: "upload.bin"
                    }
                    val saved = api.upload(name, size) {
                        resolver.openInputStream(uri)
                            ?: throw java.io.IOException("读不到这个文件")
                    }
                    notice = "已传到电脑：$saved"
                } catch (e: Exception) {
                    notice = e.message ?: "上传失败"
                } finally {
                    uploading = false
                }
            }
        }
    }

    // 每 2 秒同步一次电脑状态
    LaunchedEffect(session) {
        while (true) {
            try {
                status = api.status()
                online = true
            } catch (_: Exception) {
                online = false
            }
            delay(2_000)
        }
    }

    fun act(errorText: String = "操作失败", block: suspend () -> Unit) {
        scope.launch {
            try {
                block()
                status = runCatching { api.status() }.getOrNull()
                online = true
                notice = ""
            } catch (e: Exception) {
                online = false
                notice = e.message ?: errorText
            }
        }
    }

    val vol = volDrag ?: (status?.volume ?: 0).toFloat()
    val bri = briDrag ?: (status?.brightness ?: 0).toFloat()

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp),
    ) {
        Spacer(Modifier.height(14.dp))

        // ---------- 顶部 ----------
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onExit, modifier = Modifier.size(32.dp)) {
                Icon(Icons.Rounded.ArrowBack, contentDescription = "返回", tint = TextSub)
            }
            Spacer(Modifier.size(4.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    status?.name?.ifBlank { store.pcName } ?: store.pcName.ifBlank { "电脑" },
                    fontSize = 17.sp, fontWeight = FontWeight.Bold, color = TextMain,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(6.dp)
                            .background(if (online) OkGreen else Color(0xFFE05B5B), CircleShape)
                    )
                    Spacer(Modifier.size(5.dp))
                    Text(
                        if (online) "${session.host} · 已连接" else "连接断开，正在重试…",
                        fontSize = 11.sp, color = if (online) TextSub else Color(0xFFE05B5B),
                    )
                }
            }
            // 看电脑画面
            Surface(
                onClick = onScreen,
                shape = RoundedCornerShape(10.dp),
                color = AccentSoft,
                border = BorderStroke(1.dp, Color(0xFFC6D6FA)),
            ) {
                Row(
                    Modifier.padding(horizontal = 9.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        Icons.Rounded.Monitor, contentDescription = "看电脑画面",
                        tint = Accent, modifier = Modifier.size(15.dp),
                    )
                    Spacer(Modifier.size(4.dp))
                    Text("看画面", fontSize = 11.5.sp, color = Accent, fontWeight = FontWeight.Medium)
                }
            }
        }

        Spacer(Modifier.height(12.dp))

        // ---------- 音量 ----------
        RemoteCard {
            Box(Modifier.fillMaxWidth()) {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.Bottom,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("音量", fontSize = 13.sp, color = TextSub, fontWeight = FontWeight.Medium)
                    BigValue((status?.volume ?: 0).toString())
                }
            }
            ThinSlider(
                value = vol,
                onValueChange = { volDrag = it },
                onValueChangeFinished = {
                    val v = vol.toInt()
                    volDrag = null
                    act { api.setVolume(v) }
                },
                enabled = status?.canVolume != false,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                CompactAction(
                    Icons.Rounded.VolumeDown, "音量减", Modifier.weight(1f),
                ) { act { api.stepVolume(-5) } }
                CompactAction(
                    if (status?.muted == true) Icons.Rounded.VolumeOff else Icons.Rounded.VolumeUp,
                    if (status?.muted == true) "取消静音" else "静音",
                    Modifier.weight(1f),
                    active = status?.muted == true,
                ) { act { api.toggleMute() } }
                CompactAction(Icons.Rounded.VolumeUp, "音量加", Modifier.weight(1f)) { act { api.stepVolume(5) } }
            }
        }

        Spacer(Modifier.height(10.dp))

        // ---------- 亮度 ----------
        RemoteCard {
            Box(Modifier.fillMaxWidth()) {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.Bottom,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("亮度", fontSize = 13.sp, color = TextSub, fontWeight = FontWeight.Medium)
                    BigValue((status?.brightness ?: 0).toString())
                }
            }
            ThinSlider(
                value = bri,
                onValueChange = { briDrag = it },
                onValueChangeFinished = {
                    val v = bri.toInt()
                    briDrag = null
                    act { api.setBrightness(v) }
                },
                enabled = status?.brightness != null,
            )
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Rounded.WbSunny, null, tint = TextSub, modifier = Modifier.size(14.dp))
                    Spacer(Modifier.size(5.dp))
                    Chip(modeText(status?.brightnessMode))
                }
                if (status?.hdr == true) Chip("HDR 开", color = Violet, bg = Color(0xFFF4EFFC))
            }

            if (status?.hdr == true && status?.brightnessMode != "ddc") {
                Spacer(Modifier.height(8.dp))
                Surface(
                    shape = RoundedCornerShape(10.dp),
                    color = WarnSoft,
                    border = BorderStroke(1.dp, Color(0xFFF4E3C4)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(horizontal = 11.dp, vertical = 8.dp)) {
                        Text(
                            "HDR 开启时显示器会锁死背光，亮度只能走软件通道。关掉 HDR 即可恢复真实背光调节。",
                            fontSize = 11.sp, color = Color(0xFF8A6120), lineHeight = 15.sp,
                        )
                        TextButton(
                            onClick = { act("关闭 HDR 失败") { api.setHdr(false) } },
                            contentPadding = androidx.compose.foundation.layout.PaddingValues(0.dp),
                        ) {
                            Text("关闭 HDR，启用真实背光", fontSize = 12.sp, color = Accent, fontWeight = FontWeight.Medium)
                        }
                    }
                }
            }
            Hint(status?.brightnessReason.orEmpty())
        }

        Spacer(Modifier.height(10.dp))

        // ---------- 电源 ----------
        RemoteCard {
            Text("电源", fontSize = 13.sp, color = TextSub, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(10.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PowerAction(Icons.Rounded.Lock, "锁屏", Modifier.weight(1f)) { act { api.power("lock") } }
                PowerAction(Icons.Rounded.Monitor, "关屏", Modifier.weight(1f)) { act { api.power("screen_off") } }
                PowerAction(Icons.Rounded.Bedtime, "休眠", Modifier.weight(1f)) { act { api.power("sleep") } }
                PowerAction(Icons.Rounded.Bolt, "唤醒", Modifier.weight(1f)) {
                    val mac = status?.mac?.takeIf { it.isNotBlank() } ?: store.mac
                    if (mac.isBlank()) notice = "还没拿到电脑网卡地址，先成功连接一次"
                    else if (!Wol.wake(mac)) notice = "唤醒包发送失败"
                    else notice = "已发送唤醒指令"
                }
                PowerAction(Icons.Rounded.PowerSettingsNew, "关机", Modifier.weight(1f), danger = true) {
                    confirmShutdown = true
                }
            }
        }

        Spacer(Modifier.height(10.dp))

        // ---------- 传输 ----------
        RemoteCard {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ToolAction(Icons.Rounded.UploadFile, "传文件", Modifier.weight(1f)) {
                    picker.launch(arrayOf("*/*"))
                }
                ToolAction(Icons.Rounded.ContentPaste, "剪贴板", Modifier.weight(1f), onClick = onClipboard)
                ToolAction(Icons.Rounded.CleaningServices, "整理桌面", Modifier.weight(1f)) {
                    confirmTidy = true
                }
            }
        }

        if (notice.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Text(notice, fontSize = 11.5.sp, color = WarnAmber, modifier = Modifier.padding(horizontal = 4.dp))
        }

        Spacer(Modifier.height(24.dp))
    }

    if (confirmShutdown) {
        AlertDialog(
            onDismissRequest = { confirmShutdown = false },
            title = { Text("关闭电脑？", fontSize = 17.sp, fontWeight = FontWeight.Bold, color = TextMain) },
            text = {
                Text(
                    "会给 ${status?.name?.ifBlank { session.host } ?: session.host} 发送关机指令，正在运行的程序会被结束。",
                    fontSize = 13.sp, color = TextSub, lineHeight = 19.sp,
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmShutdown = false
                    // 指令发出后电脑会立刻断电，界面随后会显示连接断开，这是预期行为
                    act("关机失败") { api.power("shutdown") }
                }) {
                    Text("关机", color = Danger, fontWeight = FontWeight.SemiBold)
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmShutdown = false }) {
                    Text("取消", color = TextSub)
                }
            },
            shape = RoundedCornerShape(20.dp),
            containerColor = CardBg,
        )
    }

    if (confirmTidy) {
        AlertDialog(
            onDismissRequest = { confirmTidy = false },
            title = { Text("整理电脑桌面？", fontSize = 17.sp, fontWeight = FontWeight.Bold, color = TextMain) },
            text = {
                Text(
                    "把桌面上散着的文件按类型归类（游戏 / 软件 / 文档 / 图片 …），文件夹本身不动。整理完可以在电脑面板上「撤销上次」还原。",
                    fontSize = 13.sp, color = TextSub, lineHeight = 19.sp,
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmTidy = false
                    scope.launch {
                        notice = try {
                            val r = api.tidyDesktop()
                            val n = r.optInt("moved", 0)
                            if (r.optBoolean("ok")) "桌面已整理，归类 $n 个文件"
                            else r.optString("error", "整理失败")
                        } catch (e: Exception) {
                            e.message ?: "整理失败"
                        }
                    }
                }) {
                    Text("整理", color = Accent, fontWeight = FontWeight.SemiBold)
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmTidy = false }) {
                    Text("取消", color = TextSub)
                }
            },
            shape = RoundedCornerShape(20.dp),
            containerColor = CardBg,
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
/** 细轨道 + 圆钮的滑块，比 Material3 默认样式更紧凑，也不会因为压高度把圆钮裁掉 */
@Composable
private fun ThinSlider(
    value: Float,
    onValueChange: (Float) -> Unit,
    onValueChangeFinished: () -> Unit,
    enabled: Boolean,
) {
    Slider(
        value = value,
        onValueChange = onValueChange,
        onValueChangeFinished = onValueChangeFinished,
        valueRange = 0f..100f,
        enabled = enabled,
        modifier = Modifier
            .fillMaxWidth()
            .height(26.dp),
        thumb = {
            Box(
                Modifier
                    .size(18.dp)
                    .shadow(2.dp, CircleShape)
                    .background(Color.White, CircleShape)
                    .border(2.5.dp, if (enabled) Accent else Color(0xFFC9CDD4), CircleShape)
            )
        },
        track = { state ->
            val span = state.valueRange.endInclusive - state.valueRange.start
            val fraction = if (span <= 0f) 0f
            else ((state.value - state.valueRange.start) / span).coerceIn(0f, 1f)
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(7.dp)
                    .clip(RoundedCornerShape(4.dp))
                    .background(Color(0xFFECEEF2))
            ) {
                Box(
                    Modifier
                        .fillMaxWidth(fraction)
                        .fillMaxHeight()
                        .background(if (enabled) Accent else Color(0xFFC9CDD4))
                )
            }
        },
    )
}

private fun modeText(mode: String?): String = when (mode) {
    "ddc" -> "真实背光 · DDC/CI"
    "wmi" -> "系统背光 · WMI"
    "sdr" -> "HDR SDR 白点"
    "soft" -> "软件调光"
    else -> "亮度"
}

@Composable
private fun Chip(text: String, color: Color = TextSub, bg: Color = Color(0xFFF0F2F5)) {
    Box(
        Modifier
            .background(bg, RoundedCornerShape(6.dp))
            .padding(horizontal = 7.dp, vertical = 2.dp)
    ) {
        Text(text, fontSize = 10.sp, color = color)
    }
}

/** 音量卡片里的紧凑操作按钮 */
@Composable
private fun CompactAction(
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    active: Boolean = false,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.height(38.dp),
        shape = RoundedCornerShape(11.dp),
        color = if (active) AccentSoft else Color(0xFFFAFBFC),
        border = BorderStroke(1.dp, if (active) Color(0xFFC6D6FA) else Line),
    ) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = label, tint = if (active) Accent else TextMain, modifier = Modifier.size(18.dp))
        }
    }
}

/** 传输区的一行按钮 */
@Composable
private fun ToolAction(
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.height(44.dp),
        shape = RoundedCornerShape(12.dp),
        color = Color(0xFFFAFBFC),
        border = BorderStroke(1.dp, Line),
    ) {
        Row(
            Modifier.fillMaxSize(),
            horizontalArrangement = Arrangement.Center,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = label, tint = Accent, modifier = Modifier.size(17.dp))
            Spacer(Modifier.size(6.dp))
            Text(label, fontSize = 12.sp, color = TextMain, fontWeight = FontWeight.Medium)
        }
    }
}

/** 电源区的方形图标按钮 */
@Composable
private fun PowerAction(
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    danger: Boolean = false,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.height(54.dp),
        shape = RoundedCornerShape(13.dp),
        color = if (danger) DangerSoft else Color(0xFFFAFBFC),
        border = BorderStroke(1.dp, if (danger) DangerLine else Line),
    ) {
        Column(
            Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Icon(
                icon, contentDescription = label,
                tint = if (danger) Danger else TextMain,
                modifier = Modifier.size(18.dp),
            )
            Spacer(Modifier.height(3.dp))
            Text(label, fontSize = 10.sp, color = if (danger) Danger else TextSub)
        }
    }
}
