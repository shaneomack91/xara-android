//! (tr)uSDX audio inside the CAT serial stream.
//!
//! DL2MAN documents the extension, and the PE1NNZ / DL2MAN firmware implements it:
//! `UA0;` stops streaming, `UA1;` streams with the speaker on, `UA2;` streams with
//! the speaker off. Audio follows `US` as unsigned 8-bit samples until `;`.
//! A sample of `0x3B` (`;`) is not sent: the radio increments it, and hosts replace
//! it with `0x3A`. RX is 7825 Hz, TX is 11520 Hz. `TX0;` keys, `RX;` unkeys.
//! CAT replies may appear between a terminating `;` and the next `US`.

use crate::resample::Resampler;
use std::collections::VecDeque;

pub const RX_RATE: u32 = 7825;
pub const TX_RATE: u32 = 11520;
pub const MODEM_RATE: u32 = 48_000;
const SILENCE: u8 = 0x80;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rx {
    Hunt,
    SawU,
    Audio,
    Cat,
}

pub struct Trusdx {
    rx: Rx,
    cat: Vec<u8>,
    from_radio: Resampler,
    to_radio: Resampler,
    pcm: VecDeque<i16>,
    tx_on: bool,
}

impl Trusdx {
    pub fn new() -> Self {
        Self {
            rx: Rx::Hunt,
            cat: Vec::new(),
            from_radio: Resampler::new(f64::from(RX_RATE), f64::from(MODEM_RATE)),
            to_radio: Resampler::new(f64::from(MODEM_RATE), f64::from(TX_RATE)),
            pcm: VecDeque::new(),
            tx_on: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn enable(speaker: bool) -> &'static [u8] {
        if speaker { b"UA1;" } else { b"UA2;" }
    }

    /// Bytes from the radio. Complete CAT frames include their trailing `;`.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut cats = Vec::new();
        for &b in bytes {
            match self.rx {
                Rx::Hunt => self.hunt(b, &mut cats),
                Rx::SawU => self.saw_u(b, &mut cats),
                Rx::Audio => {
                    if b == b';' {
                        self.rx = Rx::Hunt;
                    } else {
                        self.absorb(b);
                    }
                }
                Rx::Cat => self.cat_byte(b, &mut cats),
            }
        }
        cats
    }

    fn hunt(&mut self, b: u8, cats: &mut Vec<String>) {
        if b == b'U' {
            self.rx = Rx::SawU;
        } else if b == b';' {
        } else {
            self.rx = Rx::Cat;
            self.cat.clear();
            self.cat_byte(b, cats);
        }
    }

    fn saw_u(&mut self, b: u8, cats: &mut Vec<String>) {
        if b == b'S' {
            self.rx = Rx::Audio;
            return;
        }
        self.rx = Rx::Cat;
        self.cat.clear();
        self.cat.push(b'U');
        self.cat_byte(b, cats);
    }

    fn cat_byte(&mut self, b: u8, cats: &mut Vec<String>) {
        if b == b';' {
            self.cat.push(b';');
            if let Ok(s) = std::str::from_utf8(&self.cat) {
                let t = s.trim();
                if !t.is_empty() && t != ";" {
                    cats.push(t.to_string());
                }
            }
            self.cat.clear();
            self.rx = Rx::Hunt;
            return;
        }
        if self.cat.len() < 64 {
            self.cat.push(b);
        } else {
            self.cat.clear();
            self.rx = Rx::Hunt;
        }
    }

    fn absorb(&mut self, b: u8) {
        let x = (f64::from(b) - 128.0) / 128.0;
        for s in self.from_radio.process(&[x]) {
            self.pcm.push_back(quantize(s));
        }
    }

    /// Modem-rate mono samples. Missing radio audio is silence.
    pub fn pull_rx(&mut self, n: usize) -> Vec<i16> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.pcm.pop_front().unwrap_or(0));
        }
        out
    }

    /// Host to radio. Rising PTT sends `TX0;US`, falling PTT sends `;RX;`.
    /// Samples are 48 kHz and leave as escaped unsigned bytes at [`TX_RATE`].
    pub fn tx(&mut self, pcm: &[i16], ptt: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if ptt && !self.tx_on {
            self.tx_on = true;
            self.to_radio = Resampler::new(f64::from(MODEM_RATE), f64::from(TX_RATE));
            out.extend_from_slice(b"TX0;US");
        }
        if ptt {
            let x: Vec<f64> = pcm.iter().map(|&s| f64::from(s) / 32768.0).collect();
            for s in self.to_radio.process(&x) {
                out.push(escape_sample(to_u8(s)));
            }
        } else if self.tx_on {
            self.tx_on = false;
            out.extend_from_slice(b";RX;");
        }
        out
    }
}

impl Default for Trusdx {
    fn default() -> Self {
        Self::new()
    }
}

pub fn escape_sample(s: u8) -> u8 {
    if s == b';' { b':' } else { s }
}

fn to_u8(v: f64) -> u8 {
    let s = (v * 128.0).round().clamp(-128.0, 127.0);
    (s as i32 + i32::from(SILENCE)) as u8
}

fn quantize(v: f64) -> i16 {
    (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ua_and_us_split_cat_from_samples_and_resume_after_a_command() {
        let mut t = Trusdx::new();
        let cats = t.push(b"UA2;US\x80\x81");
        assert_eq!(cats, vec!["UA2;".to_string()]);
        let cats = t.push(b";ID020;US\x82");
        assert_eq!(cats, vec!["ID020;".to_string()]);
        assert_eq!(Trusdx::enable(false), b"UA2;");
        assert_eq!(Trusdx::enable(true), b"UA1;");
    }

    #[test]
    fn ptt_edges_open_us_and_close_with_rx() {
        let mut t = Trusdx::new();
        assert_eq!(t.tx(&[], false), b"");
        assert_eq!(t.tx(&[], true), b"TX0;US");
        assert_eq!(t.tx(&[], true), b"");
        assert_eq!(t.tx(&[], false), b";RX;");
        assert_eq!(t.tx(&[], false), b"");
    }

    #[test]
    fn a_semicolon_sample_is_sent_as_colon() {
        assert_eq!(to_u8(0.0), 0x80);
        assert_eq!(to_u8(-69.0 / 128.0), 0x3B);
        assert_eq!(escape_sample(to_u8(-69.0 / 128.0)), 0x3A);
        let mut t = Trusdx::new();
        t.tx(&[], true);
        let v = ((-69.0_f64 / 128.0) * 32768.0).round() as i16;
        let out = t.tx(&vec![v; 8000], true);
        assert!(!out.contains(&b';'), "audio payload contained a CAT terminator");
        assert!(out.contains(&b':'), "expected the escaped semicolon sample");
    }

    #[test]
    fn rx_audio_expands_toward_the_modem_rate() {
        let mut t = Trusdx::new();
        let mut bytes = b"US".to_vec();
        bytes.extend(std::iter::repeat_n(0xFF, RX_RATE as usize));
        t.push(&bytes);
        let pcm = t.pull_rx(MODEM_RATE as usize);
        let hot = pcm.iter().filter(|&&s| s > 8_000).count();
        assert!(hot > 40_000 && hot < MODEM_RATE as usize, "hot samples {hot}");
    }
}
