use std::f64::consts::PI;

const HALF: i64 = 24;
const BETA: f64 = 8.0;

fn bessel(v: f64) -> f64 {
    let (mut s, mut term) = (1.0, 1.0);
    for k in 1..30 {
        term *= (v / 2.0) / k as f64;
        s += term * term;
    }
    s
}

fn kaiser(x: f64, norm: f64) -> f64 {
    if x.abs() >= 1.0 { 0.0 } else { bessel(BETA * (1.0 - x * x).sqrt()) / norm }
}

/// Windowed-sinc resampler. Same shape as the desktop modem's rate converter.
pub struct Resampler {
    step: f64,
    cutoff: f64,
    half: i64,
    window_norm: f64,
    buf: Vec<f64>,
    base: i64,
    t: f64,
}

impl Resampler {
    pub fn new(rate_in: f64, rate_out: f64) -> Self {
        let cutoff = 0.95 * (rate_out / rate_in).min(1.0);
        let half = (HALF as f64 / cutoff.min(1.0)).ceil() as i64;
        Self { step: rate_in / rate_out, cutoff, half, window_norm: bessel(BETA), buf: Vec::new(), base: 0, t: 0.0 }
    }

    pub fn process(&mut self, x: &[f64]) -> Vec<f64> {
        if self.step == 1.0 {
            return x.to_vec();
        }
        self.buf.extend_from_slice(x);
        let end = self.base + self.buf.len() as i64;
        let mut out = Vec::new();
        while (self.t.floor() as i64) + self.half < end {
            let c = self.t.floor() as i64;
            let mut acc = 0.0;
            for i in (c - self.half + 1).max(self.base)..=c + self.half {
                let u = self.t - i as f64;
                let a = PI * self.cutoff * u;
                let s = if a.abs() < 1e-12 { 1.0 } else { a.sin() / a };
                acc += self.buf[(i - self.base) as usize]
                    * self.cutoff
                    * s
                    * kaiser(u / self.half as f64, self.window_norm);
            }
            out.push(acc);
            self.t += self.step;
        }
        let keep_from = (self.t.floor() as i64 - self.half).max(self.base);
        self.buf.drain(..(keep_from - self.base) as usize);
        self.base = keep_from;
        out
    }
}
