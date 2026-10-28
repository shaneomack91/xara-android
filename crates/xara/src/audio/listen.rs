//! A copy of the modem's receive and transmit audio, for an app that wants to play it.
//! The modem never waits on a listener: a full queue drops the oldest samples.

use std::collections::VecDeque;
use std::sync::Mutex;

use super::RATE;

const MAX: usize = RATE as usize;

static QUEUE: Mutex<VecDeque<i16>> = Mutex::new(VecDeque::new());

fn lock() -> std::sync::MutexGuard<'static, VecDeque<i16>> {
    QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn clear() {
    lock().clear();
}

/// One mixed block. Receive and transmit are the same length; transmit is silent while receiving.
pub fn push(rx: &[i16], tx: &[i16]) {
    let mut q = lock();
    for (&a, &b) in rx.iter().zip(tx) {
        let s = i32::from(a).saturating_add(i32::from(b));
        q.push_back(s.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
    }
    let extra = q.len().saturating_sub(MAX);
    if extra > 0 {
        q.drain(..extra);
    }
}

/// Little-endian PCM16 into `dst`. Returns how many bytes were written.
pub fn take_le(dst: &mut [u8]) -> usize {
    let mut q = lock();
    let n = (dst.len() / 2).min(q.len());
    for (i, s) in q.drain(..n).enumerate() {
        dst[i * 2..i * 2 + 2].copy_from_slice(&s.to_le_bytes());
    }
    n * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_and_transmit_are_mixed_and_a_full_queue_drops_the_oldest() {
        clear();
        push(&[1000], &[2000]);
        let mut buf = [0u8; 4];
        assert_eq!(take_le(&mut buf), 2);
        assert_eq!(i16::from_le_bytes([buf[0], buf[1]]), 3000);
        assert_eq!(take_le(&mut buf), 0);
        let hot = vec![i16::MAX; MAX + 10];
        push(&hot, &vec![0; hot.len()]);
        assert_eq!(lock().len(), MAX);
        clear();
    }
}
