package io.github.x0rro.xara

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import androidx.core.content.ContextCompat
import com.hoho.android.usbserial.driver.UsbSerialDriver
import com.hoho.android.usbserial.driver.UsbSerialPort
import com.hoho.android.usbserial.driver.UsbSerialProber
import com.hoho.android.usbserial.util.SerialInputOutputManager
import java.util.concurrent.Executors

/**
 * A (tr)uSDX's USB serial port, carrying both its CAT and its audio (`--audio trusdx --capture
 * app`). Android gives apps no serial devices, so the bytes go through its USB host API here and
 * into the modem through [Native.serialIn] and [Native.serialOut]. After VA3SHC's xara-android
 * (https://github.com/x0rro/xara/pull/12).
 */
object TrusdxPort {
    private const val BAUD = 115_200
    private const val PERMISSION = "io.github.x0rro.xara.USB_PERMISSION"

    private var port: UsbSerialPort? = null
    private var io: SerialInputOutputManager? = null
    private var writer: Thread? = null
    @Volatile private var open = false

    /** USB serial adapters plugged in, the (tr)uSDX's CH340 first. */
    fun drivers(context: Context): List<UsbSerialDriver> {
        val usb = context.getSystemService(UsbManager::class.java)
        return UsbSerialProber.getDefaultProber().findAllDrivers(usb).sortedBy { if (it.device.vendorId == CH340_VENDOR) 0 else 1 }
    }

    fun label(d: UsbSerialDriver): String =
        "${d.device.productName ?: d.javaClass.simpleName.removeSuffix("SerialDriver")} " +
            "(%04x:%04x)".format(d.device.vendorId, d.device.productId)

    /** Asks for access to [device] if xara does not have it; [done] gets whether it was given. */
    fun requestPermission(context: Context, device: UsbDevice, done: (Boolean) -> Unit) {
        val usb = context.getSystemService(UsbManager::class.java)
        if (usb.hasPermission(device)) return done(true)
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context, i: Intent) {
                context.unregisterReceiver(this)
                done(i.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false))
            }
        }
        ContextCompat.registerReceiver(context, receiver, IntentFilter(PERMISSION), ContextCompat.RECEIVER_NOT_EXPORTED)
        val intent = Intent(PERMISSION).setPackage(context.packageName)
        usb.requestPermission(device, PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_MUTABLE))
    }

    /** Opens the first adapter at 115200 8N1 and starts carrying bytes; returns why not, or null. */
    @Synchronized
    fun open(context: Context): String? {
        close()
        val driver = drivers(context).firstOrNull() ?: return "No USB serial adapter found. Plug in the (tr)uSDX."
        val usb = context.getSystemService(UsbManager::class.java)
        val conn = usb.openDevice(driver.device) ?: return "No access to ${label(driver)}."
        val p = driver.ports.first()
        try {
            p.open(conn)
            p.setParameters(BAUD, 8, UsbSerialPort.STOPBITS_1, UsbSerialPort.PARITY_NONE)
            // As the (tr)uSDX's own host software does: DTR on, RTS off.
            p.dtr = true
            p.rts = false
        } catch (e: Exception) {
            runCatching { p.close() }
            return "${label(driver)}: ${e.message}"
        }
        port = p
        open = true
        io = SerialInputOutputManager(p, object : SerialInputOutputManager.Listener {
            override fun onNewData(data: ByteArray) = Native.serialIn(data, data.size)

            override fun onRunError(e: Exception) {
                if (open) Native.serialFailed(e.message ?: "read failed")
            }
        }).also { Executors.newSingleThreadExecutor().submit(it) }
        writer = Thread({
            // Small writes, so an unkey is not stuck behind a buffer the radio's UART will drop.
            val buf = ByteArray(256)
            while (open) {
                val n = Native.serialOut(buf, 100)
                if (n <= 0) continue
                try {
                    p.write(buf.copyOf(n), 2000)
                } catch (e: Exception) {
                    if (open) Native.serialFailed(e.message ?: "write failed")
                    break
                }
            }
        }, "trusdx-out").apply { start() }
        return null
    }

    /** Stops carrying bytes, after the modem has sent its last (unkey and stop streaming). */
    @Synchronized
    fun close() {
        if (!open) return
        // The writer passes on what the modem sent as it stopped within its 100 ms poll.
        Thread.sleep(250)
        open = false
        writer?.join(500)
        io?.stop()
        runCatching { port?.close() }
        port = null; io = null; writer = null
    }

    const val CH340_VENDOR = 0x1A86
}
