//! Radio CAT and (tr)uSDX audio framing. No sockets: the Android USB layer writes the bytes.

mod cat;
mod resample;
mod trusdx;

pub use cat::{G90_ADDR, Mode, Op, Parser, RadioKind, baud, command, format_frames};
pub use trusdx::{RX_RATE, TX_RATE, Trusdx};
