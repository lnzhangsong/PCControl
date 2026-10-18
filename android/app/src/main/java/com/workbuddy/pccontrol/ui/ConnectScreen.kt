package com.workbuddy.pccontrol.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowForward
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Wifi
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.workbuddy.pccontrol.Session
import com.workbuddy.pccontrol.data.Store
import com.workbuddy.pccontrol.net.Discovery
import com.workbuddy.pccontrol.net.PcApi
import kotlinx.coroutines.launch

@Composable
fun ConnectScreen(store: Store, onConnected: (Session) -> Unit) {

    val scope = rememberCoroutineScope()
    val found = remember { mutableStateListOf<Discovery.Found>() }
    var scanning by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }

    var ip by remember { mutableStateOf(store.host) }
    var port by remember { mutableStateOf(store.port.toString()) }
    var pin by remember { mutableStateOf(store.pin) }

    fun scan() {
        scope.launch {
            scanning = true
            // 广播 + 对上次连过的地址单播点名（跨网段时只能靠后者）
            val list = Discovery.scan(candidates = listOf(store.host, ip))
            found.clear()
            found.addAll(list)
            scanning = false
            if (found.isNotEmpty()) {
                val first = found.first()
                if (ip.isBlank()) ip = first.host
                if (port.isBlank()) port = first.port.toString()
            }
        }
    }

    fun connect() {
        error = ""
        if (ip.isBlank()) { error = "请填写电脑的 IP 地址"; return }
        if (pin.length != 6) { error = "配对码是 6 位数字"; return }
        busy = true
        scope.launch {
            try {
                val api = PcApi(ip.trim(), port.toIntOrNull() ?: 8765, pin.trim())
                val status = api.status()
                store.host = ip.trim()
                store.port = port.toIntOrNull() ?: 8765
                store.pin = pin.trim()
                store.pcName = status.name
                store.mac = status.mac
                onConnected(Session(store.host, store.port, store.pin))
            } catch (e: Exception) {
                error = e.message ?: "连接失败，检查 IP 与配对码"
            } finally {
                busy = false
            }
        }
    }

    // 进入页面自动搜索一次
    LaunchedEffect(Unit) { if (found.isEmpty()) scan() }

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp),
    ) {
        Spacer(Modifier.height(28.dp))
        Text("PC 遥控器", fontSize = 26.sp, fontWeight = FontWeight.Bold, color = TextMain)
        Spacer(Modifier.height(6.dp))
        Text("手机和电脑连同一个 Wi-Fi，就能调音量、亮度，还能锁屏休眠",
            fontSize = 13.sp, color = TextSub, lineHeight = 19.sp)
        Spacer(Modifier.height(20.dp))

        RemoteCard {
            CardLabel("自动发现") {
                IconButton(onClick = { scan() }, enabled = !scanning) {
                    Icon(Icons.Rounded.Refresh, contentDescription = "重新搜索", tint = TextSub)
                }
            }
            if (scanning) {
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.Center,
                ) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp, color = Accent)
                    Spacer(Modifier.size(10.dp))
                    Text("正在搜索局域网内的电脑…", fontSize = 13.sp, color = TextSub)
                }
            } else if (found.isEmpty()) {
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.Rounded.Wifi, null, tint = TextSub, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.size(8.dp))
                    Text("没找到设备，请确认电脑端服务已启动", fontSize = 12.5.sp, color = TextSub)
                }
            } else {
                found.forEachIndexed { index, item ->
                    if (index > 0) HorizontalDivider(color = Line)
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clickable {
                                ip = item.host
                                port = item.port.toString()
                                if (pin.length == 6) connect()
                            }
                            .padding(vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Column {
                            Text(item.name, fontSize = 14.5.sp, fontWeight = FontWeight.Medium, color = TextMain)
                            Text(item.host, fontSize = 12.sp, color = TextSub)
                        }
                        Icon(Icons.Rounded.ArrowForward, null, tint = Accent, modifier = Modifier.size(18.dp))
                    }
                }
            }
        }

        Spacer(Modifier.height(12.dp))

        RemoteCard {
            CardLabel("手动连接")
            Spacer(Modifier.height(10.dp))
            OutlinedTextField(
                value = ip,
                onValueChange = { ip = it },
                label = { Text("电脑 IP") },
                placeholder = { Text("192.168.1.3") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            )
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = port,
                    onValueChange = { port = it.filter { c -> c.isDigit() }.take(5) },
                    label = { Text("端口") },
                    singleLine = true,
                    modifier = Modifier.weight(1f),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                )
                OutlinedTextField(
                    value = pin,
                    onValueChange = { pin = it.filter { c -> c.isDigit() }.take(6) },
                    label = { Text("配对码") },
                    placeholder = { Text("6 位数字") },
                    singleLine = true,
                    modifier = Modifier.weight(1.6f),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
                )
            }
            Spacer(Modifier.height(14.dp))
            Button(
                onClick = { connect() },
                enabled = !busy,
                modifier = Modifier.fillMaxWidth().height(50.dp),
                colors = ButtonDefaults.buttonColors(containerColor = Accent),
            ) {
                if (busy) {
                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = androidx.compose.ui.graphics.Color.White)
                } else {
                    Text("连接", fontSize = 15.sp, fontWeight = FontWeight.Medium)
                }
            }
            Hint(error, WarnAmber)
        }

        Spacer(Modifier.height(16.dp))
        Text(
            "电脑端启动 pc-server/start.bat 后会显示 IP 与配对码。",
            fontSize = 11.5.sp, color = TextSub,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
        Spacer(Modifier.height(28.dp))
    }
}
