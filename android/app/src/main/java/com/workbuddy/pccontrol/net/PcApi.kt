package com.workbuddy.pccontrol.net

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/** 电脑端 /api/status 的返回 */
data class PcStatus(
    val name: String = "",
    val ip: String = "",
    val os: String = "",
    val volume: Int? = null,
    val muted: Boolean = false,
    val brightness: Int? = null,
    val brightnessMode: String = "",
    val brightnessReason: String = "",
    val hdr: Boolean = false,
    val mac: String = "",
    val canVolume: Boolean = true,
)

/**
 * 与 PC 端通信的极简客户端。
 * 只用 HttpURLConnection，不引入第三方网络库，方便离线编译。
 */
class PcApi(private val host: String, private val port: Int, private val pin: String) {

    private val base = "http://$host:$port"

    /** 串流地址（MPEG-TS / H.264），播放器直接用 */
    fun streamUrl(width: Int = 1920, bitrateKbps: Int = 8000, fps: Int = 30): String =
        "$base/api/stream?w=$width&br=$bitrateKbps&fps=$fps"

    /** 串流请求需要带的配对码头 */
    val authPin: String get() = pin

    private suspend fun request(path: String, body: JSONObject? = null): JSONObject =
        withContext(Dispatchers.IO) {
            val method = if (body == null) "GET" else "POST"
            val conn = (URL(base + path).openConnection() as HttpURLConnection).apply {
                requestMethod = method
                connectTimeout = 4_000
                readTimeout = 12_000          // 关 HDR 时服务端会等 2 秒
                setRequestProperty("X-Pin", pin)
                if (body != null) {
                    doOutput = true
                    setRequestProperty("Content-Type", "application/json; charset=utf-8")
                }
            }
            try {
                if (body != null) {
                    conn.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
                }
                val code = conn.responseCode
                val stream = if (code in 200..299) conn.inputStream else conn.errorStream
                val text = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
                when {
                    code == 401 -> throw IOException("配对码不正确")
                    code !in 200..299 -> throw IOException("请求失败（HTTP $code）")
                }
                JSONObject(text)
            } finally {
                conn.disconnect()
            }
        }

    suspend fun status(): PcStatus {
        val o = request("/api/status")
        return PcStatus(
            name = o.optString("name"),
            ip = o.optString("ip"),
            os = o.optString("os"),
            volume = if (o.isNull("volume")) null else o.optInt("volume"),
            muted = o.optBoolean("muted"),
            brightness = if (o.isNull("brightness")) null else o.optInt("brightness"),
            brightnessMode = o.optString("brightnessMode"),
            brightnessReason = o.optString("brightnessReason"),
            hdr = o.optBoolean("hdr"),
            mac = o.optString("mac"),
            canVolume = o.optBoolean("canVolume", true),
        )
    }

    suspend fun setVolume(value: Int) = request("/api/volume", JSONObject().put("value", value))
    suspend fun stepVolume(delta: Int) = request("/api/volume/step", JSONObject().put("delta", delta))
    suspend fun setMuted(value: Boolean) = request("/api/mute", JSONObject().put("value", value))
    suspend fun toggleMute() = request("/api/mute", JSONObject())
    suspend fun setBrightness(value: Int) = request("/api/brightness", JSONObject().put("value", value))
    suspend fun setHdr(enabled: Boolean) = request("/api/hdr", JSONObject().put("enabled", enabled))
    suspend fun media(action: String) = request("/api/media", JSONObject().put("action", action))
    suspend fun power(action: String) = request("/api/power", JSONObject().put("action", action))

    /** 读电脑剪贴板 */
    suspend fun readClipboard(): String? {
        val o = request("/api/clipboard")
        return if (o.optBoolean("ok")) o.optString("text") else null
    }

    /** 写电脑剪贴板 */
    suspend fun writeClipboard(text: String): JSONObject =
        request("/api/clipboard", JSONObject().put("text", text))

    /** 桌面图标分类整理。返回 {"moved":数量,...} */
    suspend fun tidyDesktop(): JSONObject = request("/api/desktop/tidy", JSONObject())

    /** 撤销上一次桌面整理。返回 {"restored":数量,...} */
    suspend fun undoDesktop(): JSONObject = request("/api/desktop/undo", JSONObject())

    /**
     * 上传文件。返回电脑端的保存路径。
     * [open] 每次调用都要给出一个新的输入流，避免把整个文件读进内存。
     */
    suspend fun upload(name: String, size: Long, open: () -> java.io.InputStream): String =
        withContext(Dispatchers.IO) {
            val encoded = java.net.URLEncoder.encode(name, "UTF-8")
            val conn = (URL("$base/api/upload?name=$encoded").openConnection()
                    as HttpURLConnection).apply {
                requestMethod = "POST"
                doOutput = true
                connectTimeout = 8_000
                readTimeout = 120_000
                setRequestProperty("X-Pin", pin)
                setRequestProperty("Content-Type", "application/octet-stream")
                if (size >= 0) setFixedLengthStreamingMode(size)
                else setChunkedStreamingMode(64 * 1024)
            }
            try {
                conn.outputStream.use { out ->
                    open().use { it.copyTo(out, 64 * 1024) }
                }
                val code = conn.responseCode
                val text = (if (code in 200..299) conn.inputStream else conn.errorStream)
                    ?.bufferedReader()?.use { it.readText() }.orEmpty()
                if (code == 401) throw IOException("配对码不正确")
                if (code !in 200..299) throw IOException("上传失败（HTTP $code）")
                val o = JSONObject(text)
                if (!o.optBoolean("ok")) throw IOException(o.optString("error", "上传失败"))
                o.optString("path")
            } finally {
                conn.disconnect()
            }
        }

    /** 取一帧电脑画面（JPEG）。width 是输出宽度上限，quality 1-100 */
    suspend fun screenJpeg(width: Int = 960, quality: Int = 60): ByteArray =
        withContext(Dispatchers.IO) {
            val conn = (URL("$base/api/screen?w=$width&q=$quality").openConnection()
                    as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = 4_000
                readTimeout = 10_000
                setRequestProperty("X-Pin", pin)
            }
            try {
                val code = conn.responseCode
                if (code == 401) throw IOException("配对码不正确")
                if (code !in 200..299) throw IOException("取画面失败（HTTP $code）")
                conn.inputStream.use { it.readBytes() }
            } finally {
                conn.disconnect()
            }
        }
}
