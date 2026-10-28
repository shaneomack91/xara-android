package io.github.x0rro.xara

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.BroadcastReceiver
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbManager
import android.content.pm.PackageManager
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Bundle
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.ChatBubbleOutline
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.SettingsInputAntenna
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuAnchorType
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.background
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val accent = Color(0xFFF28C26)
            val scheme = if (isSystemInDarkTheme()) darkColorScheme(primary = accent) else lightColorScheme(primary = accent)
            MaterialTheme(colorScheme = scheme) { App() }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun App() {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    val titles = listOf("xara", "Web UI", "Received", "Log")
    Scaffold(
        topBar = {
            TopAppBar(title = { Text(titles[tab]) }, actions = {
                if (tab == 2) ReceivedActions()
                if (tab == 3) LogActions()
            })
        },
        bottomBar = {
            NavigationBar {
                NavigationBarItem(tab == 0, { tab = 0 }, { Icon(Icons.Default.SettingsInputAntenna, null) }, label = { Text("Modem") })
                NavigationBarItem(tab == 1, { tab = 1 }, { Icon(Icons.Default.GraphicEq, null) }, label = { Text("Web UI") })
                NavigationBarItem(tab == 2, { tab = 2 }, { Icon(Icons.Default.ChatBubbleOutline, null) }, label = { Text("Received") })
                NavigationBarItem(tab == 3, { tab = 3 }, { Icon(Icons.AutoMirrored.Default.List, null) }, label = { Text("Log") })
            }
        },
    ) { pad ->
        Box(Modifier.padding(pad).fillMaxSize()) {
            when (tab) {
                0 -> ModemScreen()
                1 -> WebScreen()
                2 -> ReceivedScreen()
                else -> LogScreen()
            }
        }
    }
}

/** Keeps the device lists current as interfaces are plugged in and out. */
@Composable
fun rememberDevices(context: Context, inputs: Boolean): List<Device> {
    var devices by remember { mutableStateOf(AudioDevices.list(context, inputs)) }
    DisposableEffect(inputs) {
        val am = context.getSystemService(AudioManager::class.java)
        val cb = object : AudioDeviceCallback() {
            override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) {
                devices = AudioDevices.list(context, inputs)
            }

            override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) {
                devices = AudioDevices.list(context, inputs)
            }
        }
        am.registerAudioDeviceCallback(cb, null)
        onDispose { am.unregisterAudioDeviceCallback(cb) }
    }
    return devices
}

/** USB serial adapters, kept current as they are plugged in and out. */
@Composable
fun rememberSerialPorts(context: Context): List<String> {
    var ports by remember { mutableStateOf(TrusdxPort.drivers(context).map(TrusdxPort::label)) }
    DisposableEffect(Unit) {
        val r = object : BroadcastReceiver() {
            override fun onReceive(c: Context, i: Intent) {
                ports = TrusdxPort.drivers(context).map(TrusdxPort::label)
            }
        }
        val f = IntentFilter(UsbManager.ACTION_USB_DEVICE_ATTACHED).apply { addAction(UsbManager.ACTION_USB_DEVICE_DETACHED) }
        ContextCompat.registerReceiver(context, r, f, ContextCompat.RECEIVER_NOT_EXPORTED)
        onDispose { context.unregisterReceiver(r) }
    }
    return ports
}

@Composable
fun ModemScreen() {
    val context = LocalContext.current
    val prefs = remember { Prefs(context) }
    val running by Modem.running.collectAsState()
    val status by Modem.status.collectAsState()
    var call by remember { mutableStateOf(prefs.call) }
    var mode by remember { mutableStateOf(Mode.valueOf(prefs.mode)) }
    var ptt by remember { mutableStateOf(Ptt.valueOf(prefs.ptt)) }
    var rigctld by remember { mutableStateOf(prefs.rigctld) }
    var drive by remember { mutableIntStateOf(prefs.driveDb) }
    var lan by remember { mutableStateOf(prefs.lan) }
    var uiControl by remember { mutableStateOf(prefs.uiControl) }
    var allowBuiltIn by remember { mutableStateOf(prefs.allowBuiltIn) }
    var monitor by remember { mutableStateOf(prefs.monitor) }
    var monitorFrames by remember { mutableStateOf(prefs.monitorFrames) }
    var monitorCalls by remember { mutableStateOf(prefs.monitorCalls) }
    var debugLog by remember { mutableStateOf(prefs.debugLog) }
    var recordAudio by remember { mutableStateOf(prefs.recordAudio) }
    var registered by remember { mutableStateOf(prefs.registered) }
    var hookup by remember { mutableStateOf(Hookup.valueOf(prefs.hookup)) }
    var radioSpeaker by remember { mutableStateOf(prefs.radioSpeaker) }
    var phoneListen by remember { mutableStateOf(prefs.phoneListen) }
    val serialPorts = rememberSerialPorts(context)
    var problem by remember { mutableStateOf<String?>(null) }
    val inputs = rememberDevices(context, true)
    val outputs = rememberDevices(context, false)
    var capture by remember { mutableIntStateOf(prefs.capture) }
    var playback by remember { mutableIntStateOf(prefs.playback) }
    // Default to a USB interface when nothing usable is chosen; without one, the phone's
    // speaker rather than the earpiece.
    LaunchedEffect(inputs, outputs) {
        if (inputs.none { it.id == capture }) capture = (inputs.firstOrNull { it.usb } ?: inputs.firstOrNull())?.id ?: 0
        if (outputs.none { it.id == playback }) {
            playback = (outputs.firstOrNull { it.usb } ?: outputs.firstOrNull { it.speaker } ?: outputs.firstOrNull())?.id ?: 0
        }
    }

    val start = {
        prefs.call = call; prefs.mode = mode.name; prefs.ptt = ptt.name; prefs.rigctld = rigctld
        prefs.driveDb = drive; prefs.lan = lan; prefs.uiControl = uiControl; prefs.allowBuiltIn = allowBuiltIn
        prefs.capture = capture; prefs.playback = playback
        prefs.monitor = monitor; prefs.monitorFrames = monitorFrames; prefs.monitorCalls = monitorCalls
        prefs.debugLog = debugLog; prefs.recordAudio = recordAudio; prefs.registered = registered
        prefs.hookup = hookup.name; prefs.radioSpeaker = radioSpeaker; prefs.phoneListen = phoneListen
        val builtIn = listOfNotNull(inputs.find { it.id == capture }, outputs.find { it.id == playback }).filter { it.builtIn }
        problem = when {
            hookup == Hookup.TRUSDX -> if (serialPorts.isEmpty()) "No USB serial adapter found. Plug in the (tr)uSDX." else null
            inputs.none { it.id == capture } || outputs.none { it.id == playback } -> "Choose an audio input and output."
            builtIn.isNotEmpty() && !allowBuiltIn ->
                "Not transmitting into ${builtIn.first().label}. Connect your radio's USB audio interface."
            else -> null
        }
        if (problem == null && hookup == Hookup.TRUSDX) {
            val driver = TrusdxPort.drivers(context).first()
            TrusdxPort.requestPermission(context, driver.device) { granted ->
                if (granted) Modem.start(context, prefs.config()) else problem = "USB access to the (tr)uSDX was not given."
            }
        } else if (problem == null) {
            if (debugLog) {
                // What only the app knows: the devices by name and the volume the phone plays at.
                val am = context.getSystemService(AudioManager::class.java)
                val music = AudioManager.STREAM_MUSIC
                prefs.debugLogFile.appendText(
                    "android: start ${java.util.Date()}, in ${inputs.find { it.id == capture }?.label} ($capture), " +
                        "out ${outputs.find { it.id == playback }?.label} ($playback), media volume " +
                        "${am.getStreamVolume(music)}/${am.getStreamMaxVolume(music)}, drive $drive dB\n",
                )
            }
            Modem.start(context, prefs.config())
        }
    }
    val micLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { got ->
        if (got[Manifest.permission.RECORD_AUDIO] == true) start() else problem = "Microphone access is needed to hear the radio."
    }
    val editable = !running

    Column(Modifier.verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(10.dp).background(if (running) Color(0xFF34C759) else Color.Gray, CircleShape))
                    Spacer(Modifier.size(8.dp))
                    Text(problem ?: status)
                }
                Button(
                    onClick = {
                        if (running) {
                            Modem.stop(context)
                        } else {
                            val need = listOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.POST_NOTIFICATIONS)
                            // A (tr)uSDX's audio comes over USB serial, so it needs no microphone access.
                            if (hookup == Hookup.TRUSDX ||
                                context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
                            ) {
                                start()
                            } else {
                                micLauncher.launch(need.toTypedArray())
                            }
                        }
                    },
                    enabled = running || ModemConfig(call, mode, ptt, rigctld, drive, lan, uiControl, "", 0, 0, context.filesDir).callValid,
                    colors = if (running) ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error) else ButtonDefaults.buttonColors(),
                ) { Text(if (running) "Stop" else "Start") }
            }
        }

        Section("Station")
        OutlinedTextField(
            call, { call = it.uppercase() }, Modifier.fillMaxWidth(), enabled = editable, label = { Text("Call sign") },
            singleLine = true, keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Characters, autoCorrectEnabled = false),
        )
        Choice("Mode", Mode.entries, mode, { it.label }, editable) { mode = it }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("TX drive  $drive dB", Modifier.weight(1f))
            TextButton({ drive = (drive - 1).coerceAtLeast(-30) }, enabled = editable) { Text("−") }
            TextButton({ drive = (drive + 1).coerceAtMost(0) }, enabled = editable) { Text("+") }
        }
        Toggle("Registered (VARA licence)", registered, editable) { registered = it }
        Note("Only with a registered copy of VARA: higher speeds, and links at full speed with other registered stations.")

        Section("Radio")
        Choice("Connection", Hookup.entries, hookup, { it.label }, editable) { hookup = it }
        if (hookup == Hookup.TRUSDX) {
            Text("Serial port  ${serialPorts.firstOrNull() ?: "none plugged in"}")
            Toggle("(tr)uSDX speaker", radioSpeaker, editable) { radioSpeaker = it }
            Note(
                "The (tr)uSDX sends and receives its audio as data on its USB serial (CAT) port, so no sound card " +
                    "or microphone is used, and xara keys it over CAT. It needs the DL2MAN / PE1NNZ firmware with " +
                    "CAT streaming. The radio's speaker stays off unless that switch is on.",
            )
        } else {
            Choice("Audio in", inputs, inputs.find { it.id == capture }, { it?.label ?: "none" }, editable) { capture = it?.id ?: 0 }
            Choice("Audio out", outputs, outputs.find { it.id == playback }, { it?.label ?: "none" }, editable) { playback = it?.id ?: 0 }
            Choice("PTT", Ptt.entries, ptt, { if (it == Ptt.VOX) "VOX" else "rigctld (network CAT)" }, editable) { ptt = it }
            if (ptt == Ptt.RIGCTLD) {
                OutlinedTextField(
                    rigctld, { rigctld = it }, Modifier.fillMaxWidth(), enabled = editable, label = { Text("rigctld host:port") },
                    singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
                )
            }
            Toggle("Allow the phone's own speaker and mic", allowBuiltIn, editable) { allowBuiltIn = it }
            Note(
                "xara transmits into the audio output chosen above. Connect a USB audio interface such as a Digirig; " +
                    "with the toggle off, xara will not start on the phone's own speaker or mic. Key the radio with VOX " +
                    "or rigctld.",
            )
        }
        Toggle("Hear audio on this phone", phoneListen, editable) { phoneListen = it }
        Note("Plays what the modem receives and what it transmits through this phone's speaker.")

        Section("Receive")
        Toggle("Show other stations' traffic", monitor, editable && mode.isHF) { monitor = it }
        if (monitor && mode.isHF) {
            Choice("Show", listOf(false, true), monitorFrames, { if (it) "Every packet" else "Text only" }, editable) {
                monitorFrames = it
            }
            OutlinedTextField(
                monitorCalls, { monitorCalls = it.uppercase() }, Modifier.fillMaxWidth(), enabled = editable,
                label = { Text("Calls to name (optional)") }, singleLine = true,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Characters, autoCorrectEnabled = false),
            )
        }
        Note(
            if (mode.isHF) {
                "The Received tab always shows your own links. With this on it also decodes VARA HF traffic " +
                    "between other stations, like VARA's monitor. Answering stations show as #hash unless their " +
                    "call is listed. Every packet lists each frame heard."
            } else {
                "Showing other stations' traffic is for VARA HF; FM shows your own links only."
            },
        )

        Section("Troubleshooting")
        Toggle("Keep a debug log", debugLog, editable) { debugLog = it }
        Toggle("Record received audio", recordAudio, editable) { recordAudio = it }
        Note(
            "The debug log adds a timestamped line for every frame heard and sent, every host command and, " +
                "once a second, the receive and transmit levels, to debug.log in the app's storage. Recording " +
                "saves what the modem hears and sends (about 3 MB a minute) to recordings/. Both are read with " +
                "adb shell run-as io.github.x0rro.xara.",
        )

        Section("Clients")
        Toggle("Accept clients on the local network", lan, editable) { lan = it }
        Toggle("Web UI can control the radio", uiControl, editable) { uiControl = it }
        Note(
            if (lan) {
                val hosts = lanAddresses().ifEmpty { listOf("(no network address)") }
                "Point Winlink Express, Pat or another VARA client at ${hosts.joinToString { "$it:$COMMAND_PORT" }} " +
                    "(data ${COMMAND_PORT + 1}, KISS $KISS_PORT). Anyone on this network can use the modem."
            } else {
                "VARA command port $COMMAND_PORT, data ${COMMAND_PORT + 1}, KISS $KISS_PORT, on this phone only."
            },
        )
        Note("xara ${Native.version()}")
    }
}

@Composable
fun Section(title: String) = Text(title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)

@Composable
fun Note(text: String) = Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)

@Composable
fun Toggle(label: String, on: Boolean, enabled: Boolean, set: (Boolean) -> Unit) =
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(label, Modifier.weight(1f))
        Switch(on, set, enabled = enabled)
    }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun <T> Choice(label: String, options: List<T>, chosen: T, name: (T) -> String, enabled: Boolean, pick: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(open, { if (enabled) open = it }) {
        OutlinedTextField(
            name(chosen), {}, Modifier.fillMaxWidth().menuAnchor(MenuAnchorType.PrimaryNotEditable),
            readOnly = true, enabled = enabled, label = { Text(label) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(open) },
        )
        ExposedDropdownMenu(open, { open = false }) {
            options.forEach { DropdownMenuItem({ Text(name(it)) }, { pick(it); open = false }) }
        }
    }
}

@SuppressLint("SetJavaScriptEnabled")
@Composable
fun WebScreen() {
    val context = LocalContext.current
    val running by Modem.running.collectAsState()
    var reload by remember { mutableIntStateOf(0) }
    if (!running) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text("Start the modem to see its waterfall and status.", color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        return
    }
    val prefs = remember { Prefs(context) }
    val url = "http://127.0.0.1:$UI_PORT/" + if (prefs.uiControl) "?token=${prefs.uiToken}" else ""
    Box(Modifier.fillMaxSize()) {
        AndroidView(
            factory = {
                WebView(it).apply {
                    settings.javaScriptEnabled = true
                    // The UI server comes up a moment after Start; retry until it answers.
                    webViewClient = object : WebViewClient() {
                        override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                            if (request.isForMainFrame) view.postDelayed({ view.loadUrl(url) }, 1000)
                        }
                    }
                    loadUrl(url)
                }
            },
            update = { if (reload > 0) it.loadUrl(url) },
            modifier = Modifier.fillMaxSize(),
        )
        IconButton({ reload++ }, Modifier.align(Alignment.TopEnd)) { Icon(Icons.Default.Refresh, "Reload") }
    }
}

@Composable
fun ReceivedScreen() {
    val received by Modem.received.collectAsState()
    val scroll = rememberScrollState()
    LaunchedEffect(received.length) { scroll.animateScrollTo(scroll.maxValue) }
    Surface(Modifier.fillMaxSize()) {
        Text(
            received.ifEmpty { "Nothing received yet." },
            Modifier.verticalScroll(scroll).padding(12.dp),
            fontFamily = FontFamily.Monospace, fontSize = 13.sp,
            color = if (received.isEmpty()) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
        )
    }
}

@Composable
fun ReceivedActions() {
    val context = LocalContext.current
    val received by Modem.received.collectAsState()
    TextButton({
        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, received)
        context.startActivity(Intent.createChooser(send, "Share received text"))
    }) { Text("Share") }
    TextButton({ Modem.clearReceived() }) { Text("Clear") }
}

@Composable
fun LogScreen() {
    val log by Modem.log.collectAsState()
    val state = rememberLazyListState()
    LaunchedEffect(log.size) { if (log.isNotEmpty()) state.scrollToItem(log.size - 1) }
    Surface(Modifier.fillMaxSize()) {
        LazyColumn(state = state, contentPadding = androidx.compose.foundation.layout.PaddingValues(12.dp)) {
            items(log) { Text(it, fontFamily = FontFamily.Monospace, fontSize = 12.sp) }
        }
    }
}

@Composable
fun LogActions() {
    val context = LocalContext.current
    val log by Modem.log.collectAsState()
    TextButton({
        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, log.joinToString("\n"))
        context.startActivity(Intent.createChooser(send, "Share log"))
    }) { Text("Share") }
    TextButton({ Modem.clearLog() }) { Text("Clear") }
}
