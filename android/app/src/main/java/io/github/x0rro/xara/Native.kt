package io.github.x0rro.xara

/** The Rust modem (crates/xara-ffi), which runs exactly as the `xara` command does. */
object Native {
    init {
        System.loadLibrary("xara_ffi")
    }

    external fun version(): String

    /** Starts xara with command-line options; returns a handle, or 0 if one is already running. */
    external fun start(args: Array<String>): Long

    /** False once the modem has stopped by itself (bad options, audio failure). */
    external fun running(handle: Long): Boolean

    /** Stops the modem, releasing PTT first, and frees the handle. Returns the exit status. */
    external fun stop(handle: Long): Int

    /** The next line xara logged, or null. */
    external fun nextLog(): String?

    /** The next piece of what the modem received, as readable text, or null. */
    external fun nextReceived(): String?

    /** Bytes read from a (tr)uSDX's serial port: the first [n] of [data]. */
    external fun serialIn(data: ByteArray, n: Int)

    /** Fills [buf] with bytes to write to the radio, waiting up to [waitMs] for some; returns how many. */
    external fun serialOut(buf: ByteArray, waitMs: Int): Int

    /** The serial port is gone; the modem stops, saying [why]. */
    external fun serialFailed(why: String)

    /**
     * Little-endian PCM16 at 48 kHz, receive and transmit mixed.
     * Returns how many bytes were written into [buf].
     */
    external fun listenOut(buf: ByteArray): Int
}
