package io.github.x0rro.xara

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import java.io.File
import java.net.Inet4Address
import java.net.NetworkInterface
import java.security.SecureRandom

enum class Mode(val label: String, val options: List<String>) {
    HF500("HF 500 Hz", listOf("--hf", "500")),
    HF2300("HF 2300 Hz", listOf("--hf", "2300")),
    HF2750("HF 2750 Hz", listOf("--hf", "2750")),
    FM_NARROW("FM narrow", listOf("--fm", "narrow")),
    FM_WIDE("FM wide", listOf("--fm", "wide"));

    val isHF: Boolean get() = this == HF500 || this == HF2300 || this == HF2750
}

enum class Ptt { VOX, RIGCTLD }

/** How the radio is connected: its audio through a USB sound card, or a (tr)uSDX's audio over its CAT port. */
enum class Hookup(val label: String) {
    SOUND_CARD("USB sound card (Digirig or similar)"),
    TRUSDX("(tr)uSDX: audio over CAT on USB"),
}

const val COMMAND_PORT = 8300
const val KISS_PORT = 8100
const val UI_PORT = 8080

/** What the app asks xara to run as; turned into the same options the `xara` command takes. */
data class ModemConfig(
    val call: String,
    val mode: Mode,
    val ptt: Ptt,
    val rigctld: String,
    val driveDb: Int,
    val lan: Boolean,
    val uiControl: Boolean,
    val uiToken: String,
    val capture: Int,
    val playback: Int,
    val stateDir: File,
    val monitor: Boolean = false,
    val monitorFrames: Boolean = false,
    val monitorCalls: List<String> = emptyList(),
    /** A timestamped text log of the modem: frames, host commands, audio levels once a second. */
    val debugLog: File? = null,
    /** A FLAC recording of the audio heard and sent, with PTT, for troubleshooting. */
    val record: File? = null,
    /** Registered VARA speeds, for a user with a VARA licence. */
    val registered: Boolean = false,
    val hookup: Hookup = Hookup.SOUND_CARD,
    /** (tr)uSDX speaker plays the CAT audio stream. */
    val radioSpeaker: Boolean = false,
    /** This phone's speaker plays what the modem receives and transmits. */
    val phoneListen: Boolean = false,
) {
    val callValid: Boolean
        get() = call.uppercase().matches(Regex("^[A-Z0-9]{3,7}(-[0-9]{1,2})?$")) && !call.uppercase().startsWith("N0CALL")

    fun arguments(): Array<String> {
        val bind = if (lan) "0.0.0.0" else "127.0.0.1"
        val a = mutableListOf("--call", call.uppercase())
        a += mode.options
        if (hookup == Hookup.TRUSDX) {
            // The radio's serial port, which TrusdxPort carries; PTT is TX0; and RX; in the same stream.
            a += listOf("--audio", "trusdx", "--capture", "app", "--playback", "app", "--radio", "--ptt", "trusdx")
            if (radioSpeaker) a += "--trusdx-speaker"
        } else {
            a += listOf("--audio", "aaudio", "--capture", "$capture", "--playback", "$playback", "--radio")
            a += listOf("--ptt", if (ptt == Ptt.VOX) "vox" else "rigctld:$rigctld")
        }
        a += listOf("--drive-db", "$driveDb")
        a += listOf("--addr", bind, "--port", "$COMMAND_PORT", "--kiss-port", "$KISS_PORT")
        a += listOf("--ui-port", "$UI_PORT", "--ui-addr", bind)
        if (uiControl) a += listOf("--ui-control", "--ui-token", uiToken)
        if (monitor && mode.isHF) {
            a += "--monitor"
            if (monitorFrames) a += "--monitor-frames"
            monitorCalls.forEach { a += listOf("--monitor-call", it) }
        }
        debugLog?.let { a += listOf("--debug-log", it.path) }
        record?.let { a += listOf("--record", it.path) }
        if (registered) a += "--registered"
        a += listOf("--state-dir", stateDir.path)
        return a.toTypedArray()
    }
}

/** Settings kept across launches. */
class Prefs(context: Context) {
    private val p = context.getSharedPreferences("xara", Context.MODE_PRIVATE)
    private val stateDir = File(context.filesDir, "xara")
    /** Troubleshooting files, in the app's own storage (`adb shell run-as io.github.x0rro.xara`). */
    val debugLogFile = File(context.filesDir, "debug.log")
    val recordingsDir = File(context.filesDir, "recordings")

    var call by string("call", "")
    var mode by string("mode", Mode.HF2300.name)
    var ptt by string("ptt", Ptt.VOX.name)
    var rigctld by string("rigctld", "192.168.1.10:4532")
    var driveDb by int("driveDb", -5)
    var lan by bool("lan", false)
    var uiControl by bool("uiControl", false)
    var allowBuiltIn by bool("allowBuiltIn", false)
    var capture by int("capture", 0)
    var playback by int("playback", 0)
    var monitor by bool("monitor", false)
    var monitorFrames by bool("monitorFrames", false)
    var monitorCalls by string("monitorCalls", "")
    var debugLog by bool("debugLog", false)
    var recordAudio by bool("recordAudio", false)
    var registered by bool("registered", false)
    var hookup by string("hookup", Hookup.SOUND_CARD.name)
    var radioSpeaker by bool("radioSpeaker", false)
    var phoneListen by bool("phoneListen", false)

    /** A token kept across launches, so a bookmarked controlling web UI keeps working. */
    val uiToken: String
        get() = p.getString("uiToken", null) ?: run {
            val chars = ('A'..'Z') + ('a'..'z') + ('0'..'9')
            val r = SecureRandom()
            String(CharArray(32) { chars[r.nextInt(chars.size)] }).also { p.edit().putString("uiToken", it).apply() }
        }

    fun config() = ModemConfig(
        call, Mode.valueOf(mode), Ptt.valueOf(ptt), rigctld, driveDb, lan, uiControl, uiToken,
        capture, playback, stateDir, monitor, monitorFrames,
        monitorCalls.uppercase().split(' ', ',').filter { it.isNotBlank() },
        debugLog = debugLogFile.takeIf { debugLog },
        record = if (recordAudio) {
            recordingsDir.mkdirs()
            File(recordingsDir, "xara-${java.text.SimpleDateFormat("yyyyMMdd-HHmmss", java.util.Locale.US).format(java.util.Date())}.flac")
        } else {
            null
        },
        registered = registered,
        hookup = Hookup.valueOf(hookup),
        radioSpeaker = radioSpeaker,
        phoneListen = phoneListen,
    )

    private fun string(k: String, d: String) = object : kotlin.properties.ReadWriteProperty<Any?, String> {
        override fun getValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>) = p.getString(k, d) ?: d
        override fun setValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>, value: String) =
            p.edit().putString(k, value).apply()
    }

    private fun int(k: String, d: Int) = object : kotlin.properties.ReadWriteProperty<Any?, Int> {
        override fun getValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>) = p.getInt(k, d)
        override fun setValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>, value: Int) =
            p.edit().putInt(k, value).apply()
    }

    private fun bool(k: String, d: Boolean) = object : kotlin.properties.ReadWriteProperty<Any?, Boolean> {
        override fun getValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>) = p.getBoolean(k, d)
        override fun setValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>, value: Boolean) =
            p.edit().putBoolean(k, value).apply()
    }
}

/** An audio device as the app offers it; xara opens it by id. */
data class Device(val id: Int, val label: String, val builtIn: Boolean, val usb: Boolean, val speaker: Boolean = false)

object AudioDevices {
    private val builtInTypes = setOf(
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, AudioDeviceInfo.TYPE_BUILTIN_EARPIECE,
        AudioDeviceInfo.TYPE_BUILTIN_MIC, AudioDeviceInfo.TYPE_TELEPHONY,
    )
    private val usbTypes = setOf(
        AudioDeviceInfo.TYPE_USB_DEVICE, AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_ACCESSORY,
    )

    fun list(context: Context, inputs: Boolean): List<Device> {
        val am = context.getSystemService(AudioManager::class.java)
        val flag = if (inputs) AudioManager.GET_DEVICES_INPUTS else AudioManager.GET_DEVICES_OUTPUTS
        return am.getDevices(flag).map {
            // Built-in devices all carry the phone's model as their product name, so name them
            // by what they are: the earpiece is far too quiet to be heard by another device.
            val label = when (it.type) {
                AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "Phone speaker"
                AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "Phone earpiece"
                AudioDeviceInfo.TYPE_BUILTIN_MIC -> "Phone mic" + (it.address.takeIf { a -> a.isNotEmpty() }?.let { a -> " ($a)" } ?: "")
                AudioDeviceInfo.TYPE_TELEPHONY -> "Phone call audio"
                in usbTypes -> "${it.productName} (USB)"
                else -> "${it.productName} (type ${it.type})"
            }
            Device(it.id, label, it.type in builtInTypes, it.type in usbTypes, it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER)
        }
    }
}

/** This device's IPv4 addresses, for clients on the LAN. */
fun lanAddresses(): List<String> = runCatching {
    NetworkInterface.getNetworkInterfaces().toList()
        .filter { it.isUp && !it.isLoopback }
        .flatMap { it.inetAddresses.toList() }
        .filterIsInstance<Inet4Address>()
        .map { it.hostAddress ?: "" }
        .filter { it.isNotEmpty() }
}.getOrDefault(emptyList())
