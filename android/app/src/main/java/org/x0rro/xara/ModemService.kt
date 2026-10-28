package org.x0rro.xara

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import com.hoho.android.usbserial.driver.UsbSerialPort
import com.hoho.android.usbserial.driver.UsbSerialProber
import com.hoho.android.usbserial.util.SerialInputOutputManager
import java.net.Inet4Address
import java.net.NetworkInterface
import java.nio.ByteBuffer

data class ModemConfig(
    val call: String,
    val mode: Int,
    val bind: String,
    val cmdPort: Int,
    val kissPort: Int,
    val leadMs: Int,
    val tailMs: Int,
    val maxSec: Int,
    val ptt: Int,
    val radio: Int,
    val civAddr: Int,
    val baud: Int,
    val speaker: Boolean,
    val captureId: Int,
    val playbackId: Int,
    val serialName: String,
    val radioMode: Int,
)

class ModemService : Service(), SerialInputOutputManager.Listener {
    inner class Local : Binder() {
        fun service(): ModemService = this@ModemService
    }

    private val local = Local()
    private val gate = Any()
    private val inbound = ArrayDeque<Byte>()
    @Volatile private var loop = false
    @Volatile private var snap = "Stopped."
    @Volatile private var freqHz = ""
    @Volatile private var radioModeName = ""
    @Volatile private var catPtt = false
    @Volatile private var hwPtt = false
    @Volatile private var manual = false
    @Volatile private var serialNote = ""
    private var notice = ""

    private var worker: Thread? = null
    private var usbPort: UsbSerialPort? = null
    private var serialIo: SerialInputOutputManager? = null
    private var record: AudioRecord? = null
    private var track: AudioTrack? = null
    private var wake: PowerManager.WakeLock? = null
    private var cfg: ModemConfig? = null

    private val inBuf: ByteBuffer = ByteBuffer.allocateDirect(8192)
    private val outBuf: ByteBuffer = ByteBuffer.allocateDirect(8192)
    private val catBuf: ByteBuffer = ByteBuffer.allocateDirect(4096)
    private val cmdBuf: ByteBuffer = ByteBuffer.allocateDirect(64)

    override fun onBind(intent: Intent?): IBinder = local

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        show("xara", "Starting", intent?.getBooleanExtra("mic", true) ?: true)
        return START_NOT_STICKY
    }

    fun snapshot(): String = snap

    fun running(): Boolean = loop

    fun setManualKey(on: Boolean) {
        manual = on
        if (NativeModem.available) NativeModem.setManual(on)
        if (cfg?.ptt != PTT_TRUSDX) {
            applyLine(on || hwPtt)
        }
    }

    fun startModem(config: ModemConfig) {
        halt()
        cfg = config
        if (!NativeModem.available) {
            snap = BUILD_HELP
            show("xara", "native library missing", false)
            return
        }
        val msg = NativeModem.start(config.call, config.mode, config.bind, config.cmdPort, config.kissPort, config.leadMs)
        if (msg.startsWith("ERR")) {
            snap = msg.removePrefix("ERR\n")
            show("xara", "start failed", false)
            return
        }
        NativeModem.setTail(config.tailMs)
        freqHz = ""
        radioModeName = ""
        catPtt = false
        if (config.radio != RADIO_NONE) {
            NativeModem.catOpen(config.radio, config.civAddr)
        }
        val usbErr = openSerial(config)
        if (usbErr != null && config.ptt == PTT_TRUSDX) {
            NativeModem.stop()
            snap = usbErr
            show("xara", "serial failed", false)
            return
        }
        serialNote = when {
            usbErr != null -> "$usbErr Audio continues on the mic and speaker."
            usbPort == null && config.ptt != PTT_VOX -> "Audio only. No serial PTT."
            else -> ""
        }
        val audioErr = if (config.ptt == PTT_TRUSDX) null else openAudio(config)
        if (audioErr != null) {
            closeSerial()
            NativeModem.stop()
            snap = audioErr
            show("xara", "audio failed", false)
            return
        }
        primeRadio(config)
        loop = true
        worker = Thread({ runLoop(config, msg.removePrefix("OK\n")) }, "xara-modem").also { it.start() }
    }

    fun halt() {
        loop = false
        manual = false
        try {
            record?.stop()
        } catch (_: IllegalStateException) {
        }
        worker?.join(2000)
        worker = null
        if (NativeModem.available && usbPort != null) {
            val n = NativeModem.endTx(outBuf)
            if (n > 0) writeRaw(n)
            applyLine(false)
        }
        closeAudio()
        closeSerial()
        if (NativeModem.available) NativeModem.stop()
        hwPtt = false
        serialNote = ""
        notice = ""
        wake?.let { if (it.isHeld) it.release() }
        wake = null
    }

    override fun onDestroy() {
        halt()
        stopForeground(STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    override fun onNewData(data: ByteArray) {
        synchronized(gate) {
            for (b in data) inbound.add(b)
            while (inbound.size > 200_000) inbound.removeFirst()
        }
    }

    override fun onRunError(e: Exception) {
        snap = "Serial read stopped: ${e.message}"
        loop = false
    }

    private fun runLoop(config: ModemConfig, bound: String) {
        val block = NativeModem.block().coerceAtLeast(1)
        val rx = ShortArray(block)
        val tx = ShortArray(block)
        var line = false
        var tailUntil = 0L
        var onSince = 0L
        var forced = false
        val maxMs = config.maxSec.coerceIn(1, 120) * 1000L
        while (loop) {
            val now = SystemClock.elapsedRealtime()
            val modemWants = if (config.ptt == PTT_TRUSDX) {
                val n = takeInbound()
                val packed = NativeModem.cycleTrusdx(inBuf, n, forced, outBuf, catBuf)
                if (packed < 0) {
                    snap = "Modem stopped."
                    break
                }
                val serialN = (packed and 0xffffffffL).toInt()
                val catN = (packed ushr 32).toInt()
                if (serialN > 0) writeRaw(serialN)
                if (catN > 0) noteCat(catN)
                NativeModem.lastKeyed()
            } else {
                val got = readBlock(rx)
                if (got < 0) {
                    snap = "Audio input stopped."
                    break
                }
                val keyed = NativeModem.process(rx, tx, forced)
                if (keyed < 0) {
                    snap = "Modem stopped."
                    break
                }
                writeBlock(tx)
                val inn = takeInbound()
                if (inn > 0) {
                    val cn = NativeModem.catFeed(inBuf, inn, catBuf)
                    if (cn > 0) noteCat(cn)
                }
                keyed == 1
            }
            val want = !forced && (modemWants || manual || (line && now < tailUntil))
            if (want && !line) tailUntil = 0
            if (!want && line && tailUntil == 0L) tailUntil = now + config.tailMs.coerceAtLeast(0)
            val key = !forced && (modemWants || manual || (line && now < tailUntil))
            if (config.ptt != PTT_TRUSDX) applyLine(key)
            line = key
            hwPtt = key
            if (key) {
                if (onSince == 0L) onSince = now
                if (now - onSince > maxMs) forced = true
            } else if (!modemWants && !manual) {
                onSince = 0L
                forced = false
                tailUntil = 0
            }
            publish(config, bound, key, forced)
            if (config.ptt == PTT_TRUSDX) pace(now)
        }
        loop = false
    }

    private fun pace(started: Long) {
        val used = SystemClock.elapsedRealtime() - started
        val rest = 10L - used
        if (rest > 0) {
            try {
                Thread.sleep(rest)
            } catch (_: InterruptedException) {
                loop = false
            }
        }
    }

    private fun publish(config: ModemConfig, bound: String, key: Boolean, forced: Boolean) {
        val hosts = ipv4()
        val cmd = lineAfter(bound, "cmd ")
        val data = lineAfter(bound, "data ")
        val kiss = lineAfter(bound, "kiss ")
        val session = sessionName(config.mode)
        val text = buildString {
            append("xara is running.\n\n")
            append(session)
            append("\nCommand ")
            append(pretty(cmd, hosts))
            append("\nData ")
            append(pretty(data, hosts))
            append("\nThe data port is the command port plus one.")
            append("\nOn this phone, use host 127.0.0.1.")
            append("\n\nKISS TNC ")
            append(if (kiss == "off" || kiss.isEmpty()) "is off" else pretty(kiss, hosts))
            append("\nKISS carries AX.25 inside the VARA-compatible modem, not 1200-baud packet.")
            if (NativeModem.registered()) append("\nRegistered speeds.")
            append("\n\nPTT ")
            append(pttName(config.ptt))
            append(if (key) " keyed" else " idle")
            if (forced) append(" (watchdog)")
            if (config.radio == RADIO_NONE) {
                append("\nRadio: none")
            } else {
                append("\nRadio ")
                append(radioName(config.radio))
                append(if (freqHz.isEmpty()) "" else "  $freqHz Hz")
                append(if (radioModeName.isEmpty()) "" else "  $radioModeName")
                if (catPtt) append("  CAT-TX")
            }
            if (serialNote.isNotEmpty()) {
                append("\n")
                append(serialNote)
            }
        }
        snap = text
        val line = "$session  ${pretty(cmd, hosts)}"
        if (line != notice) {
            notice = line
            show("xara", line, config.ptt != PTT_TRUSDX)
        }
    }

    private fun primeRadio(config: ModemConfig) {
        if (usbPort == null) return
        if (config.ptt == PTT_RTS) {
            usbPort?.setRTS(false)
            usbPort?.setDTR(false)
        } else if (config.ptt == PTT_DTR) {
            usbPort?.setDTR(false)
            usbPort?.setRTS(false)
        } else if (config.ptt == PTT_TRUSDX) {
            usbPort?.setDTR(true)
            usbPort?.setRTS(false)
        }
        if (config.radio == RADIO_NONE) return
        writeOp(NativeModem.OP_OPEN, 0, config.radioMode)
        if (config.ptt == PTT_TRUSDX) {
            val n = NativeModem.trusdxEnable(config.speaker, cmdBuf)
            if (n > 0) writeCmd(n)
        }
        writeOp(NativeModem.OP_GET_FREQ, 0, config.radioMode)
        writeOp(NativeModem.OP_GET_MODE, 0, config.radioMode)
    }

    fun pushFreq(hz: Long, mode: Int) {
        writeOp(NativeModem.OP_SET_FREQ, hz, mode)
        writeOp(NativeModem.OP_SET_MODE, hz, mode)
    }

    fun pollRadio(mode: Int) {
        writeOp(NativeModem.OP_GET_FREQ, 0, mode)
        writeOp(NativeModem.OP_GET_MODE, 0, mode)
    }

    private fun writeOp(op: Int, hz: Long, mode: Int) {
        if (usbPort == null || cfg?.radio == RADIO_NONE || !NativeModem.available) return
        val n = NativeModem.catBuild(op, hz, mode, cmdBuf)
        if (n > 0) writeCmd(n)
    }

    private fun writeCmd(n: Int) {
        val raw = ByteArray(n)
        for (i in 0 until n) raw[i] = cmdBuf.get(i)
        try {
            usbPort?.write(raw, 500)
        } catch (_: Exception) {
        }
    }

    private fun writeRaw(n: Int) {
        val raw = ByteArray(n)
        for (i in 0 until n) raw[i] = outBuf.get(i)
        try {
            usbPort?.write(raw, 500)
        } catch (e: Exception) {
            snap = "Serial write failed: ${e.message}"
            loop = false
        }
    }

    private fun applyLine(on: Boolean) {
        val port = usbPort ?: return
        val config = cfg ?: return
        try {
            when (config.ptt) {
                PTT_RTS -> {
                    port.setRTS(on)
                    port.setDTR(false)
                }
                PTT_DTR -> {
                    port.setDTR(on)
                    port.setRTS(false)
                }
                PTT_CAT -> {
                    if (on != hwPtt) writeOp(if (on) NativeModem.OP_PTT_ON else NativeModem.OP_PTT_OFF, 0, config.radioMode)
                }
            }
        } catch (e: Exception) {
            snap = "PTT failed: ${e.message}"
            loop = false
        }
    }

    private fun noteCat(n: Int) {
        val raw = ByteArray(n)
        for (i in 0 until n) raw[i] = catBuf.get(i)
        val text = String(raw, Charsets.US_ASCII)
        for (line in text.lineSequence()) {
            when {
                line.startsWith("F ") -> freqHz = line.removePrefix("F ").trim()
                line.startsWith("M ") -> radioModeName = line.removePrefix("M ").trim()
                line.startsWith("P ") -> catPtt = line.trim().endsWith("1")
            }
        }
    }

    private fun takeInbound(): Int {
        synchronized(gate) {
            var i = 0
            while (inbound.isNotEmpty() && i < inBuf.capacity()) {
                inBuf.put(i, inbound.removeFirst())
                i++
            }
            return i
        }
    }

    private fun openSerial(config: ModemConfig): String? {
        if (config.serialName.isEmpty()) {
            return if (config.ptt == PTT_TRUSDX) "Select a USB serial device." else null
        }
        val usb = getSystemService(UsbManager::class.java)
        val driver = UsbSerialProber.getDefaultProber().findAllDrivers(usb)
            .firstOrNull { it.device.deviceName == config.serialName }
            ?: return "USB serial device is gone. Refresh and grant permission."
        if (!usb.hasPermission(driver.device)) return "USB permission was not granted."
        val connection = usb.openDevice(driver.device) ?: return "Could not open the USB device."
        val port = driver.ports.firstOrNull() ?: return "That USB device has no serial port."
        return try {
            port.open(connection)
            port.setParameters(config.baud, 8, UsbSerialPort.STOPBITS_1, UsbSerialPort.PARITY_NONE)
            port.setDTR(config.ptt == PTT_TRUSDX)
            port.setRTS(false)
            usbPort = port
            serialIo = SerialInputOutputManager(port, this).also {
                it.setReadTimeout(50)
                it.start()
            }
            null
        } catch (e: Exception) {
            "Serial open failed: ${e.message}"
        }
    }

    private fun closeSerial() {
        try {
            serialIo?.stop()
        } catch (_: Exception) {
        }
        serialIo = null
        try {
            usbPort?.close()
        } catch (_: Exception) {
        }
        usbPort = null
        synchronized(gate) { inbound.clear() }
    }

    private fun openAudio(config: ModemConfig): String? {
        val manager = getSystemService(AudioManager::class.java)
        val rate = 48000
        val min = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        if (min <= 0) return "This device has no 48 kHz mono capture."
        val capture = findDevice(manager, config.captureId, true)
        val playback = findDevice(manager, config.playbackId, false)
        val rec = try {
            buildRecord(rate, min, capture)
        } catch (e: Exception) {
            return "Audio input failed: ${e.message}"
        }
        val play = try {
            buildTrack(rate, min, playback)
        } catch (e: Exception) {
            rec.release()
            return "Audio output failed: ${e.message}"
        }
        record = rec
        track = play
        rec.startRecording()
        play.play()
        val pm = getSystemService(PowerManager::class.java)
        wake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "xara:modem").also { it.acquire(4 * 60 * 60 * 1000L) }
        return null
    }

    private fun buildRecord(rate: Int, min: Int, device: AudioDeviceInfo?): AudioRecord {
        val format = AudioFormat.Builder()
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setSampleRate(rate)
            .setChannelMask(AudioFormat.CHANNEL_IN_MONO)
            .build()
        val sources = intArrayOf(MediaRecorder.AudioSource.UNPROCESSED, MediaRecorder.AudioSource.MIC)
        var last: Exception? = null
        for (source in sources) {
            try {
                val rec = AudioRecord.Builder()
                    .setAudioSource(source)
                    .setAudioFormat(format)
                    .setBufferSizeInBytes(min.coerceAtLeast(rate / 5) * 2)
                    .build()
                if (rec.state != AudioRecord.STATE_INITIALIZED) {
                    rec.release()
                    continue
                }
                if (device != null) rec.setPreferredDevice(device)
                return rec
            } catch (e: Exception) {
                last = e
            }
        }
        throw last ?: IllegalStateException("no audio input")
    }

    private fun buildTrack(rate: Int, min: Int, device: AudioDeviceInfo?): AudioTrack {
        val format = AudioFormat.Builder()
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setSampleRate(rate)
            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
            .build()
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .build()
        val play = AudioTrack.Builder()
            .setAudioAttributes(attrs)
            .setAudioFormat(format)
            .setBufferSizeInBytes(min.coerceAtLeast(rate / 5) * 2)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()
        if (device != null) play.setPreferredDevice(device)
        return play
    }

    private fun closeAudio() {
        try {
            record?.stop()
        } catch (_: Exception) {
        }
        record?.release()
        record = null
        try {
            track?.stop()
        } catch (_: Exception) {
        }
        track?.release()
        track = null
    }

    private fun readBlock(dst: ShortArray): Int {
        val rec = record ?: return -1
        var got = 0
        while (got < dst.size && loop) {
            val n = rec.read(dst, got, dst.size - got)
            if (n < 0) return -1
            if (n == 0) return -1
            got += n
        }
        return got
    }

    private fun writeBlock(src: ShortArray) {
        val play = track ?: return
        var sent = 0
        while (sent < src.size && loop) {
            val n = play.write(src, sent, src.size - sent)
            if (n < 0) {
                loop = false
                return
            }
            sent += n
        }
    }

    private fun show(title: String, text: String, mic: Boolean) {
        val id = "xara"
        if (Build.VERSION.SDK_INT >= 26) {
            val channel = NotificationChannel(id, "xara modem", NotificationManager.IMPORTANCE_LOW)
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
        val note: Notification = NotificationCompat.Builder(this, id)
            .setContentTitle(title)
            .setContentText(text.take(80))
            .setSmallIcon(android.R.drawable.stat_sys_headset)
            .setOngoing(true)
            .build()
        // connectedDevice is only valid while a USB device is open. Claiming it
        // with nothing attached makes Android reject the service on API 34+.
        var type = 0
        if (Build.VERSION.SDK_INT >= 30 && mic) {
            type = type or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        }
        if (Build.VERSION.SDK_INT >= 29 && usbPort != null) {
            type = type or ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        }
        if (Build.VERSION.SDK_INT < 29) {
            startForeground(1, note)
            return
        }
        if (type == 0) type = ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        try {
            startForeground(1, note, type)
        } catch (_: SecurityException) {
            if (mic && Build.VERSION.SDK_INT >= 30) {
                startForeground(1, note, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
            } else {
                throw SecurityException("USB serial is required for this PTT mode.")
            }
        }
    }
}

fun findDevice(manager: AudioManager, id: Int, input: Boolean): AudioDeviceInfo? {
    if (id < 0) return null
    val flag = if (input) AudioManager.GET_DEVICES_INPUTS else AudioManager.GET_DEVICES_OUTPUTS
    return manager.getDevices(flag).firstOrNull { it.id == id }
}

fun audioNames(manager: AudioManager, input: Boolean): List<Pair<Int, String>> {
    val flag = if (input) AudioManager.GET_DEVICES_INPUTS else AudioManager.GET_DEVICES_OUTPUTS
    val out = mutableListOf(-1 to "System default")
    for (d in manager.getDevices(flag)) {
        val kind = when (d.type) {
            AudioDeviceInfo.TYPE_BUILTIN_MIC -> "mic"
            AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "speaker"
            AudioDeviceInfo.TYPE_WIRED_HEADSET -> "headset"
            AudioDeviceInfo.TYPE_USB_DEVICE, AudioDeviceInfo.TYPE_USB_HEADSET -> "USB"
            AudioDeviceInfo.TYPE_BLUETOOTH_SCO, AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> "bluetooth"
            else -> "device"
        }
        out.add(d.id to "$kind ${d.productName}")
    }
    return out
}

fun serialDevices(usb: UsbManager): List<Pair<String, String>> {
    return UsbSerialProber.getDefaultProber().findAllDrivers(usb).map { driver ->
        val dev: UsbDevice = driver.device
        val label = listOfNotNull(dev.productName, dev.manufacturerName).joinToString(" ").ifBlank { dev.deviceName }
        dev.deviceName to "$label (${dev.deviceName})"
    }
}

private fun lineAfter(text: String, prefix: String): String {
    return text.lineSequence().firstOrNull { it.startsWith(prefix) }?.removePrefix(prefix)?.trim().orEmpty()
}

private fun ipv4(): List<String> {
    val out = mutableListOf<String>()
    val ifs = NetworkInterface.getNetworkInterfaces() ?: return out
    for (nif in ifs) {
        if (!nif.isUp || nif.isLoopback) continue
        for (addr in nif.inetAddresses) {
            if (addr is Inet4Address && !addr.isLoopbackAddress) {
                addr.hostAddress?.let { out.add(it) }
            }
        }
    }
    return out
}

private fun pretty(bound: String, hosts: List<String>): String {
    if (bound.isEmpty()) return "—"
    val port = bound.substringAfterLast(':')
    val host = bound.substringBeforeLast(':')
    if (host == "0.0.0.0" || host == "::") {
        val lans = if (hosts.isEmpty()) "" else hosts.joinToString(", ") { "$it:$port" } + "; "
        return "${lans}127.0.0.1:$port"
    }
    return bound
}

const val PTT_RTS = 0
const val PTT_DTR = 1
const val PTT_CAT = 2
const val PTT_TRUSDX = 3
const val PTT_VOX = 4

const val RADIO_NONE = -1
const val RADIO_KENWOOD = 0
const val RADIO_G90 = 1
const val RADIO_ICOM = 2
const val RADIO_YAESU = 3
const val RADIO_TRUSDX = 4

fun sessionName(mode: Int): String = when (mode) {
    0 -> "VARA-compatible HF 500"
    1 -> "VARA-compatible HF 2300"
    2 -> "VARA-compatible HF 2750"
    3 -> "VARA-compatible FM narrow"
    else -> "VARA-compatible FM wide"
}

fun pttName(id: Int): String = when (id) {
    PTT_RTS -> "RTS"
    PTT_DTR -> "DTR"
    PTT_CAT -> "CAT"
    PTT_TRUSDX -> "(tr)uSDX audio"
    else -> "VOX"
}

fun radioName(id: Int): String = when (id) {
    RADIO_KENWOOD -> "Kenwood TS-480"
    RADIO_G90 -> "Xiegu G90"
    RADIO_ICOM -> "Icom CI-V"
    RADIO_YAESU -> "Yaesu FT-891/991"
    RADIO_TRUSDX -> "(tr)uSDX"
    else -> "(None)"
}

fun radioHelp(id: Int): String = when (id) {
    RADIO_NONE -> "No radio is selected. xara does not send CAT and does not assume a PTT circuit. Use the microphone and speaker, with VOX if the radio keys from audio."
    RADIO_G90 -> "Xiegu G90 CAT is CI-V at 19200 baud, address 70. xara can set frequency, mode, and PTT."
    RADIO_KENWOOD -> "Kenwood TS-480 style commands: frequency, mode, and TX/RX PTT. Also used by radios that emulate that set."
    RADIO_TRUSDX -> "(tr)uSDX sends and receives audio on the CAT port, and uses Kenwood-style control commands. DTR is held high. Firmware 2.00t and later uses 115200 baud."
    RADIO_ICOM -> "Icom CI-V. The address defaults to 94, which is an IC-7300. Change it to match the radio. CAT PTT uses command 1C 00."
    RADIO_YAESU -> "Yaesu FT-891 and FT-991 ASCII CAT. Frequency is 9 digits. TX1 keys, TX0 unkeys."
    else -> ""
}

fun modeHelp(mode: Int): String = when (mode) {
    0 -> "Narrowest VARA-compatible HF bandwidth, 500 Hz. Use this when the channel is tight."
    1 -> "Standard VARA-compatible HF bandwidth, 2300 Hz."
    2 -> "Widest VARA-compatible HF bandwidth, 2750 Hz."
    3 -> "VARA-compatible FM, narrow. A client should use the FM session type."
    else -> "VARA-compatible FM, wide. A client should use the FM session type."
}

fun pttHelp(ptt: Int): String = when (ptt) {
    PTT_RTS -> "Keys the radio by raising RTS on the USB serial port."
    PTT_DTR -> "Keys the radio by raising DTR on the USB serial port."
    PTT_CAT -> "Keys the radio with a CAT transmit command. Pick a radio type that supports it."
    PTT_TRUSDX -> "Sends microphone audio over the (tr)uSDX CAT port instead of the phone speaker."
    else -> "xara does not key the radio. The radio must key itself from the audio, or you can tap Key."
}
