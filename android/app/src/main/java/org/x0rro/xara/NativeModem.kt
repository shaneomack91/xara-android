package org.x0rro.xara

import java.nio.ByteBuffer

object NativeModem {
    val available: Boolean

    init {
        var ok = false
        try {
            System.loadLibrary("xara_android")
            ok = ping() == 1
        } catch (_: UnsatisfiedLinkError) {
            ok = false
        }
        available = ok
    }

    const val OP_OPEN = 0
    const val OP_PTT_ON = 1
    const val OP_PTT_OFF = 2
    const val OP_SET_FREQ = 3
    const val OP_GET_FREQ = 4
    const val OP_SET_MODE = 5
    const val OP_GET_MODE = 6

    external fun ping(): Int
    external fun registered(): Boolean
    external fun block(): Int
    external fun start(call: String, mode: Int, addr: String, cmdPort: Int, kissPort: Int, leadMs: Int): String
    external fun status(): String
    external fun process(rx: ShortArray, tx: ShortArray, forceOff: Boolean): Int
    external fun stop()
    external fun lastKeyed(): Boolean
    external fun setTail(ms: Int)
    external fun setManual(on: Boolean)
    external fun catOpen(radio: Int, addr: Int): Int
    external fun radioBaud(radio: Int): Int
    external fun catBuild(op: Int, hz: Long, mode: Int, dst: ByteBuffer): Int
    external fun catFeed(src: ByteBuffer, len: Int, dst: ByteBuffer): Int
    external fun trusdxEnable(speaker: Boolean, dst: ByteBuffer): Int
    external fun endTx(dst: ByteBuffer): Int
    external fun cycleTrusdx(
        src: ByteBuffer,
        len: Int,
        forceOff: Boolean,
        dst: ByteBuffer,
        cat: ByteBuffer,
    ): Long
}

val BUILD_HELP = """
The native modem library is not in this install.

On a machine with the Android SDK and NDK (r27 or newer):

    rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
    cargo install cargo-ndk
    RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384" cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o android/app/src/main/jniLibs build --release -p xara-droid

Then rebuild the app (Android Studio, or gradle assembleDebug in android/).
The .so must be libxara_android.so under jniLibs/<abi>/.
""".trimIndent()
