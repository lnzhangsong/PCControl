package com.workbuddy.pccontrol.net

import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress

/** 网络唤醒（Wake-on-LAN）：电脑睡眠/关机时也能把它叫醒 */
object Wol {

    fun wake(mac: String): Boolean = try {
        val clean = mac.replace(":", "").replace("-", "").replace(".", "")
        if (clean.length != 12) {
            false
        } else {
            sendMagic(clean)
        }
    } catch (_: Exception) {
        false
    }

    private fun sendMagic(clean: String): Boolean = try {
        val macBytes = ByteArray(6) { i ->
            clean.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
        val packet = ByteArray(6 + 16 * 6)
        for (i in 0 until 6) packet[i] = 0xFF.toByte()
        for (i in 0 until 16) {
            System.arraycopy(macBytes, 0, packet, 6 + i * 6, 6)
        }
        DatagramSocket().use { socket ->
            socket.broadcast = true
            val addr = InetAddress.getByName("255.255.255.255")
            repeat(3) { socket.send(DatagramPacket(packet, packet.size, addr, 9)) }
        }
        true
    } catch (_: Exception) {
        false
    }
}
