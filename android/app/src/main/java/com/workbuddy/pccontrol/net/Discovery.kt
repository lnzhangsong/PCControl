package com.workbuddy.pccontrol.net

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.SocketTimeoutException
import java.util.Collections

/** 局域网自动发现：向 UDP 55667 广播，PC 端服务会回自己的名字和端口 */
object Discovery {

    private const val PORT = 55667
    private const val MAGIC = "PCREMOTE_DISCOVER"

    data class Found(val host: String, val name: String, val port: Int)

    /**
     * @param candidates 历史连接过的地址。广播出不了本网段，跨网段时必须靠单播点名询问。
     */
    suspend fun scan(
        candidates: List<String> = emptyList(),
        timeoutMs: Long = 2_200,
    ): List<Found> = withContext(Dispatchers.IO) {
        val result = mutableListOf<Found>()
        var socket: DatagramSocket? = null
        try {
            socket = DatagramSocket().apply {
                broadcast = true
                soTimeout = 250
            }
            val payload = MAGIC.toByteArray()
            val buf = ByteArray(512)

            // 1) 广播：同网段时最快，但路由器不会把它转给别的网段
            val targets = broadcastAddresses()
            repeat(3) {
                targets.forEach { addr ->
                    runCatching {
                        socket.send(DatagramPacket(payload, payload.size, addr, PORT))
                    }
                }
            }

            // 2) 单播点名：跨网段唯一可行的方式（网关会正常路由）
            val unicasts = candidates
                .filter { it.isNotBlank() }
                .mapNotNull { runCatching { InetAddress.getByName(it) }.getOrNull() }
                .distinctBy { it.hostAddress }
            repeat(2) {
                unicasts.forEach { addr ->
                    runCatching {
                        socket.send(DatagramPacket(payload, payload.size, addr, PORT))
                    }
                }
            }

            val deadline = System.currentTimeMillis() + timeoutMs
            while (System.currentTimeMillis() < deadline) {
                try {
                    val packet = DatagramPacket(buf, buf.size)
                    socket.receive(packet)
                    val text = String(packet.data, 0, packet.length).trim()
                    if (!text.startsWith("{")) continue
                    val json = JSONObject(text)
                    if (json.optString("app") != "pccontrol") continue
                    val host = packet.address.hostAddress ?: continue
                    result += Found(
                        host = host,
                        name = json.optString("name", host),
                        port = json.optInt("port", 8765),
                    )
                } catch (_: SocketTimeoutException) {
                    // 继续等
                }
            }
        } catch (_: Exception) {
            // 广播被路由器拦截时返回已发现的结果（可能为空）
        } finally {
            socket?.close()
        }
        result.distinctBy { it.host }
    }

    /** 255.255.255.255 + 每个网卡自己的广播地址（如 192.168.1.255） */
    private fun broadcastAddresses(): List<InetAddress> {
        val list = mutableListOf<InetAddress>()
        runCatching { list += InetAddress.getByName("255.255.255.255") }
        runCatching {
            Collections.list(NetworkInterface.getNetworkInterfaces()).forEach { nif ->
                if (!nif.isUp || nif.isLoopback) return@forEach
                nif.interfaceAddresses.forEach { ia ->
                    ia.broadcast?.let { list += it }
                }
            }
        }
        return list.distinctBy { it.hostAddress }
    }
}
