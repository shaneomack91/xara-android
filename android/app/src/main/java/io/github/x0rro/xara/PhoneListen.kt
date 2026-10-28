package io.github.x0rro.xara

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack

/** Plays the modem's receive and transmit audio through this phone's speaker. */
object PhoneListen {
    private const val RATE = 48_000

    @Volatile private var on = false
    private var thread: Thread? = null

    @Synchronized
    fun start() {
        if (on) return
        val min = AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(RATE)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            .setBufferSizeInBytes(min.coerceAtLeast(RATE / 5 * 2))
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()
        track.play()
        on = true
        thread = Thread({
            val buf = ByteArray(960)
            while (on) {
                val n = Native.listenOut(buf)
                if (n > 0) track.write(buf, 0, n, AudioTrack.WRITE_BLOCKING) else Thread.sleep(5)
            }
            runCatching { track.stop() }
            track.release()
        }, "xara-listen").apply { start() }
    }

    @Synchronized
    fun stop() {
        if (!on) return
        on = false
        thread?.join(500)
        thread = null
    }
}
