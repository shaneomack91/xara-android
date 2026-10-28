# xara

a VARA-compatible modem for linux and macOS, with apps for iPhone/iPad and
Android.
tested against VARA HF 4.9.0 and FM 4.4.0.

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
 - iOS app: the same modem on an iPhone or iPad, see [iOS](#ios)
 - Android app: the same modem on an Android phone or tablet, see [Android](#android)

## building

install deps:

    apt install libpulse-dev libasound2-dev

then install from here:

    cargo install --locked --git https://github.com/x0rro/xara xara

alternatively for a local setup you can do:

    cargo build --release
    cargo test

### macOS

no extra deps: audio goes through CoreAudio (`--audio coreaudio`, the default
on macOS). install rust, then the same `cargo install` line as above.

devices are named by CoreAudio UID or by their exact name; list them with:

    xara --list-devices

    xara --call YOURCALL --hf 2300 \
         --capture "USB Audio CODEC" --playback "USB Audio CODEC" \
         --radio --ptt rts:/dev/cu.usbserial-0001

without `--radio`, playback is only allowed into a virtual device such as
[BlackHole](https://github.com/ExistentialAudio/BlackHole), which is also the
easy way to loop two xara instances (or xara and VARA in a VM) together.

the first run asks for microphone access for your terminal. serial ports are
the `/dev/cu.*` devices. CM108/CM119 PTT is addressed by USB vendor:product
id instead of a hidraw path (`ioreg -p IOUSB -l | grep -E 'idVendor|idProduct'`
shows them): `--ptt cm108:0d8c:0012`, or `cm108:1209:7388` for an AIOC.

for a window instead of the command line, `macos/` has a Mac app with the
same modem, settings, web UI and a menu bar icon; see
[macos/README.md](macos/README.md).

to start xara at login, edit and load the launchd agent in `dist/launchd`:

    cp dist/launchd/xara.plist ~/Library/LaunchAgents/
    launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/xara.plist

### iOS

`ios/` is an iPhone and iPad app that runs this same modem: not a rewrite,
the Rust code linked in through `crates/xara-ffi`. it has the modem settings,
xara's web UI and the log, and opens the usual TCP ports, so Winlink Express,
Pat or another VARA client on the phone's network can use it as their modem.

plug in a USB audio interface such as a Digirig. iOS gives apps no serial or
HID access, so PTT is VOX or hamlib `rigctld` over the network.

to build it, add the rust iOS targets and open the Xcode project; the build
compiles the Rust modem first:

    rustup target add aarch64-apple-ios aarch64-apple-ios-sim

see [ios/README.md](ios/README.md) for the details.

### Android

`android/` is an Android app that runs this same modem, loaded as a native
library through `crates/xara-ffi`. like the iOS app it has the modem settings,
xara's web UI and the log, and opens the usual TCP ports for VARA clients on
the phone's network. it runs as a foreground service, so it keeps going in the
background.

plug in a USB audio interface such as a Digirig and choose it as the audio in
and out. PTT is VOX or hamlib `rigctld` over the network. or plug in a
(tr)uSDX and choose it as the connection: its audio and PTT go over its USB
serial port, so nothing else is needed (see [(tr)uSDX](#trusdx)).

to build it you need the Android SDK with NDK 28.2 and the rust Android
targets; gradle compiles the Rust modem first:

    rustup target add aarch64-linux-android x86_64-linux-android
    cd android && ./gradlew assembleDebug

see [android/README.md](android/README.md) for the details.

## options

    --config FILE           read key = value options from FILE

    --call CALL             your callsign (required)

    --audio pulse|alsa|coreaudio|trusdx  audio path (coreaudio on macOS)
    --list-devices          list capture and playback devices, then exit
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

    --registered            registered speeds, for a user with a VARA licence

    --digi CALL             FM digipeat for CALL (default: off)
    --record PATH           record rx, tx and PTT to FLAC (or WAV if .wav)
    --audio-latency-ms N    allow for N ms of delay in the audio path that xara
                            does not add, round trip (two phones on their own
                            speakers and mics: ~500)
    --debug-log PATH        append a timestamped log to PATH: every line xara
                            logs, frames heard and sent, and once a second the
                            rx and tx audio levels
    --monitor               print what's received, other stations' HF traffic too
    --monitor-call CALL     name CALL when heard (repeatable; else #hash)
    --monitor-frames        show every frame heard, not just links and text
    --quiet                 no output
    --version and --help    guess

## PTT settings

we support the following PTT methods:

 - rts and dtr on a serial port
 - cm108/cm119 HID GPIO (hidraw on linux, USB id on macOS)
 - hamlib rigctld CAT
 - (tr)uSDX CAT, in the serial stream that carries its audio
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
    --ptt cm108:VID:PID[:GPIO]  same, by hex USB id (macOS)
    --ptt rigctld[:HOST:PORT]   hamlib rigctld CAT on HOST:PORT
    --ptt trusdx                (tr)uSDX CAT, with --audio trusdx
    --trusdx-speaker            (tr)uSDX speaker on while streaming (default off)

    --ptt-lead-ms N             PTT held for N ms before xmit (default: 0)
    --ptt-tail-ms N             PTT held for N ms after xmit (default: 100)
    --ptt-max-s N               release PTT after N sec max (1-120, default: 30)

## (tr)uSDX

the (tr)uSDX can send and receive its audio as data on its USB serial (CAT)
port (DL2MAN / PE1NNZ firmware), so it needs no sound card: received audio
streams at 7825 Hz after `UA2;`, and transmit audio goes at 11520 Hz between
`TX0;` and `RX;`. `--audio trusdx` uses that, with the serial port as both
the capture and playback device, and `--ptt trusdx` keys the radio in the
same stream:

    xara --call YOURCALL --hf 2300 --audio trusdx \
         --capture /dev/ttyUSB0 --playback /dev/ttyUSB0 --radio --ptt trusdx

the port runs at 115200 8N1 with DTR on and RTS off; on macOS it is the
`/dev/cu.wchusbserial*` device. the radio's speaker is off while xara
streams unless `--trusdx-speaker` is set (`UA1;` instead of `UA2;`). the
Android app does the same over Android's USB host API, and can also play the
modem's receive and transmit audio on the phone. ported
from Shane Curless VA3SHC's Android work in
[x0rro/xara#12](https://github.com/x0rro/xara/pull/12).

## xara-monitor

`xara-monitor` listens to VARA HF traffic between other stations and never
transmits. it is considerably more demanding than `xara` and probably won't run
on a pi.

    xara-monitor --audio alsa --capture plughw:CARD=Device,DEV=0

options:

    --wav FILE              load from a wav file
    --stdin RATE            s16le mono at RATE Hz

    --audio pulse|alsa|coreaudio  audio path (coreaudio on macOS)
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

if you have a registered copy of VARA, run with `xara --registered ...` (or
`registered` in the config file, or turn on *Registered (VARA licence)* in the
web UI's settings) so links with other registered stations run at full speed.
it is off unless you turn it on. the `registered` feature is built in by
default; `--no-default-features --features pulse,alsa` leaves it out.

## web UI settings

with `--ui-control`, the web UI's settings card saves to the state directory
and restarts xara, as the apps' settings do: call, bandwidth, audio devices,
PTT timing, audio delay, *Registered (VARA licence)*, *Show other stations'
traffic* with *Every packet* and calls to name (HF), *Record received audio*
and *Keep a debug log*. the last two write `xara-DATE-TIME.flac` and
`debug.log` next to the settings file (`/var/lib/xara/STATION/` for the
system service). each switch can be left to the command line, or turned on
or off over it.

## copyright

copyright (c) 2026 x0rro
released under MIT license, see LICENSE
