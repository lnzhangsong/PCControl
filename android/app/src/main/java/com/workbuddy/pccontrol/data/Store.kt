package com.workbuddy.pccontrol.data

import android.content.Context

/** 记住上次连接的电脑，下次打开直接连 */
class Store(context: Context) {

    private val sp = context.getSharedPreferences("pccontrol", Context.MODE_PRIVATE)

    var host: String
        get() = sp.getString("host", "") ?: ""
        set(v) = sp.edit().putString("host", v).apply()

    var port: Int
        get() = sp.getInt("port", 8765)
        set(v) = sp.edit().putInt("port", v).apply()

    var pin: String
        get() = sp.getString("pin", "") ?: ""
        set(v) = sp.edit().putString("pin", v).apply()

    var pcName: String
        get() = sp.getString("pcName", "") ?: ""
        set(v) = sp.edit().putString("pcName", v).apply()

    var mac: String
        get() = sp.getString("mac", "") ?: ""
        set(v) = sp.edit().putString("mac", v).apply()

    val hasSaved: Boolean get() = host.isNotEmpty() && pin.length == 6
}
