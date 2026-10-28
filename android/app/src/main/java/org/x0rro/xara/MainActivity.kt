package org.x0rro.xara

import android.Manifest
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.hardware.usb.UsbManager
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.view.ViewGroup
import android.widget.AdapterView
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.text.Editable
import android.text.TextWatcher
import android.widget.TextView
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import com.google.android.material.appbar.MaterialToolbar
import com.hoho.android.usbserial.driver.UsbSerialProber
import kotlin.math.max

class MainActivity : AppCompatActivity() {
    private val prefs by lazy { getSharedPreferences("xara", MODE_PRIVATE) }
    private var modem: ModemService? = null
    private var bound = false
    private var pendingStart = false
    private var page = Page.Main

    private lateinit var call: EditText
    private lateinit var mode: Spinner
    private lateinit var modeNote: TextView
    private lateinit var capture: Spinner
    private lateinit var playback: Spinner
    private lateinit var serial: Spinner
    private lateinit var baud: Spinner
    private lateinit var ptt: Spinner
    private lateinit var pttNote: TextView
    private lateinit var radio: Spinner
    private lateinit var radioNote: TextView
    private lateinit var civ: EditText
    private lateinit var civLabel: TextView
    private lateinit var civNote: TextView
    private lateinit var speaker: CheckBox
    private lateinit var freqBlock: View
    private lateinit var freq: EditText
    private lateinit var radioMode: Spinner
    private lateinit var bind: EditText
    private lateinit var cmdPort: EditText
    private lateinit var dataNote: TextView
    private lateinit var kissPort: EditText
    private lateinit var lead: EditText
    private lateinit var tail: EditText
    private lateinit var maxOn: EditText
    private lateinit var status: TextView
    private lateinit var summary: TextView
    private lateinit var key: Button
    private lateinit var pageMain: View
    private lateinit var pageAudio: View
    private lateinit var pageRadio: View
    private lateinit var pageTnc: View

    private var captures = listOf(-1 to "System default")
    private var playbacks = listOf(-1 to "System default")
    private var ports = listOf("" to "No USB serial device")
    private var suppressBaud = false
    private var currentRadio = RADIO_NONE

    private val backToMain = object : OnBackPressedCallback(false) {
        override fun handleOnBackPressed() = showPage(Page.Main)
    }

    private val micPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (!pendingStart) return@registerForActivityResult
        pendingStart = false
        if (!granted && ptt.selectedItemPosition != PTT_TRUSDX) {
            status.text = "Microphone permission is required for sound-card audio."
            showPage(Page.Main)
            return@registerForActivityResult
        }
        if (usbReady()) begin()
    }

    private val notifyPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false) == true && pendingStart) {
                pendingStart = false
                begin()
            } else if (pendingStart) {
                pendingStart = false
                status.text = "USB permission was denied."
                showPage(Page.Main)
            }
            refreshDevices()
        }
    }

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            modem = (binder as ModemService.Local).service()
            if (pendingStart) {
                pendingStart = false
                modem?.startModem(readConfig())
            }
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            modem = null
        }
    }

    private val tick = object : Runnable {
        override fun run() {
            val running = modem?.running() == true
            if (running) status.text = modem?.snapshot()
            key.isEnabled = running
            status.postDelayed(this, 500)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        onBackPressedDispatcher.addCallback(this, backToMain)

        call = field(prefs.getString("call", "N0CALL") ?: "N0CALL")
        mode = spin(listOf("HF 500", "HF 2300", "HF 2750", "FM narrow", "FM wide"), prefs.getInt("mode", 1))
        modeNote = note("")
        capture = Spinner(this)
        playback = Spinner(this)
        serial = Spinner(this)
        baud = spin(listOf("4800", "9600", "19200", "38400", "57600", "115200"), savedBaudIndex())
        ptt = spin(listOf("RTS", "DTR", "CAT", "(tr)uSDX audio", "VOX"), prefs.getInt("pttMode", PTT_VOX))
        pttNote = note("")
        radio = Spinner(this)
        radioNote = note("")
        civ = field(prefs.getString("civ", "70") ?: "70")
        civLabel = label("CI-V address")
        civNote = note("Hex address of an Icom CI-V radio. A Xiegu G90 uses 70. An IC-7300 uses 94.")
        speaker = CheckBox(this).apply {
            text = "Play (tr)uSDX speaker while audio is on the CAT port"
            isChecked = prefs.getBoolean("speaker", false)
        }
        freq = field(prefs.getString("freq", "14074000") ?: "14074000")
        radioMode = spin(listOf("LSB", "USB", "CW", "CW-R", "FM", "AM", "DIG", "DATA-R"), prefs.getInt("rmode", 1))
        bind = field(prefs.getString("bind", "0.0.0.0") ?: "0.0.0.0")
        cmdPort = field(prefs.getInt("cmd", 8300).toString())
        dataNote = note("")
        kissPort = field(prefs.getInt("kiss", 8001).toString())
        lead = field(prefs.getInt("lead", 0).toString())
        tail = field(prefs.getInt("tail", 100).toString())
        maxOn = field(prefs.getInt("max", 30).toString())
        status = TextView(this).apply { setTextIsSelectable(true) }
        summary = note("")
        key = Button(this).apply {
            text = "Key"
            isEnabled = false
        }
        val start = Button(this).apply { text = "Start" }
        val stop = Button(this).apply { text = "Stop" }
        val refreshAudio = Button(this).apply { text = "Refresh devices" }
        val refreshRadio = Button(this).apply { text = "Refresh devices" }
        val setRadio = Button(this).apply { text = "Set frequency and mode" }
        val readRadio = Button(this).apply { text = "Read radio" }

        pageMain = scroll(
            label("Callsign"),
            call,
            note("Sent to the other station. xara will not start as N0CALL."),
            label("VARA-compatible mode"),
            mode,
            modeNote,
            row(start, stop, key),
            note("Key forces transmit until you tap Unkey. With VOX and no radio, the radio has to key itself."),
            summary,
            label("Status"),
            status,
        )
        pageAudio = scroll(
            label("Audio input"),
            capture,
            note("Where xara listens. System default is the phone microphone."),
            label("Audio output"),
            playback,
            note("Where decoded audio is played. System default is the phone speaker."),
            refreshAudio,
        )
        freqBlock = column(
            label("Frequency (Hz)"),
            freq,
            note("CAT frequency. Used only when a radio type is selected and a USB serial port is open."),
            label("Radio mode"),
            radioMode,
            row(setRadio, readRadio),
            pad = false,
        )
        pageRadio = scroll(
            label("Radio"),
            radio,
            radioNote,
            label("PTT"),
            ptt,
            pttNote,
            label("USB serial"),
            serial,
            note("Optional. RTS, DTR, CAT, and (tr)uSDX audio need this. Leave it unset to use the microphone and speaker only."),
            label("Baud"),
            baud,
            note("Serial speed for the selected radio. Ignored when the radio is (None) and PTT is VOX."),
            refreshRadio,
            civLabel,
            civ,
            civNote,
            speaker,
            freqBlock,
        )
        pageTnc = scroll(
            label("Listen address"),
            bind,
            note("0.0.0.0 accepts a client on this phone and from other devices on the network."),
            label("Command port"),
            cmdPort,
            note("TCP port for the VARA-compatible command channel. The usual value is 8300."),
            dataNote,
            label("KISS port"),
            kissPort,
            note("TCP port for a KISS TNC client. 0 turns it off. The usual value is 8001. Frames are AX.25 inside the VARA-compatible modem, not 1200-baud packet."),
            label("PTT lead (ms)"),
            lead,
            note("How long xara keys before it sends audio."),
            label("PTT tail (ms)"),
            tail,
            note("How long xara stays keyed after the audio stops."),
            label("Max key-down (seconds)"),
            maxOn,
            note("Watchdog. xara unkeys if one transmission runs longer than this."),
        )

        val toolbar = MaterialToolbar(this, null, com.google.android.material.R.attr.toolbarStyle)
        pageAudio.visibility = View.GONE
        pageRadio.visibility = View.GONE
        pageTnc.visibility = View.GONE
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(
                toolbar,
                LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                ),
            )
            val fill = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
            addView(pageMain, fill)
            addView(pageAudio, LinearLayout.LayoutParams(fill))
            addView(pageRadio, LinearLayout.LayoutParams(fill))
            addView(pageTnc, LinearLayout.LayoutParams(fill))
        }
        setContentView(root)
        setSupportActionBar(toolbar)
        applySafeArea(toolbar, pageMain, pageAudio, pageRadio, pageTnc)

        fill(radio, radioChoices.map { it.second }, radioIndex(prefs.getInt("radioId", RADIO_NONE)))
        currentRadio = selectedRadio()
        mode.onItemSelectedListener = select { modeNote.text = modeHelp(it) }
        modeNote.text = modeHelp(mode.selectedItemPosition)
        radio.onItemSelectedListener = select { onRadioPicked(it) }
        ptt.onItemSelectedListener = select { item ->
            pttNote.text = pttHelp(item)
            if (item == PTT_TRUSDX && selectedRadio() != RADIO_TRUSDX) {
                radio.setSelection(radioIndex(RADIO_TRUSDX))
            }
        }
        pttNote.text = pttHelp(ptt.selectedItemPosition)
        onRadioPicked(radio.selectedItemPosition)
        cmdPort.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                refreshDataNote()
            }
        })
        refreshDataNote()

        start.setOnClickListener { armStart() }
        stop.setOnClickListener {
            pendingStart = false
            key.text = "Key"
            modem?.halt()
            showStopped()
            stopService(Intent(this, ModemService::class.java))
        }
        key.setOnClickListener {
            val on = key.text == "Key"
            key.text = if (on) "Unkey" else "Key"
            modem?.setManualKey(on)
        }
        refreshAudio.setOnClickListener { refreshDevices() }
        refreshRadio.setOnClickListener { refreshDevices() }
        setRadio.setOnClickListener {
            if (selectedRadio() == RADIO_NONE) {
                status.text = "No radio is selected, so xara does not send CAT."
                showPage(Page.Main)
                return@setOnClickListener
            }
            val hz = freq.text.toString().toLongOrNull() ?: return@setOnClickListener
            modem?.pushFreq(hz, radioMode.selectedItemPosition)
            save()
        }
        readRadio.setOnClickListener {
            if (selectedRadio() == RADIO_NONE) {
                status.text = "No radio is selected, so there is nothing to read."
                showPage(Page.Main)
                return@setOnClickListener
            }
            modem?.pollRadio(radioMode.selectedItemPosition)
        }

        ContextCompat.registerReceiver(this, usbReceiver, IntentFilter(USB_ACTION), ContextCompat.RECEIVER_NOT_EXPORTED)
        refreshDevices()
        showPage(Page.Main)
        showStopped()
    }

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menu.add(0, MENU_AUDIO, 0, "Audio")
        menu.add(0, MENU_RADIO, 1, "Radio")
        menu.add(0, MENU_TNC, 2, "TNC")
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        when (item.itemId) {
            android.R.id.home -> showPage(Page.Main)
            MENU_AUDIO -> showPage(Page.Audio)
            MENU_RADIO -> showPage(Page.Radio)
            MENU_TNC -> showPage(Page.Tnc)
            else -> return super.onOptionsItemSelected(item)
        }
        return true
    }

    override fun onStart() {
        super.onStart()
        if (!bound) {
            bound = bindService(Intent(this, ModemService::class.java), connection, 0)
        }
        status.post(tick)
    }

    override fun onStop() {
        status.removeCallbacks(tick)
        if (bound) {
            unbindService(connection)
            bound = false
            modem = null
        }
        super.onStop()
    }

    override fun onDestroy() {
        unregisterReceiver(usbReceiver)
        super.onDestroy()
    }

    private fun showPage(next: Page) {
        page = next
        pageMain.visibility = if (next == Page.Main) View.VISIBLE else View.GONE
        pageAudio.visibility = if (next == Page.Audio) View.VISIBLE else View.GONE
        pageRadio.visibility = if (next == Page.Radio) View.VISIBLE else View.GONE
        pageTnc.visibility = if (next == Page.Tnc) View.VISIBLE else View.GONE
        supportActionBar?.title = when (next) {
            Page.Main -> "xara"
            Page.Audio -> "Audio"
            Page.Radio -> "Radio"
            Page.Tnc -> "TNC"
        }
        supportActionBar?.setDisplayHomeAsUpEnabled(next != Page.Main)
        backToMain.isEnabled = next != Page.Main
        if (next == Page.Main) refreshSummary()
        if (next == Page.Tnc) refreshDataNote()
    }

    private fun showStopped() {
        if (!NativeModem.available) {
            status.text = BUILD_HELP
            return
        }
        refreshSummary()
        status.text = "Stopped."
    }

    private fun refreshSummary() {
        val cmd = cmdPort.text.toString().toIntOrNull() ?: 8300
        val kiss = kissPort.text.toString().toIntOrNull() ?: 8001
        val kissText = if (kiss == 0) "off" else kiss.toString()
        summary.text = "PTT ${ptt.selectedItem}. Radio ${radio.selectedItem}.\n" +
            "Command port $cmd, data port ${cmd + 1}, KISS port $kissText.\n" +
            "On this phone, a client uses host 127.0.0.1."
    }

    private fun refreshDataNote() {
        val cmd = cmdPort.text.toString().toIntOrNull() ?: 8300
        dataNote.text = "The data port is always the next port, ${cmd + 1}. A client connects to both."
    }

    private fun onRadioPicked(position: Int) {
        val id = radioChoices.getOrNull(position)?.first ?: RADIO_NONE
        radioNote.text = radioHelp(id)
        val civOn = id == RADIO_G90 || id == RADIO_ICOM
        civLabel.visibility = if (civOn) View.VISIBLE else View.GONE
        civ.visibility = if (civOn) View.VISIBLE else View.GONE
        civNote.visibility = if (civOn) View.VISIBLE else View.GONE
        speaker.visibility = if (id == RADIO_TRUSDX) View.VISIBLE else View.GONE
        freqBlock.visibility = if (id == RADIO_NONE) View.GONE else View.VISIBLE
        if (id == currentRadio) return
        currentRadio = id
        if (id == RADIO_G90 || id == RADIO_ICOM) {
            val cur = civ.text.toString().trim()
            if (cur.isEmpty() || cur == "70" || cur == "94") civ.setText(if (id == RADIO_G90) "70" else "94")
        }
        if (id == RADIO_TRUSDX && ptt.selectedItemPosition != PTT_TRUSDX) ptt.setSelection(PTT_TRUSDX)
        if (id == RADIO_NONE && (ptt.selectedItemPosition == PTT_CAT || ptt.selectedItemPosition == PTT_TRUSDX)) {
            ptt.setSelection(PTT_VOX)
        }
        if (id != RADIO_NONE) applyDefaultBaud(id)
    }

    private fun armStart() {
        save()
        if (!NativeModem.available) {
            status.text = BUILD_HELP
            showPage(Page.Main)
            return
        }
        val needsMic = ptt.selectedItemPosition != PTT_TRUSDX
        if (needsMic && ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            pendingStart = true
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            notifyPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        if (!usbReady()) return
        begin()
    }

    private fun usbReady(): Boolean {
        val name = ports.getOrNull(serial.selectedItemPosition)?.first.orEmpty()
        if (name.isEmpty()) return true
        val usb = getSystemService(UsbManager::class.java)
        val device = UsbSerialProber.getDefaultProber().findAllDrivers(usb).firstOrNull { it.device.deviceName == name }?.device
        if (device == null) {
            status.text = "USB serial device is gone. Refresh devices."
            showPage(Page.Main)
            return false
        }
        if (usb.hasPermission(device)) return true
        pendingStart = true
        val intent = Intent(USB_ACTION).setPackage(packageName)
        val flags = if (Build.VERSION.SDK_INT >= 31) android.app.PendingIntent.FLAG_MUTABLE else 0
        val pending = android.app.PendingIntent.getBroadcast(this, 0, intent, flags or android.app.PendingIntent.FLAG_UPDATE_CURRENT)
        usb.requestPermission(device, pending)
        status.text = "Allow USB access to the CAT cable."
        showPage(Page.Main)
        return false
    }

    private fun begin() {
        showPage(Page.Main)
        val intent = Intent(this, ModemService::class.java).putExtra("mic", ptt.selectedItemPosition != PTT_TRUSDX)
        ContextCompat.startForegroundService(this, intent)
        if (modem == null) {
            pendingStart = true
            if (bound) {
                unbindService(connection)
                bound = false
            }
            bound = bindService(intent, connection, BIND_AUTO_CREATE)
        } else {
            modem?.startModem(readConfig())
        }
    }

    private fun readConfig(): ModemConfig {
        val port = ports.getOrNull(serial.selectedItemPosition)?.first.orEmpty()
        return ModemConfig(
            call = call.text.toString().trim().ifEmpty { "N0CALL" },
            mode = mode.selectedItemPosition,
            bind = bind.text.toString().trim().ifEmpty { "0.0.0.0" },
            cmdPort = cmdPort.text.toString().toIntOrNull() ?: 8300,
            kissPort = kissPort.text.toString().toIntOrNull() ?: 8001,
            leadMs = lead.text.toString().toIntOrNull() ?: 0,
            tailMs = tail.text.toString().toIntOrNull() ?: 100,
            maxSec = maxOn.text.toString().toIntOrNull() ?: 30,
            ptt = ptt.selectedItemPosition,
            radio = selectedRadio(),
            civAddr = civ.text.toString().trim().toIntOrNull(16) ?: 0x70,
            baud = baud.selectedItem?.toString()?.toIntOrNull() ?: 9600,
            speaker = speaker.isChecked,
            captureId = captures.getOrNull(capture.selectedItemPosition)?.first ?: -1,
            playbackId = playbacks.getOrNull(playback.selectedItemPosition)?.first ?: -1,
            serialName = port,
            radioMode = radioMode.selectedItemPosition,
        )
    }

    private fun save() {
        prefs.edit()
            .putString("call", call.text.toString())
            .putInt("mode", mode.selectedItemPosition)
            .putInt("pttMode", ptt.selectedItemPosition)
            .putInt("radioId", selectedRadio())
            .putString("civ", civ.text.toString())
            .putBoolean("speaker", speaker.isChecked)
            .putString("freq", freq.text.toString())
            .putInt("rmode", radioMode.selectedItemPosition)
            .putString("bind", bind.text.toString())
            .putInt("cmd", cmdPort.text.toString().toIntOrNull() ?: 8300)
            .putInt("kiss", kissPort.text.toString().toIntOrNull() ?: 8001)
            .putInt("lead", lead.text.toString().toIntOrNull() ?: 0)
            .putInt("tail", tail.text.toString().toIntOrNull() ?: 100)
            .putInt("max", maxOn.text.toString().toIntOrNull() ?: 30)
            .putInt("baudIndex", baud.selectedItemPosition)
            .apply()
    }

    private fun selectedRadio(): Int = radioChoices.getOrNull(radio.selectedItemPosition)?.first ?: RADIO_NONE

    private fun radioIndex(id: Int): Int = radioChoices.indexOfFirst { it.first == id }.coerceAtLeast(0)

    private fun savedBaudIndex(): Int {
        if (prefs.contains("baudIndex")) return prefs.getInt("baudIndex", 1)
        return baudIndexFor(prefs.getInt("radioId", RADIO_NONE))
    }

    private fun refreshDevices() {
        val audio = getSystemService(AudioManager::class.java)
        val capSel = captures.getOrNull(capture.selectedItemPosition)?.first ?: prefs.getInt("cap", -1)
        val playSel = playbacks.getOrNull(playback.selectedItemPosition)?.first ?: prefs.getInt("play", -1)
        captures = audioNames(audio, true)
        playbacks = audioNames(audio, false)
        fill(capture, captures.map { it.second }, captures.indexOfFirst { it.first == capSel }.coerceAtLeast(0))
        fill(playback, playbacks.map { it.second }, playbacks.indexOfFirst { it.first == playSel }.coerceAtLeast(0))
        prefs.edit().putInt("cap", captures.getOrNull(capture.selectedItemPosition)?.first ?: -1)
            .putInt("play", playbacks.getOrNull(playback.selectedItemPosition)?.first ?: -1).apply()

        val usb = getSystemService(UsbManager::class.java)
        val found = serialDevices(usb)
        val keep = ports.getOrNull(serial.selectedItemPosition)?.first ?: prefs.getString("serial", "")
        ports = if (found.isEmpty()) listOf("" to "No USB serial device") else found
        fill(serial, ports.map { it.second }, ports.indexOfFirst { it.first == keep }.let { if (it < 0) 0 else it })
        prefs.edit().putString("serial", ports.getOrNull(serial.selectedItemPosition)?.first).apply()
    }

    private fun applyDefaultBaud(radioId: Int) {
        if (suppressBaud || radioId == RADIO_NONE) return
        val rate = if (NativeModem.available) NativeModem.radioBaud(radioId) else fallbackBaud(radioId)
        val labels = (0 until baud.count).map { baud.getItemAtPosition(it).toString() }
        val idx = labels.indexOf(rate.toString())
        if (idx >= 0) baud.setSelection(idx)
    }

    private fun fill(spinner: Spinner, labels: List<String>, index: Int) {
        suppressBaud = true
        spinner.adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, labels)
        spinner.setSelection(index.coerceIn(0, (labels.size - 1).coerceAtLeast(0)))
        suppressBaud = false
    }

    private fun spin(labels: List<String>, index: Int): Spinner = Spinner(this).also { fill(it, labels, index) }

    private fun field(value: String): EditText {
        return EditText(this).apply {
            setText(value)
            setSingleLine()
        }
    }

    private fun label(text: String): TextView {
        return TextView(this).apply {
            this.text = text
            setTextAppearance(com.google.android.material.R.style.TextAppearance_Material3_TitleSmall)
            setPadding(0, dp(12), 0, 0)
        }
    }

    private fun note(text: String): TextView {
        return TextView(this).apply {
            this.text = text
            setTextAppearance(com.google.android.material.R.style.TextAppearance_Material3_BodySmall)
            setPadding(0, dp(2), 0, dp(4))
        }
    }

    private fun applySafeArea(toolbar: View, vararg pages: View) {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val bars = WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
        ViewCompat.setOnApplyWindowInsetsListener(toolbar) { view, insets ->
            val safe = insets.getInsets(bars)
            view.setPadding(safe.left, safe.top, safe.right, 0)
            insets
        }
        for (page in pages) {
            ViewCompat.setOnApplyWindowInsetsListener(page) { view, insets ->
                val safe = insets.getInsets(bars)
                val ime = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
                view.setPadding(safe.left, 0, safe.right, max(safe.bottom, ime))
                insets
            }
        }
        ViewCompat.requestApplyInsets(toolbar)
    }

    private fun scroll(vararg views: View): ScrollView {
        return ScrollView(this).apply {
            isFillViewport = true
            addView(column(*views))
        }
    }

    private fun column(vararg views: View, pad: Boolean = true): LinearLayout {
        val inset = if (pad) dp(16) else 0
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(inset, inset, inset, inset)
            for (v in views) addView(v)
        }
    }

    private fun row(vararg views: View): LinearLayout {
        return LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            for (v in views) addView(v, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        }
    }

    private fun select(block: (Int) -> Unit) = object : AdapterView.OnItemSelectedListener {
        override fun onItemSelected(parent: AdapterView<*>?, view: View?, position: Int, id: Long) = block(position)
        override fun onNothingSelected(parent: AdapterView<*>?) {}
    }

    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()

    private enum class Page { Main, Audio, Radio, Tnc }
}

private val radioChoices = listOf(
    RADIO_NONE to "(None)",
    RADIO_KENWOOD to "Kenwood TS-480",
    RADIO_G90 to "Xiegu G90 (CI-V)",
    RADIO_ICOM to "Icom CI-V",
    RADIO_YAESU to "Yaesu FT-891/991",
    RADIO_TRUSDX to "(tr)uSDX",
)

private fun baudIndexFor(radio: Int): Int = when (radio) {
    RADIO_G90, RADIO_ICOM -> 2
    RADIO_YAESU -> 3
    RADIO_TRUSDX -> 5
    else -> 1
}

private fun fallbackBaud(radio: Int): Int = when (radio) {
    RADIO_G90, RADIO_ICOM -> 19200
    RADIO_YAESU -> 38400
    RADIO_TRUSDX -> 115200
    else -> 9600
}

private const val USB_ACTION = "org.x0rro.xara.USB"
private const val MENU_AUDIO = 1
private const val MENU_RADIO = 2
private const val MENU_TNC = 3
