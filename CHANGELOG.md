# Changelog

## unreleased

android app around the existing modem

- phone UI for callsign, VARA-compatible mode, audio devices, and a TCP TNC
- USB serial PTT (RTS, DTR), CAT, and (tr)uSDX audio-over-CAT
- radios: Kenwood TS-480, Xiegu G90 (CI-V), Icom CI-V, Yaesu FT-891/991, (tr)uSDX
- default is VOX with no radio, so the app runs on the mic and speaker
- higher VARA speeds stay off unless the build enables the `registered` feature
- `cargo build` stays the Linux program; the Android JNI crate is built with `-p xara-droid`

## 0.1.1

recording feature and bugfixes

- `--record` flag captures rx, tx, and PTT in FLAC
- bugfix: long squelch tails no longer trigger false receive
- bugfix: phantom frames no longer send inappropriate compression

## 0.1.0

first release

- modem is fully compatible with VARA HF 4.9.0 and VARA FM 4.4.0
- web UI for configuration and management
- all the usual TCP ports (works with your favorite software)
- pipewire, pulse, and alsa support
- `xara-monitor`: passive VARA HF monitor (with web UI)
