#[cfg(aaudio)]
pub mod aaudio;
#[cfg(aaudio)]
mod aaudio_ffi;
#[cfg(alsa)]
pub mod alsa;
#[cfg(coreaudio)]
pub mod coreaudio;
#[cfg(coreaudio)]
mod coreaudio_ffi;
pub mod listen;
#[cfg(pulse)]
pub mod pulse;
#[cfg(pulse)]
mod pulse_ffi;
pub mod trusdx;

#[cfg(not(any(pulse, alsa, coreaudio, aaudio)))]
compile_error!(
    "xara needs an audio backend: enable the pulse or alsa feature (Linux), or build for macOS, iOS or Android"
);

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

pub const RATE: u32 = 48000;
pub const BLOCK: usize = 480;
pub(crate) const STALL: Duration = Duration::from_secs(1);

pub type DeviceList = Vec<(String, String)>;

fn capture_only() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "write to a capture-only audio device")
}

#[cfg(any(pulse, alsa, coreaudio))]
fn remaining(until: std::time::Instant, what: &str) -> io::Result<Duration> {
    until
        .checked_duration_since(std::time::Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, format!("{what}: timed out")))
}

pub trait Audio {
    fn read(&mut self, x: &mut [i16]) -> io::Result<()>;
    fn write(&mut self, x: &[i16]) -> io::Result<()>;
}

pub fn sample(v: i16) -> f64 {
    f64::from(v) / 32768.0
}

pub fn quantize(v: f64) -> i16 {
    (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

pub struct TxAudio {
    delay: VecDeque<f64>,
}

impl TxAudio {
    pub fn new(lead_samples: usize) -> Self {
        Self { delay: std::iter::repeat_n(0.0, lead_samples).collect() }
    }

    pub fn process(&mut self, out: &mut [f64], mute: bool) {
        if mute {
            out.fill(0.0);
            self.delay.iter_mut().for_each(|v| *v = 0.0);
        }
        if !self.delay.is_empty() {
            self.delay.extend(out.iter().copied());
            for o in out.iter_mut() {
                *o = self.delay.pop_front().unwrap();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Pulse,
    Alsa,
    CoreAudio,
    AAudio,
    /// A (tr)uSDX's audio inside its CAT serial stream, on any platform.
    Trusdx,
}

impl Default for Backend {
    fn default() -> Self {
        if cfg!(coreaudio) {
            Self::CoreAudio
        } else if cfg!(aaudio) {
            Self::AAudio
        } else if cfg!(pulse) {
            Self::Pulse
        } else {
            Self::Alsa
        }
    }
}

impl std::str::FromStr for Backend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let b = match s {
            "pulse" => Self::Pulse,
            "alsa" => Self::Alsa,
            "coreaudio" => Self::CoreAudio,
            "aaudio" => Self::AAudio,
            "trusdx" => Self::Trusdx,
            _ => return Err(format!("--audio {s:?}: expected pulse, alsa, coreaudio, aaudio or trusdx")),
        };
        if b.built() { Ok(b) } else { Err(format!("--audio {s}: {}", unbuilt_why(b))) }
    }
}

impl Backend {
    pub fn built(self) -> bool {
        match self {
            Self::Pulse => cfg!(pulse),
            Self::Alsa => cfg!(alsa),
            Self::CoreAudio => cfg!(coreaudio),
            Self::AAudio => cfg!(aaudio),
            Self::Trusdx => true,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pulse => "pulse",
            Self::Alsa => "alsa",
            Self::CoreAudio => "coreaudio",
            Self::AAudio => "aaudio",
            Self::Trusdx => "trusdx",
        }
    }
}

fn unbuilt_why(b: Backend) -> String {
    match b {
        Backend::CoreAudio => "CoreAudio is only available on macOS and iOS".into(),
        Backend::AAudio => "AAudio is only available on Android".into(),
        _ if !cfg!(target_os = "linux") => format!("{} is only available on Linux", b.name()),
        _ => format!("this xara was built without the {} feature", b.name()),
    }
}

#[cfg(not(all(pulse, alsa, coreaudio, aaudio)))]
fn unbuilt(b: Backend) -> io::Error {
    io::Error::other(unbuilt_why(b))
}

pub fn open(b: Backend, capture: &str, playback: &str, radio: bool) -> io::Result<Box<dyn Audio>> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => Ok(Box::new(pulse::PulseIo::open(capture, playback, radio)?)),
        #[cfg(alsa)]
        Backend::Alsa => Ok(Box::new(alsa::AlsaIo::open(capture, playback, radio)?)),
        #[cfg(coreaudio)]
        Backend::CoreAudio => Ok(Box::new(coreaudio::CoreAudioIo::open(capture, playback, radio)?)),
        #[cfg(aaudio)]
        Backend::AAudio => Ok(Box::new(aaudio::AAudioIo::open(capture, playback, radio)?)),
        Backend::Trusdx => Ok(Box::new(trusdx::TrusdxIo::open(capture, playback, radio)?)),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio)))]
        _ => Err(unbuilt(b)),
    }
}

pub fn check_playback(b: Backend, playback: &str, radio: bool) -> io::Result<()> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => pulse::check(playback, radio),
        #[cfg(alsa)]
        Backend::Alsa => alsa::resolve_playback(playback, radio).map(drop),
        #[cfg(coreaudio)]
        Backend::CoreAudio => coreaudio::check(playback, radio),
        #[cfg(aaudio)]
        Backend::AAudio => aaudio::check(playback, radio),
        Backend::Trusdx => trusdx::check(playback, radio),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio)))]
        _ => Err(unbuilt(b)),
    }
}

pub fn list(b: Backend) -> io::Result<(DeviceList, DeviceList)> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => pulse::list(),
        #[cfg(alsa)]
        Backend::Alsa => alsa::list(),
        #[cfg(coreaudio)]
        Backend::CoreAudio => coreaudio::list(),
        #[cfg(aaudio)]
        Backend::AAudio => aaudio::list(),
        Backend::Trusdx => trusdx::list(),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio)))]
        _ => Err(unbuilt(b)),
    }
}

pub fn open_capture(b: Backend, capture: &str) -> io::Result<Box<dyn Audio>> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => Ok(Box::new(pulse::PulseIo::open_capture(capture)?)),
        #[cfg(alsa)]
        Backend::Alsa => Ok(Box::new(alsa::AlsaIo::open_capture(capture)?)),
        #[cfg(coreaudio)]
        Backend::CoreAudio => Ok(Box::new(coreaudio::CoreAudioIo::open_capture(capture)?)),
        #[cfg(aaudio)]
        Backend::AAudio => Ok(Box::new(aaudio::AAudioIo::open_capture(capture)?)),
        Backend::Trusdx => Ok(Box::new(trusdx::TrusdxIo::open_capture(capture)?)),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio)))]
        _ => Err(unbuilt(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_convert_at_the_device_boundary() {
        for s in [i16::MIN, -1, 0, 1, 12345, i16::MAX] {
            assert_eq!(quantize(sample(s)), s);
        }
        assert_eq!((quantize(1.0), quantize(-1.0), quantize(4.0), quantize(-4.0)), (32767, -32768, 32767, -32768));
        assert_eq!((quantize(0.5 / 32768.0), quantize(-0.4 / 32768.0), quantize(f64::NAN)), (1, 0, 0));
        assert_eq!(sample(-32768), -1.0);
    }
}
