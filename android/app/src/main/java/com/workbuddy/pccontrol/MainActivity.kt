package com.workbuddy.pccontrol

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import com.workbuddy.pccontrol.data.Store
import com.workbuddy.pccontrol.net.PcApi
import com.workbuddy.pccontrol.ui.AppTheme
import com.workbuddy.pccontrol.ui.Bg
import com.workbuddy.pccontrol.ui.ClipboardHistory
import com.workbuddy.pccontrol.ui.ClipboardSync
import com.workbuddy.pccontrol.ui.ClipboardView
import com.workbuddy.pccontrol.ui.ConnectScreen
import com.workbuddy.pccontrol.ui.RemoteScreen
import com.workbuddy.pccontrol.ui.ScreenView

/** 一次连接会话 */
data class Session(val host: String, val port: Int, val pin: String)

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ClipboardHistory.init(this)
        setContent {
            AppTheme {
                Surface(Modifier.fillMaxSize(), color = Bg) {
                    Box(Modifier.fillMaxSize()) {
                        AppRoot(
                            openScreen = intent?.getBooleanExtra("screen", false) == true,
                            openClipboard = intent?.getBooleanExtra("clipboard", false) == true,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun AppRoot(openScreen: Boolean = false, openClipboard: Boolean = false) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val store = remember { Store(context.applicationContext) }
    var session by remember { mutableStateOf<Session?>(null) }
    var triedAuto by remember { mutableStateOf(false) }
    var showScreen by remember { mutableStateOf(openScreen) }
    var showClipboard by remember { mutableStateOf(openClipboard) }

    // 上次连过就自动重连，省去每次输入
    LaunchedEffect(Unit) {
        if (!triedAuto && store.hasSaved) {
            triedAuto = true
            val ok = runCatching {
                PcApi(store.host, store.port, store.pin).status()
            }.isSuccess
            if (ok) session = Session(store.host, store.port, store.pin)
        }
    }

    // targetSdk 35 起系统强制 edge-to-edge：普通页面让出状态栏/导航栏，
    // 看画面那页要真正的全屏，不能加内边距，否则四周会留一圈黑边
    val current = session
    // 连着电脑就一直双向同步剪贴板，不用用户点任何按钮
    if (current != null) {
        ClipboardSync(current)
    }

    if (current == null) {
        Box(Modifier.fillMaxSize().safeDrawingPadding()) {
            ConnectScreen(store) { session = it }
        }
    } else if (showScreen) {
        ScreenView(current) { showScreen = false }
    } else if (showClipboard) {
        Box(Modifier.fillMaxSize().safeDrawingPadding()) {
            ClipboardView(current) { showClipboard = false }
        }
    } else {
        Box(Modifier.fillMaxSize().safeDrawingPadding()) {
            RemoteScreen(
                current, store,
                onExit = { session = null },
                onScreen = { showScreen = true },
                onClipboard = { showClipboard = true },
            )
        }
    }
}
