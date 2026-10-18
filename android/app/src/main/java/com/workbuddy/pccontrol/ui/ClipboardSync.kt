package com.workbuddy.pccontrol.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import com.workbuddy.pccontrol.Session
import com.workbuddy.pccontrol.net.PcApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.util.concurrent.atomic.AtomicReference

/**
 * 剪贴板自动跟随：两边复制的东西，另一边直接就能粘贴，不用点任何按钮。
 *
 * 两个方向：
 * - 手机复制 → 监听系统剪贴板，变了就推到电脑；
 * - 电脑复制 → 每 1.5 秒拉一次，变了就写进手机剪贴板。
 *
 * 用 [lastSynced] 记住"两边已经一致的内容"，避免两边互相推送形成死循环。
 *
 * 系统限制（不是实现问题）：Android 10 起后台应用读不到剪贴板，
 * 所以手机复制后自动上传只在 App 处于前台时有效；电脑→手机不受影响。
 */
@Composable
fun ClipboardSync(session: Session) {
    val context = LocalContext.current
    val api = remember(session) { PcApi(session.host, session.port, session.pin) }
    val lastSynced = remember { AtomicReference<String?>(null) }

    DisposableEffect(session) {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val listener = ClipboardManager.OnPrimaryClipChangedListener {
            val text = runCatching {
                cm.primaryClip?.takeIf { it.itemCount > 0 }
                    ?.getItemAt(0)?.coerceToText(context)?.toString()
            }.getOrNull().orEmpty()

            if (text.isBlank() || text == lastSynced.get()) return@OnPrimaryClipChangedListener
            lastSynced.set(text)
            ClipboardHistory.add(text, fromPc = false)
            Thread {
                runCatching { kotlinx.coroutines.runBlocking { api.writeClipboard(text) } }
            }.start()
        }
        runCatching { cm.addPrimaryClipChangedListener(listener) }
        onDispose { runCatching { cm.removePrimaryClipChangedListener(listener) } }
    }

    LaunchedEffect(session) {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

        // 刚连上时先看手机这边：手机上有内容说明是用户刚复制的，优先推给电脑
        val local = runCatching {
            cm.primaryClip?.takeIf { it.itemCount > 0 }
                ?.getItemAt(0)?.coerceToText(context)?.toString()
        }.getOrNull().orEmpty()
        if (local.isNotBlank()) {
            lastSynced.set(local)
            ClipboardHistory.add(local, fromPc = false)
            runCatching { api.writeClipboard(local) }
        }

        while (true) {
            val pc = runCatching { api.readClipboard() }.getOrNull()
            if (!pc.isNullOrBlank() && pc != lastSynced.get()) {
                lastSynced.set(pc)
                ClipboardHistory.add(pc, fromPc = true)
                runCatching {
                    cm.setPrimaryClip(ClipData.newPlainText("PC Remote", pc))
                }
            }
            delay(1_500)
        }
    }
}
