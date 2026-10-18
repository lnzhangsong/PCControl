package com.workbuddy.pccontrol.ui

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/** 一条剪贴板记录。[fromPc] 为 true 表示内容来自电脑，false 表示来自手机 */
data class ClipItem(
    val id: Long,
    val text: String,
    val fromPc: Boolean,
    val time: Long,
)

/**
 * 剪贴板历史：存在手机本地（SharedPreferences），App 重装前一直留着。
 *
 * 只存最近 [MAX] 条，纯文本，同一内容重复出现时把旧的提到最前面，
 * 不产生重复条目。
 */
object ClipboardHistory {

    private const val MAX = 100

    private var prefs: android.content.SharedPreferences? = null

    fun init(context: Context) {
        if (prefs == null) {
            prefs = context.applicationContext
                .getSharedPreferences("clipboard", Context.MODE_PRIVATE)
        }
    }

    @Synchronized
    fun list(): List<ClipItem> {
        val raw = prefs?.getString("items", null) ?: return emptyList()
        return runCatching {
            val arr = JSONArray(raw)
            val out = ArrayList<ClipItem>(arr.length())
            for (i in 0 until arr.length()) {
                val o = arr.optJSONObject(i) ?: continue
                out.add(
                    ClipItem(
                        id = o.optLong("id"),
                        text = o.optString("text"),
                        fromPc = o.optBoolean("pc"),
                        time = o.optLong("t"),
                    )
                )
            }
            out
        }.getOrDefault(emptyList())
    }

    /** 记一条。同样的内容已经存在就只把它挪到最前面并刷新时间 */
    @Synchronized
    fun add(text: String, fromPc: Boolean) {
        val t = text.trim()
        if (t.isEmpty()) return
        val p = prefs ?: return
        val items = list().toMutableList()
        items.removeAll { it.text == t }
        items.add(0, ClipItem(System.currentTimeMillis(), t, fromPc, System.currentTimeMillis()))
        val keep = items.take(MAX)
        p.edit().putString("items", toJson(keep)).apply()
    }

    @Synchronized
    fun remove(id: Long) {
        val p = prefs ?: return
        val items = list().filterNot { it.id == id }
        p.edit().putString("items", toJson(items)).apply()
    }

    @Synchronized
    fun clear() {
        prefs?.edit()?.putString("items", JSONArray().toString())?.apply()
    }

    private fun toJson(items: List<ClipItem>): String {
        val arr = JSONArray()
        items.forEach {
            arr.put(
                JSONObject()
                    .put("id", it.id)
                    .put("text", it.text)
                    .put("pc", it.fromPc)
                    .put("t", it.time)
            )
        }
        return arr.toString()
    }
}
