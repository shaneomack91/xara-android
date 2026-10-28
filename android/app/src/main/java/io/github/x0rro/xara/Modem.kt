package io.github.x0rro.xara

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The one embedded modem, and the app's view of it. */
object Modem {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var handle = 0L

    val running = MutableStateFlow(false)
    val status = MutableStateFlow("Stopped")
    private val _log = MutableStateFlow<List<String>>(emptyList())
    val log: StateFlow<List<String>> = _log
    private val _received = MutableStateFlow("")

    /** What the modem received, as readable text, appended as it arrives. */
    val received: StateFlow<String> = _received

    init {
        scope.launch {
            while (isActive) {
                drainLog()
                if (handle != 0L && !Native.running(handle)) finish()
                delay(250)
            }
        }
    }

    /** Starts the service that holds the modem; it calls [startNative]. */
    fun start(context: Context, config: ModemConfig) {
        if (handle != 0L) return
        val i = Intent(context, ModemService::class.java).putExtra(ModemService.ARGS, config.arguments())
            .putExtra(ModemService.TRUSDX, config.hookup == Hookup.TRUSDX)
            .putExtra(ModemService.HEAR, config.phoneListen)
        config.stateDir.mkdirs()
        ContextCompat.startForegroundService(context, i)
        status.value = "Starting…"
    }

    fun stop(context: Context) {
        scope.launch { finish() }
        context.stopService(Intent(context, ModemService::class.java))
    }

    internal fun startNative(args: Array<String>): Boolean {
        if (handle != 0L) return true
        handle = Native.start(args)
        if (handle == 0L) {
            status.value = "The modem is already running."
            return false
        }
        running.value = true
        status.value = "Running"
        return true
    }

    private suspend fun finish() {
        val h = handle.takeIf { it != 0L } ?: return
        handle = 0L
        status.value = "Stopping…"
        val code = withContext(Dispatchers.IO) { Native.stop(h).also { TrusdxPort.close() } }
        drainLog()
        running.value = false
        val why = _log.value.lastOrNull { it.startsWith("xara: ") }?.removePrefix("xara: ") ?: "see Log"
        status.value = if (code == 0) "Stopped" else "Stopped (exit $code): $why"
        ModemService.instance?.stopSelf()
    }

    private fun drainLog() {
        val got = generateSequence { Native.nextLog() }.toList()
        if (got.isNotEmpty()) _log.value = (_log.value + got).takeLast(2000)
        val rx = generateSequence { Native.nextReceived() }.joinToString("")
        if (rx.isNotEmpty()) _received.value = (_received.value + rx).takeLast(200_000)
    }

    fun clearLog() {
        _log.value = emptyList()
    }

    fun clearReceived() {
        _received.value = ""
    }
}

/**
 * A foreground service, so Android keeps the modem and its microphone access alive while
 * the app is in the background.
 */
class ModemService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
    }

    override fun onDestroy() {
        PhoneListen.stop()
        TrusdxPort.close()
        instance = null
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val trusdx = intent?.getBooleanExtra(TRUSDX, false) == true
        // A (tr)uSDX sends its audio over USB serial; nothing is recorded from a microphone.
        val type = if (trusdx) ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE else ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        startForeground(1, notification(), type)
        val args = intent?.getStringArrayExtra(ARGS)
        val serial = if (trusdx) TrusdxPort.open(this) else null
        if (serial != null) {
            Modem.status.value = serial
            stopSelf()
        } else if (args == null || !Modem.startNative(args)) {
            TrusdxPort.close()
            stopSelf()
        } else if (intent?.getBooleanExtra(HEAR, false) == true) {
            PhoneListen.start()
        }
        return START_NOT_STICKY
    }

    private fun notification(): Notification {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, "Modem", NotificationManager.IMPORTANCE_LOW))
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL)
            .setContentTitle("xara is running")
            .setContentText("The modem is listening and may transmit.")
            .setSmallIcon(android.R.drawable.stat_sys_speakerphone)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
    }

    companion object {
        const val ARGS = "args"
        const val TRUSDX = "trusdx"
        const val HEAR = "hear"
        private const val CHANNEL = "modem"
        var instance: ModemService? = null
    }
}
