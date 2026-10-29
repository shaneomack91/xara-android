//! (tr)uSDX audio inside the CAT serial stream. Ported from Shane Curless VA3SHC's
//! xara-android (https://github.com/x0rro/xara/pull/12).
//!
//! DL2MAN documents the extension, and the PE1NNZ / DL2MAN firmware implements it: `UA0;`
//! stops streaming, `UA1;` streams with the speaker on, `UA2;` with it off. Received audio
//! follows `US` as unsigned 8-bit samples at 7825 Hz until `;`, with CAT replies between a
//! `;` and the next `US`. Transmit audio is not a `US` frame: `;TX0;` keys, the following
//! bytes are raw 8-bit samples at 11520 Hz, and `;RX;` unkeys. A sample is never `;`: the
//! radio sends 0x3C for it, and hosts send 0x3A. `UA` starts the receive stream. While that
//! stream is running the radio treats the bytes that follow a late `;TX0;` as CAT, answers
//! `E;`, and drops the `;RX;` that should unkey it, so transmit stops the stream first and
//! sends `;TX0;` before any sample.

use std::collections::VecDeque;

use crate::audio::{quantize, sample, RATE};
use crate::resample::Resampler;

pub const RX_RATE: u32 = 7825;
pub const TX_RATE: u32 = 11520;
/// 8N1 at this rate carries exactly the 11520 bytes a second transmit audio needs.
pub const BAUD: u32 = 115_200;
/// Streaming with the radio's speaker off: the modem is listening, nobody else needs to.
pub const START: &[u8] = b"UA2;";
/// Same stream with the radio's speaker on (`UA1;`).
pub const START_SPEAKER: &[u8] = b"UA1;";
pub const STOP: &[u8] = b";RX;UA0;";
/// Unkey, then start the receive stream again. `;RX;` alone leaves that stream off.
pub const RESUME: &[u8] = b";RX;UA2;";
pub const RESUME_SPEAKER: &[u8] = b";RX;UA1;";

pub fn start(speaker: bool) -> &'static [u8] {
    if speaker {
        START_SPEAKER
    } else {
        START
    }
}

pub fn resume(speaker: bool) -> &'static [u8] {
    if speaker {
        RESUME_SPEAKER
    } else {
        RESUME
    }
}
const SILENCE: u8 = 0x80;
const CAT_MAX: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rx {
    Hunt,
    SawU,
    Audio,
    Cat,
}

/// Splits what the radio sends into audio and CAT replies, and frames audio to send to it.
pub struct Codec {
    rx: Rx,
    cat: Vec<u8>,
    from_radio: Resampler,
    to_radio: Resampler,
    keyed: bool,
}

fn resampler(from: u32, to: u32) -> Resampler {
    Resampler::new(f64::from(from), f64::from(to)).expect("truSDX rates are in range")
}

impl Default for Codec {
    fn default() -> Self {
        Self::new()
    }
}

impl Codec {
    pub fn new() -> Self {
        Self {
            rx: Rx::Hunt,
            cat: Vec::new(),
            from_radio: resampler(RX_RATE, RATE),
            to_radio: resampler(RATE, TX_RATE),
            keyed: false,
        }
    }

    /// Bytes from the radio: audio goes to `pcm` at the modem's rate, and complete CAT replies
    /// (with their `;`) are returned.
    pub fn receive(&mut self, bytes: &[u8], pcm: &mut VecDeque<i16>) -> Vec<String> {
        let (mut cats, mut audio) = (Vec::new(), Vec::new());
        for &b in bytes {
            match (self.rx, b) {
                (Rx::Audio, b';') => self.rx = Rx::Hunt,
                (Rx::Audio, b) => audio.push((f64::from(b) - f64::from(SILENCE)) / 128.0),
                (Rx::Hunt, b'U') => self.rx = Rx::SawU,
                (Rx::Hunt, b';' | b'\r' | b'\n') => {}
                (Rx::SawU, b'S') => self.rx = Rx::Audio,
                (Rx::Hunt, b) => {
                    self.cat.clear();
                    self.cat_byte(b, &mut cats);
                }
                (Rx::SawU, b) => {
                    self.cat.clear();
                    self.cat.push(b'U');
                    self.cat_byte(b, &mut cats);
                }
                (Rx::Cat, b) => self.cat_byte(b, &mut cats),
            }
        }
        pcm.extend(self.from_radio.process(&audio).into_iter().map(quantize));
        cats
    }

    fn cat_byte(&mut self, b: u8, cats: &mut Vec<String>) {
        self.rx = Rx::Cat;
        self.cat.push(b);
        if b == b';' {
            cats.push(String::from_utf8_lossy(&self.cat).trim().to_string());
            self.cat.clear();
            self.rx = Rx::Hunt;
        } else if self.cat.len() > CAT_MAX {
            self.cat.clear();
            self.rx = Rx::Hunt;
        }
    }

    pub fn keyed(&self) -> bool {
        self.keyed
    }

    /// Bytes for the radio carrying `pcm` (at the modem's rate): nothing while unkeyed, `;TX0;`
    /// when `keyed` rises, then raw samples, and `;RX;` when it falls.
    pub fn send(&mut self, pcm: &[i16], keyed: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if keyed && !self.keyed {
            self.to_radio = resampler(RATE, TX_RATE);
            // `;` ends an open receive stream. `TX0;` keys. The samples follow with no `US`:
            // `US` is the radio's own receive stream, and sending it here makes the samples CAT.
            out.extend_from_slice(b";TX0;");
        }
        if keyed {
            let x: Vec<f64> = pcm.iter().map(|&s| sample(s)).collect();
            out.extend(self.to_radio.process(&x).into_iter().map(byte));
        } else if self.keyed {
            out.extend_from_slice(b";RX;");
        }
        self.keyed = keyed;
        out
    }
}

fn byte(v: f64) -> u8 {
    let b = ((v * 128.0).round().clamp(-128.0, 127.0) as i32 + i32::from(SILENCE)) as u8;
    if b == b';' {
        b':'
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cat_replies_come_apart_from_audio_and_streaming_resumes() {
        let mut c = Codec::new();
        let mut pcm = VecDeque::new();
        assert_eq!(c.receive(b"UA2;US\x80\x81", &mut pcm), ["UA2;"]);
        assert_eq!(c.receive(b";ID020;FA00007074000;US\x82", &mut pcm), ["ID020;", "FA00007074000;"]);
        // A command starting with U that is not US stays a command.
        assert_eq!(c.receive(b";UA1;", &mut pcm), ["UA1;"]);
        assert_eq!(c.rx, Rx::Hunt);
    }

    #[test]
    fn received_audio_is_upsampled_to_the_modem_rate() {
        let mut c = Codec::new();
        let mut pcm = VecDeque::new();
        let tone: Vec<u8> = (0..RX_RATE as usize)
            .map(|i| byte(0.5 * (2.0 * std::f64::consts::PI * 1500.0 * i as f64 / f64::from(RX_RATE)).sin()))
            .collect();
        let mut stream = b"US".to_vec();
        stream.extend(&tone);
        c.receive(&stream, &mut pcm);
        let n = pcm.len() as f64;
        assert!((n - f64::from(RATE)).abs() < 200.0, "{n} samples from a second");
        let rms = (pcm.iter().skip(1000).map(|&s| sample(s).powi(2)).sum::<f64>() / (n - 1000.0)).sqrt();
        assert!((rms - 0.5 / 2f64.sqrt()).abs() < 0.02, "rms {rms}");
    }

    #[test]
    fn keying_frames_the_samples_and_never_sends_a_semicolon() {
        let mut c = Codec::new();
        assert!(c.send(&[0; 480], false).is_empty());
        let on = c.send(&[i16::MAX / 2; 4800], true);
        assert!(on.starts_with(b";TX0;"));
        assert!(!on[5..].contains(&b';'));
        let n = on.len() - 5;
        assert!((1100..=1152).contains(&n), "{n} bytes for 100 ms");
        assert!(c.send(&[0; 480], true).len() > 100);
        assert_eq!(c.send(&[0; 480], false), b";RX;");
        assert!(c.send(&[0; 480], false).is_empty());
        assert_eq!(byte(f64::from(b';' as i32 - 128) / 128.0), b':');
    }
}
