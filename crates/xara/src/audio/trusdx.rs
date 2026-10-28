//! (tr)uSDX audio over its CAT serial port (see `crate::trusdx`). The capture and playback
//! device is the serial port: a device path, or `app` when the program embedding xara carries
//! the bytes itself, as the Android app does (Android lets apps reach USB serial adapters only
//! through its USB host API).

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{Audio, DeviceList, RATE};
use crate::trusdx::{self, BAUD, Codec};

// The app log does not show up in logcat, so key and unkey are written there directly.
#[cfg(target_os = "android")]
#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: libc::c_int, tag: *const libc::c_char, text: *const libc::c_char) -> libc::c_int;
}

fn note(msg: &str) {
    say!("truSDX: {msg}");
    #[cfg(target_os = "android")]
    {
        let Ok(text) = std::ffi::CString::new(msg) else { return };
        unsafe {
            __android_log_write(4, c"xara".as_ptr(), text.as_ptr());
        }
    }
}

/// The device name for a port the embedding app carries.
pub const APP: &str = "app";
/// Received audio older than this is dropped, so a radio clock running fast cannot add delay.
const MAX_QUEUED: usize = RATE as usize / 4;
const QUEUED_AFTER_DROP: usize = RATE as usize / 20;
/// How late radio audio may be before the block is filled with silence.
const JITTER: Duration = Duration::from_millis(60);
/// The radio counts as streaming while its audio is this recent.
const STREAMING: Duration = Duration::from_millis(300);
/// Without received audio for this long, ask the radio to stream again (it may have been off).
const RESTART: Duration = Duration::from_secs(2);

/// Set by the `trusdx` PTT, read by the audio: the radio is keyed in its audio stream.
static KEYED: AtomicBool = AtomicBool::new(false);
/// `E;` replies from the radio: it parsed transmit audio as CAT, so it was not in the audio stream.
static CAT_ERRORS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// `UA1;` instead of `UA2;`: the radio's own speaker plays the stream.
static SPEAKER: AtomicBool = AtomicBool::new(false);

pub fn set_keyed(on: bool) {
    KEYED.store(on, Ordering::SeqCst);
}

pub fn set_speaker(on: bool) {
    SPEAKER.store(on, Ordering::SeqCst);
}

#[derive(Default)]
struct Rx {
    codec: Codec,
    pcm: VecDeque<i16>,
    heard: Option<Instant>,
    failed: Option<String>,
}

#[derive(Default)]
struct Shared {
    rx: Mutex<Rx>,
    ready: Condvar,
    closed: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Rx> {
        self.rx.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn receive(&self, bytes: &[u8]) {
        let mut rx = self.lock();
        let Rx { codec, pcm, heard, .. } = &mut *rx;
        let before = pcm.len();
        for cat in codec.receive(bytes, pcm) {
            if cat == "E;" {
                CAT_ERRORS.fetch_add(1, Ordering::Relaxed);
            } else if cat.starts_with("RX") || cat.starts_with("TX") || cat.starts_with("UA") {
                note(&format!("radio {cat}"));
            }
        }
        if pcm.len() > before {
            *heard = Some(Instant::now());
        }
        if pcm.len() > MAX_QUEUED {
            let n = pcm.len() - QUEUED_AFTER_DROP;
            pcm.drain(..n);
        }
        self.ready.notify_all();
    }

    fn fail(&self, why: String) {
        self.lock().failed.get_or_insert(why);
        self.ready.notify_all();
    }
}

/// The port as the embedding app sees it: bytes it read go to [`app_received`], and it writes
/// what [`app_take`] returns.
struct AppPort {
    to_radio: Mutex<VecDeque<u8>>,
    wanted: Condvar,
    /// Signalled when bytes are taken off `to_radio`, so a full queue can accept more.
    space: Condvar,
    modem: Mutex<Option<Arc<Shared>>>,
}

static APP_PORT: AppPort = AppPort {
    to_radio: Mutex::new(VecDeque::new()),
    wanted: Condvar::new(),
    space: Condvar::new(),
    modem: Mutex::new(None),
};

/// How much transmit audio may wait in the app queue. More than this and the write waits,
/// instead of bursting faster than 115200 baud and overflowing the radio (which drops `;RX;`).
const TX_QUEUE_MAX: usize = 2048;

/// Bytes the app read from the radio's serial port.
pub fn app_received(bytes: &[u8]) {
    if let Some(s) = APP_PORT.modem.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        s.receive(bytes);
    }
}

/// The app lost the serial port; the modem stops with `why`.
pub fn app_failed(why: &str) {
    if let Some(s) = APP_PORT.modem.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        s.fail(format!("truSDX serial port: {why}"));
    }
}

/// Up to `max` bytes for the app to write to the radio, waiting up to `wait` for some.
pub fn app_take(max: usize, wait: Duration) -> Vec<u8> {
    let q = APP_PORT.to_radio.lock().unwrap_or_else(PoisonError::into_inner);
    let (mut q, _) =
        APP_PORT.wanted.wait_timeout_while(q, wait, |q| q.is_empty()).unwrap_or_else(PoisonError::into_inner);
    let n = q.len().min(max);
    let out: Vec<u8> = q.drain(..n).collect();
    drop(q);
    if n > 0 {
        APP_PORT.space.notify_all();
    }
    out
}

enum Port {
    Tty(File),
    App,
}

impl Port {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Tty(f) => f.write_all(bytes),
            Self::App => {
                if bytes.is_empty() {
                    return Ok(());
                }
                let mut q = APP_PORT.to_radio.lock().unwrap_or_else(PoisonError::into_inner);
                let started = Instant::now();
                while q.len().saturating_add(bytes.len()) > TX_QUEUE_MAX {
                    let left = Duration::from_millis(500).saturating_sub(started.elapsed());
                    if left.is_zero() {
                        return Err(io::Error::other("truSDX serial port: transmit queue is not draining"));
                    }
                    let (guard, waited) = APP_PORT.space.wait_timeout(q, left).unwrap_or_else(PoisonError::into_inner);
                    q = guard;
                    if waited.timed_out() && q.len().saturating_add(bytes.len()) > TX_QUEUE_MAX {
                        return Err(io::Error::other("truSDX serial port: transmit queue is not draining"));
                    }
                }
                q.extend(bytes);
                APP_PORT.wanted.notify_all();
                Ok(())
            }
        }
    }
}

fn termios(f: &File, path: &str) -> io::Result<()> {
    let fd = f.as_raw_fd();
    let err = |what: &str| io::Error::other(format!("{path}: {what}: {}", io::Error::last_os_error()));
    // SAFETY: fd is open, and t is a termios the calls fill in.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return Err(err("not a serial port"));
        }
        libc::cfmakeraw(&mut t);
        t.c_cflag |= libc::CLOCAL | libc::CREAD;
        // Reads return within 100 ms, so the reader notices the port closing.
        t.c_cc[libc::VMIN] = 0;
        t.c_cc[libc::VTIME] = 1;
        if libc::cfsetspeed(&mut t, BAUD as libc::speed_t) != 0 || libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
            return Err(err("setting 115200 8N1"));
        }
        // As the (tr)uSDX's own host software does: DTR on, RTS off.
        let (dtr, rts) = (libc::TIOCM_DTR, libc::TIOCM_RTS);
        libc::ioctl(fd, libc::TIOCMBIS as _, &dtr);
        libc::ioctl(fd, libc::TIOCMBIC as _, &rts);
    }
    Ok(())
}

fn port_name(capture: &str, playback: &str) -> io::Result<String> {
    if capture.is_empty() || (!playback.is_empty() && playback != capture) {
        return Err(io::Error::other(format!(
            "--audio trusdx: --capture and --playback are both the radio's serial port (got {capture:?} and {playback:?})"
        )));
    }
    if capture != APP && !capture.starts_with('/') {
        return Err(io::Error::other(format!("--audio trusdx: {capture:?} is not a serial device path or {APP}")));
    }
    Ok(capture.to_string())
}

pub fn check(playback: &str, _radio: bool) -> io::Result<()> {
    port_name(playback, playback).map(drop)
}

/// Serial ports that could be a (tr)uSDX: its CH340 USB-serial chip, or any USB serial port.
pub fn list() -> io::Result<(DeviceList, DeviceList)> {
    let prefixes = ["ttyUSB", "ttyACM", "cu.wchusbserial", "cu.usbserial", "cu.usbmodem"];
    let mut ports: DeviceList = std::fs::read_dir("/dev")
        .map(|d| {
            d.filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| prefixes.iter().any(|p| n.starts_with(p)))
                .map(|n| (format!("/dev/{n}"), "serial port".to_string()))
                .collect()
        })
        .unwrap_or_default();
    ports.sort();
    ports.push((APP.into(), "a port the app embedding xara carries".into()));
    Ok((ports.clone(), ports))
}

pub struct TrusdxIo {
    shared: Arc<Shared>,
    port: Port,
    tx: Codec,
    radio: bool,
    reader: Option<JoinHandle<()>>,
    due: Option<Instant>,
    asked: Instant,
    /// Set when PTT drops. Until PTT rises again, samples must not key the radio: a tail of
    /// non-zero samples was turning transmit back on, with no later PTT edge to send `;RX;`.
    latched_off: bool,
    /// Idle blocks since the unkey. `;RX;` is repeated a few times; one of them is often lost.
    idle_blocks: u32,
    speaker: bool,
}

impl TrusdxIo {
    pub fn open(capture: &str, playback: &str, radio: bool) -> io::Result<Self> {
        let name = port_name(capture, playback)?;
        let shared = Arc::new(Shared::default());
        let (port, reader) = if name == APP {
            let mut modem = APP_PORT.modem.lock().unwrap_or_else(PoisonError::into_inner);
            if modem.is_some() {
                return Err(io::Error::other("the app's truSDX port is already in use"));
            }
            *modem = Some(shared.clone());
            APP_PORT.to_radio.lock().unwrap_or_else(PoisonError::into_inner).clear();
            (Port::App, None)
        } else {
            let f = OpenOptions::new().read(true).write(true).custom_flags(libc::O_NOCTTY).open(&name)?;
            termios(&f, &name)?;
            let (mut r, s) = (f.try_clone()?, shared.clone());
            let reader = thread::Builder::new().name("trusdx".into()).spawn(move || {
                let mut buf = [0u8; 1024];
                while !s.closed.load(Ordering::SeqCst) {
                    match r.read(&mut buf) {
                        Ok(n) => s.receive(&buf[..n]),
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e) => return s.fail(format!("{name}: {e}")),
                    }
                }
            })?;
            (Port::Tty(f), Some(reader))
        };
        set_keyed(false);
        let mut io = Self {
            shared,
            port,
            tx: Codec::new(),
            radio,
            reader,
            due: None,
            asked: Instant::now(),
            latched_off: false,
            idle_blocks: 0,
            speaker: SPEAKER.load(Ordering::SeqCst),
        };
        io.port.write(trusdx::start(io.speaker))?;
        Ok(io)
    }

    pub fn open_capture(capture: &str) -> io::Result<Self> {
        Self::open(capture, capture, false)
    }
}

impl Audio for TrusdxIo {
    /// Radio audio as it comes; when the radio sends none (before it starts streaming, and
    /// while it transmits), silence at the wall clock's pace, so the modem keeps time.
    fn read(&mut self, x: &mut [i16]) -> io::Result<()> {
        let block = Duration::from_secs_f64(x.len() as f64 / f64::from(RATE));
        let now = Instant::now();
        // Blocks fall due on a steady clock, restarted only when far off it.
        let due = match self.due {
            Some(d) if d + STREAMING > now && d < now + STREAMING => d + block,
            _ => now + block,
        };
        self.due = Some(due);
        let mut rx = self.shared.lock();
        // A keyed radio sends no audio, so then there is nothing to wait for.
        let keyed = self.tx.keyed() || KEYED.load(Ordering::SeqCst);
        let streaming = !keyed && rx.heard.is_some_and(|t| now.duration_since(t) < STREAMING);
        let until = if streaming { due + JITTER } else { due };
        while rx.pcm.len() < x.len() && rx.failed.is_none() {
            let Some(left) = until.checked_duration_since(Instant::now()) else { break };
            rx = self.shared.ready.wait_timeout(rx, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        // Samples often arrive before the block is due. Returning then runs the modem, and its
        // transmit audio, faster than the 115200 baud stream. The radio's UART overflows and the
        // `;RX;` that should unkey it is the byte that gets lost.
        while rx.failed.is_none() {
            let Some(left) = due.checked_duration_since(Instant::now()) else { break };
            let (guard, waited) = self.shared.ready.wait_timeout(rx, left).unwrap_or_else(PoisonError::into_inner);
            rx = guard;
            if waited.timed_out() {
                break;
            }
        }
        if let Some(why) = &rx.failed {
            return Err(io::Error::other(why.clone()));
        }
        let n = rx.pcm.len().min(x.len());
        for (o, s) in x.iter_mut().zip(rx.pcm.drain(..n).chain(std::iter::repeat(0))) {
            *o = s;
        }
        let quiet = rx.heard.is_none_or(|t| t.elapsed() > RESTART);
        drop(rx);
        // A transmit sends no received audio, so that silence must not immediately look like a
        // dead stream. Once the radio has been unkeyed, `UA` starts receive audio again.
        if keyed {
            self.asked = Instant::now();
        } else if quiet && self.asked.elapsed() > RESTART {
            self.asked = Instant::now();
            note("resume stream");
            self.port.write(trusdx::resume(self.speaker))?;
        }
        Ok(())
    }

    /// Keys from PTT, or from audio before the first unkey (the PTT flag can lag the first
    /// samples of a frame). After an unkey, only PTT may key again: leftover samples were
    /// turning the radio back on, and nothing afterwards sent `;RX;`. `;RX;` is also repeated
    /// across the next idle blocks. A single one is often lost when it follows a full stream.
    fn write(&mut self, x: &[i16]) -> io::Result<()> {
        if !self.radio {
            return Ok(());
        }
        let ptt = KEYED.load(Ordering::SeqCst);
        let audio = x.iter().any(|&s| s != 0);
        if ptt {
            self.latched_off = false;
        } else if self.tx.keyed() {
            // PTT has dropped while samples are still playing. Unkey now; those samples must
            // not hold the stream open, or key the radio again afterwards.
            self.latched_off = true;
        }
        let keyed = ptt || (audio && !self.latched_off);
        let was = self.tx.keyed();
        let mut bytes = self.tx.send(x, keyed);
        if was && !keyed {
            self.latched_off = true;
            self.idle_blocks = 0;
            note(&format!("unkey ptt={ptt} audio={audio} e={}", CAT_ERRORS.swap(0, Ordering::Relaxed)));
        } else if keyed && !was {
            note(&format!("key ptt={ptt} audio={audio} e={}", CAT_ERRORS.swap(0, Ordering::Relaxed)));
        } else if !keyed && self.latched_off {
            self.idle_blocks = self.idle_blocks.saturating_add(1);
            // A few extra `;RX;` in case the one at the end of the samples was lost, then `UA`
            // so the radio streams receive audio again. `UA` stays out of the sample burst:
            // the next transmission starts with `;`, which closes that stream before `TX0;`.
            if self.idle_blocks == 30 {
                bytes.extend_from_slice(trusdx::start(self.speaker));
                note("stream");
            } else if (1..=2).contains(&(self.idle_blocks / 10)) && self.idle_blocks % 10 == 0 {
                bytes.extend_from_slice(b";RX;");
                note(&format!("unkey repeat {}", self.idle_blocks / 10));
            }
        }
        if bytes.is_empty() { Ok(()) } else { self.port.write(&bytes) }
    }
}

impl Drop for TrusdxIo {
    fn drop(&mut self) {
        set_keyed(false);
        let _ = self.port.write(trusdx::STOP);
        self.shared.closed.store(true, Ordering::SeqCst);
        if let Port::App = self.port {
            *APP_PORT.modem.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exclusive<T>(f: impl FnOnce() -> T) -> T {
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        set_speaker(false);
        let v = f();
        set_speaker(false);
        v
    }

    #[test]
    fn the_port_is_one_device_for_capture_and_playback() {
        assert!(port_name("/dev/ttyUSB0", "/dev/ttyUSB0").is_ok());
        assert!(port_name("/dev/ttyUSB0", "").is_ok());
        assert!(port_name(APP, APP).is_ok());
        assert!(port_name("/dev/ttyUSB0", "/dev/ttyUSB1").is_err());
        assert!(port_name("default", "default").is_err());
    }

    /// The app's side of the port, end to end: it asks the radio to stream, hears its audio,
    /// keys and unkeys it in the stream, and leaves it unkeyed and quiet when closed.
    #[test]
    fn app_port_round_trip() {
        exclusive(|| app_port_round_trip_locked());
    }

    fn app_port_round_trip_locked() {
        let mut io = TrusdxIo::open(APP, APP, true).unwrap();
        assert!(TrusdxIo::open(APP, APP, true).is_err());
        assert_eq!(app_take(64, Duration::ZERO), trusdx::START);
        let mut stream = b"UA2;US".to_vec();
        stream.extend(std::iter::repeat_n(0xC0u8, trusdx::RX_RATE as usize / 10));
        app_received(&stream);
        let mut x = [0i16; super::super::BLOCK];
        io.read(&mut x).unwrap();
        assert!(x[200..].iter().all(|&s| s > 12000), "{:?}", &x[200..210]);

        let quiet = [0i16; super::super::BLOCK];
        io.write(&quiet).unwrap();
        assert!(app_take(4096, Duration::ZERO).is_empty(), "nothing is sent unkeyed");
        set_keyed(true);
        io.write(&quiet).unwrap();
        io.write(&x).unwrap();
        let sent = app_take(4096, Duration::ZERO);
        assert!(sent.starts_with(b";TX0;") && sent.len() > 100);
        set_keyed(false);
        io.write(&quiet).unwrap();
        assert_eq!(app_take(4096, Duration::ZERO), b";RX;");
        // Samples still playing after PTT drops must unkey, and must not key the radio again.
        set_keyed(true);
        io.write(&x).unwrap();
        set_keyed(false);
        io.write(&x).unwrap();
        assert_eq!(app_take(4096, Duration::ZERO).windows(4).any(|w| w == b";RX;"), true);
        io.write(&x).unwrap();
        assert!(app_take(4096, Duration::ZERO).is_empty(), "residual audio must not rekey");
        io.write(&quiet).unwrap();
        // Once PTT has dropped, samples alone must not key the radio again.
        io.write(&x).unwrap();
        assert!(app_take(4096, Duration::ZERO).is_empty(), "audio must not key after an unkey");
        set_keyed(true);
        io.write(&x).unwrap();
        assert!(app_take(4096, Duration::ZERO).starts_with(b";TX0;"));
        set_keyed(false);
        io.write(&quiet).unwrap();
        assert_eq!(app_take(4096, Duration::ZERO), b";RX;");
        let mut repeated = false;
        let mut streamed = false;
        for _ in 0..30 {
            io.write(&quiet).unwrap();
            let got = app_take(4096, Duration::ZERO);
            if got == b";RX;" {
                repeated = true;
            }
            if got == trusdx::START {
                streamed = true;
            }
        }
        assert!(repeated, "an unkey is repeated, one ;RX; is often lost");
        assert!(streamed, "receive streaming starts again after the radio is unkeyed");

        // A block with no radio audio is silence, on time.
        let t = Instant::now();
        for _ in 0..20 {
            io.read(&mut x).unwrap();
        }
        assert!(x.iter().all(|&s| s == 0));
        let took = t.elapsed();
        assert!(took > Duration::from_millis(100) && took < Duration::from_millis(400), "{took:?}");

        app_failed("unplugged");
        assert!(io.read(&mut x).is_err_and(|e| e.to_string().contains("unplugged")));
        drop(io);
        assert_eq!(app_take(64, Duration::ZERO), trusdx::STOP);
        assert!(TrusdxIo::open(APP, APP, true).is_ok(), "free again once closed");
    }

    #[test]
    fn the_radio_speaker_uses_ua1() {
        exclusive(|| {
            set_speaker(true);
            let io = TrusdxIo::open(APP, APP, false).unwrap();
            assert_eq!(app_take(64, Duration::ZERO), trusdx::START_SPEAKER);
            drop(io);
            let _ = app_take(64, Duration::ZERO);
        });
    }
}
