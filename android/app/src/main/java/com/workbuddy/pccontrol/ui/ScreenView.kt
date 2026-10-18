package com.workbuddy.pccontrol.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.ProgressiveMediaSource
import androidx.media3.ui.AspectRatioFrameLayout
import androidx.media3.ui.PlayerView
import com.workbuddy.pccontrol.Session
import com.workbuddy.pccontrol.net.PcApi
import kotlinx.coroutines.delay

/**
 * 实时看电脑画面。
 *
 * 电脑端用 ffmpeg 的 gdigrab 抓屏 + NVENC 硬件编码成 H.264，
 * 封成 MPEG-TS 直接从 HTTP 流过来，手机用 ExoPlayer 硬解上屏。
 * 1080p30 在局域网里延迟约 0.3~0.8 秒，看视频不卡。
 */
@Composable
fun ScreenView(session: Session, onBack: () -> Unit) {

    val api = remember(session) { PcApi(session.host, session.port, session.pin) }
    val context = LocalContext.current

    var level by remember { mutableStateOf(1) } // 0 流畅 / 1 标准 / 2 清晰
    var ready by remember { mutableStateOf(false) }
    var notice by remember { mutableStateOf("正在启动串流…") }
    var retry by remember { mutableStateOf(0) }
    var fill by remember { mutableStateOf(false) } // 铺满屏幕（裁掉多余部分）
    var barsVisible by remember { mutableStateOf(true) } // 工具栏，闲置几秒后自动藏起来

    // 横屏 + 隐藏状态栏/导航栏，画面才能铺满
    DisposableEffect(Unit) {
        val act = context.findActivity()
        act?.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        val window = act?.window
        val controller = if (window != null) {
            WindowInsetsControllerCompat(window, window.decorView).also {
                it.hide(WindowInsetsCompat.Type.systemBars())
                it.systemBarsBehavior =
                    WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else null
        onDispose {
            controller?.show(WindowInsetsCompat.Type.systemBars())
            act?.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        }
    }

    val (width, bitrate, label) = when (level) {
        0 -> Triple(1280, 4000, "720p")
        2 -> Triple(2560, 14000, "1440p")
        else -> Triple(1920, 8000, "1080p")
    }

    val player = remember {
        val http = DefaultHttpDataSource.Factory()
            .setDefaultRequestProperties(mapOf("X-Pin" to api.authPin))
            .setConnectTimeoutMs(5_000)
            .setReadTimeoutMs(15_000)
            .setAllowCrossProtocolRedirects(true)
        ExoPlayer.Builder(context)
            .setMediaSourceFactory(ProgressiveMediaSource.Factory(http))
            // 直播流不能按点播那样攒两秒半再播，否则起播慢、还容易反复 rebuffer
            .setLoadControl(
                DefaultLoadControl.Builder()
                    .setBufferDurationsMs(800, 3_000, 180, 400)
                    .build()
            )
            .build()
            .apply {
                playWhenReady = true
                volume = 0f
            }
    }

    // 工具栏闲置 3.5 秒自动淡出，看视频时整块屏幕只剩画面
    LaunchedEffect(barsVisible, level, fill) {
        if (barsVisible) {
            delay(3_500)
            barsVisible = false
        }
    }

    // 换画质 / 重试：重建一遍源
    LaunchedEffect(level, retry) {
        ready = false
        notice = "正在启动串流…"
        player.stop()
        player.clearMediaItems()
        // DefaultExtractorsFactory 会自动识别 MPEG-TS
        val source = ProgressiveMediaSource.Factory(
            DefaultHttpDataSource.Factory()
                .setDefaultRequestProperties(mapOf("X-Pin" to api.authPin))
                .setConnectTimeoutMs(5_000)
                .setReadTimeoutMs(15_000)
        ).createMediaSource(MediaItem.fromUri(api.streamUrl(width, bitrate)))
        player.setMediaSource(source)
        player.prepare()
    }

    DisposableEffect(player) {
        val listener = object : Player.Listener {
            override fun onPlaybackStateChanged(state: Int) {
                when (state) {
                    Player.STATE_READY -> {
                        // 直播流会周期性地在 READY / BUFFERING 之间抖，出过画面就别再回退
                        ready = true
                        notice = ""
                    }
                    Player.STATE_ENDED -> notice = "串流已结束，点右上角重连"
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                ready = false
                notice = "串流中断：${error.errorCodeName}"
            }
        }
        player.addListener(listener)
        onDispose {
            player.removeListener(listener)
            player.release()
        }
    }

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        AndroidView(
            factory = { ctx ->
                PlayerView(ctx).apply {
                    useController = false
                    setShutterBackgroundColor(android.graphics.Color.BLACK)
                    this.player = player
                }
            },
            update = { view ->
                view.resizeMode = if (fill) {
                    AspectRatioFrameLayout.RESIZE_MODE_FILL
                } else {
                    AspectRatioFrameLayout.RESIZE_MODE_FIT
                }
            },
            modifier = Modifier.fillMaxSize(),
        )

        // 点画面开关工具栏，不显示水波纹
        Box(
            Modifier
                .fillMaxSize()
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                ) { barsVisible = !barsVisible }
        )

        if (!ready) {
            Column(
                Modifier.align(Alignment.Center),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                CircularProgressIndicator(
                    color = Color(0xFF6E8FE8),
                    strokeWidth = 2.dp,
                    modifier = Modifier.size(28.dp),
                )
                Spacer(Modifier.size(12.dp))
                Text(notice.ifEmpty { "连接中…" }, color = Color(0xFF9AA3B2), fontSize = 12.5.sp)
            }
        }

        // 顶部条：出画面几秒后自动淡出，看视频时画面完全干净
        AnimatedVisibility(
            visible = barsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.TopCenter),
        ) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .background(Color(0x99000000))
                    .padding(horizontal = 6.dp, vertical = 3.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = onBack, modifier = Modifier.size(34.dp)) {
                    Icon(Icons.Rounded.ArrowBack, "返回", tint = Color.White, modifier = Modifier.size(18.dp))
                }
                Column(Modifier.weight(1f).padding(start = 2.dp)) {
                    Text(
                        "${session.host} 的屏幕",
                        color = Color.White,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Medium,
                    )
                    Text(
                        "H.264 硬解 · $label · ${bitrate / 1000} Mbps",
                        color = Color(0xFF8C97A8),
                        fontSize = 9.5.sp,
                    )
                }
                if (notice.isNotEmpty() && ready) {
                    Text(notice, color = Color(0xFFFFB4A8), fontSize = 10.sp)
                }
                IconButton(onClick = { retry++ }, modifier = Modifier.size(34.dp)) {
                    Icon(Icons.Rounded.Refresh, "重连", tint = Color.White, modifier = Modifier.size(17.dp))
                }
            }
        }

        // 底部：画质
        AnimatedVisibility(
            visible = barsVisible,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .background(Color(0x99000000))
                    .padding(horizontal = 10.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("画质", color = Color(0xFF9AA3B2), fontSize = 10.5.sp)
                Spacer(Modifier.size(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    LevelChip("流畅 720p", level == 0) { level = 0 }
                    LevelChip("标准 1080p", level == 1) { level = 1 }
                    LevelChip("清晰 1440p", level == 2) { level = 2 }
                }
                Spacer(Modifier.weight(1f))
                LevelChip(if (fill) "铺满 ✓" else "铺满", fill) { fill = !fill }
            }
        }
    }
}

@Composable
private fun LevelChip(text: String, active: Boolean, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(8.dp),
        color = if (active) Color.White else Color(0x22FFFFFF),
    ) {
        Text(
            text,
            fontSize = 10.5.sp,
            color = if (active) Color(0xFF14181F) else Color(0xFFD7DEEA),
            fontWeight = if (active) FontWeight.SemiBold else FontWeight.Normal,
            modifier = Modifier.padding(horizontal = 9.dp, vertical = 4.dp),
        )
    }
}

private fun Context.findActivity(): Activity? {
    var c: Context? = this
    while (c is ContextWrapper) {
        if (c is Activity) return c
        c = c.baseContext
    }
    return null
}
