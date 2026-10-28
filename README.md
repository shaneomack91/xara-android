# xara

a VARA-compatible modem. the linux program is tested against VARA HF 4.9.0 and FM 4.4.0. this tree also builds an Android app around the same modem.

## minimum config

set your call sign, HF bandwidth or FM mode, audio, and (if using a radio)
radio and ptt. do this with command line flags or a config file.

    xara --call YOURCALL --hf 2300 --audio alsa \
         --capture plughw:CARD=Device,DEV=0 \
         --playback plughw:CARD=Device,DEV=0

to transmit, add `--radio` and `--ptt` flags.

## other features

 - web UI for configuration and management
 - all the usual TCP ports (works with your favorite software)
 - digipeating (enable by call with `--digi N1CALL`)
 - `xara-monitor`: passive VARA HF monitor (with web UI)

## building

install deps:

    apt install libpulse-dev libasound2-dev

then install from here:

    cargo install --locked --git https://github.com/x0rro/xara xara

alternatively for a local setup you can do:

    cargo build --release
    cargo test

## android

the phone app is in `android/`. the main screen is the session: callsign,
VARA-compatible mode, and Start, Stop, and Key. Audio, Radio, and TNC settings
are in the menu.

build it with the Android SDK, NDK r27 or newer, and JDK 17 or 21. Gradle 8.9
does not run on JDK 25. `ANDROID_HOME` must point at the SDK. `cargo-ndk` finds
the NDK there. the native library is built into `android/app/src/main/jniLibs`
and is not part of the git tree.

    rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
    cargo install cargo-ndk
    RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384" cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o android/app/src/main/jniLibs build --release -p xara-droid

then, from `android/`, `./gradlew assembleDebug` (or open that directory in
Android Studio). the debug APK is
`android/app/build/outputs/apk/debug/app-debug.apk`.

higher VARA speeds stay off unless you have a licence and build with the
`registered` feature. put this in `android/local.properties`, which is not
committed:

    xara.registered=true

and add `--features registered` to the `cargo ndk` command above, or run
`./gradlew cargoNdk` before the APK. without that, the modem uses the
unregistered speed limits.

 - **Audio.** microphone and speaker. system default is the phone mic and speaker.
 - **Radio.** radio type, PTT, USB serial, baud, and CAT frequency. the default
   radio is (None) and the default PTT is VOX, so a fresh install uses the mic
   and speaker and does not send CAT or assume a PTT line. USB serial is an OTG
   CAT or PTT cable (FTDI, CH340, CP210x, Prolific, CDC).
 - **TNC.** listen address, command port, KISS port, and PTT timing.

PTT choices are VOX, RTS, DTR, CAT, and (tr)uSDX audio-over-CAT. RTS and DTR
key a serial control line. CAT sends a key command. (tr)uSDX audio sends sound
on the CAT port instead of the phone speaker.

radios: (None), Kenwood TS-480 (`FA`/`FB`/`MD`/`IF`, `TX`/`RX`), Xiegu G90
(CI-V, 19200, address `70`), Icom CI-V, Yaesu FT-891/991, and (tr)uSDX
(Kenwood commands plus `UA`/`US` audio, 115200).

a client connects over TCP. the command port defaults to 8300. the data port is
always the next one (8301). on the phone use host `127.0.0.1`; from another
device use the phone's address. HF modes are the VARA-compatible HF session.
FM narrow and FM wide are the VARA-compatible FM session. the KISS port
defaults to 8001 (0 turns it off). that path carries AX.25 inside the
VARA-compatible modem, not 1200-baud packet. Winlink for Android (WoAD) is one
client that can use these ports.

## options

    --config FILE           read key = value options from FILE

    --call CALL             your callsign (required)

    --audio pulse|alsa      pulse vs alsa audio path
    --capture SOURCE        audio source
    --playback SINK         audio sink

    --hf 500|2300|2750      HF mode, bandwidth setting
    --fm narrow|wide        FM mode, narrow vs wide

    --radio                 enable radio mode
    --drive-db D            TX drive (<= 0 dB, default -5)
    --ptt [see section]     PTT has a lot of options

    --ui-port N             serve web UI on port N (default off)
    --ui-addr A             webui listen address (default 127.0.0.1)
    --ui-host NAME          accept as HTTP Host for UI (repeatable)
    --ui-token T            UI token (default: random)
    --ui-control            DANGER: remote token-holders can key radio
    --state-dir DIR         web UI settings storage (default: set by systemd)

    --addr A                KISS/command address (default: 127.0.0.1)
    --port N                command port (< 65535, default: 8300)
    --kiss-port N|off       KISS TCP port (default: 8100)
    --persistence P         KISS p-persistence

    --registered            enable registered speeds (must set build flag)

    --digi CALL             FM digipeat for CALL (default: off)
    --record PATH           record rx, tx and PTT to FLAC (or WAV if .wav)
    --quiet                 no output
    --version and --help    guess

## PTT settings

we support the following PTT methods:

 - rts and dtr on a serial port
 - cm108/cm119 HID GPIO
 - hamlib rigctld CAT
 - vox (not recommended)

you can adjust the following PTT settings as well:

 - PTT lead time
 - PTT tail time
 - max PTT

summary of settings:

    --ptt vox                   use VOX (just don't bother)
    --ptt rts:DEV               key serial port on rts, hold dtr low
    --ptt dtr:DEV               key serial port on dtr, hold rts low
    --ptt cm108:DEV[:GPIO]      cm108/cm119 HID GPIO (default GPIO: 3)
    --ptt rigctld[:HOST:PORT]   hamlib rigctld CAT on HOST:PORT

    --ptt-lead-ms N             PTT held for N ms before xmit (default: 0)
    --ptt-tail-ms N             PTT held for N ms after xmit (default: 100)
    --ptt-max-s N               release PTT after N sec max (1-120, default: 30)

## xara-monitor

`xara-monitor` listens to VARA HF traffic between other stations and never
transmits. it is considerably more demanding than `xara` and probably won't run
on a pi.

    xara-monitor --audio alsa --capture plughw:CARD=Device,DEV=0

options:

    --wav FILE              load from a wav file
    --stdin RATE            s16le mono at RATE Hz

    --audio pulse|alsa      pulse or alsa audio path
    --capture SOURCE        audio source

    --hf 500|2300|2750      HF bandwidth (default 2300)

    --call CALL             known call (repeatable)
    --calls FILE            list of known calls, one per line
    --no-blind              only attempt to resolve known calls

    --ui-port N             read-only web UI on port N (default: off)
    --ui-addr ADDR          web UI listen addr (default: 127.0.0.1)
    --ui-host NAME          accept as HTTP Host for UI (repeatable)

    --key-idle MIN          max target idle time (default: 30, 0 disables)
    --dump                  dump entire payloads
    --verbose               report unconfirmed blind detections
    --json                  print newline-delimited JSON records
    --version and --help    guess

## registered mode

compile with the `registered` feature for higher speeds if you have a
registered copy of VARA, then run with `xara --registered ...`. the Android
build uses the same feature; see the android section. it is off unless you
turn it on before compiling.

## copyright

copyright (c) 2026 x0rro
released under MIT license, see LICENSE
