//! Kenwood TS-480 ASCII, Yaesu new-CAT, and Icom CI-V (including the Xiegu G90).
//!
//! The G90 USB CAT port uses CI-V. Address `0x70`, 19200 8N1,
//! frequency commands `0x03`/`0x05`, mode `0x04`/`0x06`, PTT `0x1C 0x00`. Kenwood
//! `FA`/`FB`/`MD`/`IF`/`AI`/`TX`/`RX` is the TS-480 set, which (tr)uSDX also emulates.
//! Published sources: Xiegu G90 CI-V reference, Kenwood TS-480 PC command set,
//! Yaesu FT-891 CAT (9-digit `FA`, `TX1`/`TX0`), DL2MAN (tr)uSDX CAT page.

pub const CONTROLLER: u8 = 0xE0;
pub const G90_ADDR: u8 = 0x70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioKind {
    Kenwood = 0,
    G90 = 1,
    Icom = 2,
    Yaesu = 3,
    TruSdx = 4,
}

impl RadioKind {
    pub fn from_id(id: i32) -> Option<Self> {
        Some(match id {
            0 => Self::Kenwood,
            1 => Self::G90,
            2 => Self::Icom,
            3 => Self::Yaesu,
            4 => Self::TruSdx,
            _ => return None,
        })
    }

    fn ascii(self) -> bool {
        matches!(self, Self::Kenwood | Self::Yaesu | Self::TruSdx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Lsb = 0,
    Usb = 1,
    Cw = 2,
    Cwr = 3,
    Fm = 4,
    Am = 5,
    Dig = 6,
    DataR = 7,
}

impl Mode {
    pub fn from_id(id: i32) -> Option<Self> {
        Some(match id {
            0 => Self::Lsb,
            1 => Self::Usb,
            2 => Self::Cw,
            3 => Self::Cwr,
            4 => Self::Fm,
            5 => Self::Am,
            6 => Self::Dig,
            7 => Self::DataR,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Lsb => "LSB",
            Self::Usb => "USB",
            Self::Cw => "CW",
            Self::Cwr => "CW-R",
            Self::Fm => "FM",
            Self::Am => "AM",
            Self::Dig => "DIG",
            Self::DataR => "DATA-R",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Open = 0,
    PttOn = 1,
    PttOff = 2,
    SetFreq = 3,
    GetFreq = 4,
    SetMode = 5,
    GetMode = 6,
    Id = 7,
}

impl Op {
    pub fn from_id(id: i32) -> Option<Self> {
        Some(match id {
            0 => Self::Open,
            1 => Self::PttOn,
            2 => Self::PttOff,
            3 => Self::SetFreq,
            4 => Self::GetFreq,
            5 => Self::SetMode,
            6 => Self::GetMode,
            7 => Self::Id,
            _ => return None,
        })
    }
}

pub fn baud(kind: RadioKind) -> u32 {
    match kind {
        RadioKind::Kenwood => 9600,
        RadioKind::G90 | RadioKind::Icom => 19200,
        RadioKind::Yaesu => 38400,
        RadioKind::TruSdx => 115200,
    }
}

pub fn command(kind: RadioKind, addr: u8, op: Op, hz: u64, mode: Mode) -> Vec<u8> {
    if kind.ascii() {
        return ascii_command(kind, op, hz, mode);
    }
    civ_command(kind, addr, op, hz, mode)
}

fn ascii_command(kind: RadioKind, op: Op, hz: u64, mode: Mode) -> Vec<u8> {
    let s = match op {
        Op::Open => "AI0;".to_string(),
        Op::Id => "ID;".to_string(),
        Op::GetFreq => "FA;".to_string(),
        Op::SetFreq if kind == RadioKind::Yaesu => format!("FA{:09};", hz.min(999_999_999)),
        Op::SetFreq => format!("FA{hz:011};"),
        Op::GetMode if kind == RadioKind::Yaesu => "MD0;".to_string(),
        Op::GetMode => "MD;".to_string(),
        Op::SetMode if kind == RadioKind::Yaesu => format!("MD0{};", yaesu_mode(mode)),
        Op::SetMode => format!("MD{};", kenwood_mode(mode) as char),
        Op::PttOn if kind == RadioKind::Yaesu => "TX1;".to_string(),
        Op::PttOff if kind == RadioKind::Yaesu => "TX0;".to_string(),
        Op::PttOn if kind == RadioKind::TruSdx => "TX0;".to_string(),
        Op::PttOn => "TX;".to_string(),
        Op::PttOff => "RX;".to_string(),
    };
    s.into_bytes()
}

fn kenwood_mode(mode: Mode) -> u8 {
    match mode {
        Mode::Lsb => b'1',
        Mode::Usb | Mode::Dig => b'2',
        Mode::Cw => b'3',
        Mode::Fm => b'4',
        Mode::Am => b'5',
        Mode::DataR => b'9',
        Mode::Cwr => b'7',
    }
}

fn kenwood_mode_from(d: u8) -> Option<Mode> {
    Some(match d {
        b'1' => Mode::Lsb,
        b'2' => Mode::Usb,
        b'3' => Mode::Cw,
        b'4' => Mode::Fm,
        b'5' => Mode::Am,
        b'6' => Mode::Dig,
        b'7' => Mode::Cwr,
        b'9' => Mode::DataR,
        _ => return None,
    })
}

/// FT-891: 1 LSB, 2 USB, 3 CW, 4 FM, 5 AM, 6 DATA, 7 CW-R.
fn yaesu_mode(mode: Mode) -> char {
    match mode {
        Mode::Lsb => '1',
        Mode::Usb => '2',
        Mode::Cw => '3',
        Mode::Fm => '4',
        Mode::Am => '5',
        Mode::Dig | Mode::DataR => '6',
        Mode::Cwr => '7',
    }
}

fn yaesu_mode_from(d: u8) -> Option<Mode> {
    Some(match d {
        b'1' => Mode::Lsb,
        b'2' => Mode::Usb,
        b'3' => Mode::Cw,
        b'4' => Mode::Fm,
        b'5' => Mode::Am,
        b'6' | b'C' | b'c' => Mode::Dig,
        b'7' | b'D' | b'd' => Mode::Cwr,
        _ => return None,
    })
}

fn civ_command(kind: RadioKind, addr: u8, op: Op, hz: u64, mode: Mode) -> Vec<u8> {
    let data: Vec<u8> = match op {
        Op::Open | Op::Id => return Vec::new(),
        Op::GetFreq => vec![0x03],
        Op::SetFreq => {
            let mut d = vec![0x05];
            d.extend(bcd5(hz));
            d
        }
        Op::GetMode => vec![0x04],
        Op::SetMode if kind == RadioKind::Icom => vec![0x06, icom_mode(mode), 0x01],
        Op::SetMode => vec![0x06, g90_mode(mode)],
        Op::PttOn => vec![0x1C, 0x00, 0x01],
        Op::PttOff => vec![0x1C, 0x00, 0x00],
    };
    civ(addr, CONTROLLER, &data)
}

fn civ(to: u8, from: u8, data: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFE, 0xFE, to, from];
    v.extend_from_slice(data);
    v.push(0xFD);
    v
}

/// G90 table 2-2: 0 LSB, 1 USB, 2 AM, 3 CW, 7 CW-R. No FM; digital is USB.
fn g90_mode(mode: Mode) -> u8 {
    match mode {
        Mode::Lsb => 0x00,
        Mode::Usb | Mode::Dig | Mode::DataR | Mode::Fm => 0x01,
        Mode::Am => 0x02,
        Mode::Cw => 0x03,
        Mode::Cwr => 0x07,
    }
}

fn g90_mode_from(b: u8) -> Option<Mode> {
    Some(match b {
        0x00 => Mode::Lsb,
        0x01 => Mode::Usb,
        0x02 => Mode::Am,
        0x03 => Mode::Cw,
        0x07 => Mode::Cwr,
        _ => return None,
    })
}

/// Icom CI-V: 0 LSB, 1 USB, 2 AM, 3 CW, 4 RTTY, 5 FM, 7 CW-R, 8 RTTY-R.
fn icom_mode(mode: Mode) -> u8 {
    match mode {
        Mode::Lsb => 0x00,
        Mode::Usb | Mode::Dig => 0x01,
        Mode::Am => 0x02,
        Mode::Cw => 0x03,
        Mode::Fm => 0x05,
        Mode::Cwr => 0x07,
        Mode::DataR => 0x08,
    }
}

fn icom_mode_from(b: u8) -> Option<Mode> {
    Some(match b {
        0x00 => Mode::Lsb,
        0x01 => Mode::Usb,
        0x02 => Mode::Am,
        0x03 => Mode::Cw,
        0x04 => Mode::Dig,
        0x05 => Mode::Fm,
        0x07 => Mode::Cwr,
        0x08 => Mode::DataR,
        _ => return None,
    })
}

fn bcd5(mut hz: u64) -> [u8; 5] {
    let mut o = [0u8; 5];
    for b in &mut o {
        let lo = (hz % 10) as u8;
        hz /= 10;
        let hi = (hz % 10) as u8;
        hz /= 10;
        *b = (hi << 4) | lo;
    }
    o
}

fn decode_bcd5(b: &[u8]) -> Option<u64> {
    if b.len() < 5 {
        return None;
    }
    let (mut hz, mut mul) = (0u64, 1u64);
    for &byte in &b[..5] {
        let (lo, hi) = (byte & 0x0f, byte >> 4);
        if lo > 9 || hi > 9 {
            return None;
        }
        hz += u64::from(lo) * mul;
        mul *= 10;
        hz += u64::from(hi) * mul;
        mul *= 10;
    }
    Some(hz)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Freq(u64),
    Mode(Mode),
    Ptt(bool),
    Raw(String),
}

pub struct Parser {
    kind: RadioKind,
    buf: Vec<u8>,
}

impl Parser {
    pub fn new(kind: RadioKind) -> Self {
        Self { kind, buf: Vec::new() }
    }

    pub fn set_kind(&mut self, kind: RadioKind) {
        self.kind = kind;
        self.buf.clear();
    }

    pub fn feed(&mut self, bytes: &[u8]) -> String {
        self.buf.extend_from_slice(bytes);
        let events = if self.kind.ascii() { self.pull_ascii() } else { self.pull_civ() };
        format_events(&events)
    }

    fn pull_ascii(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(i) = self.buf.iter().position(|&b| b == b';') {
            let frame: Vec<u8> = self.buf.drain(..=i).collect();
            if frame.len() > 64 {
                continue;
            }
            let Ok(text) = std::str::from_utf8(&frame) else { continue };
            let text = text.trim();
            if text.is_empty() || text == ";" {
                continue;
            }
            out.extend(parse_ascii(self.kind, text));
        }
        if self.buf.len() > 128 {
            self.buf.clear();
        }
        out
    }

    fn pull_civ(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            let Some(start) = self.buf.windows(2).position(|w| w == [0xFE, 0xFE]) else {
                if self.buf.len() > 128 || self.buf.last() != Some(&0xFE) {
                    self.buf.clear();
                } else if self.buf.len() > 1 {
                    self.buf.drain(..self.buf.len() - 1);
                }
                break;
            };
            self.buf.drain(..start);
            let Some(end) = self.buf.iter().position(|&b| b == 0xFD) else {
                if self.buf.len() > 128 {
                    self.buf.clear();
                }
                break;
            };
            let frame: Vec<u8> = self.buf.drain(..=end).collect();
            if let Some(ev) = parse_civ(self.kind, &frame) {
                out.extend(ev);
            }
        }
        out
    }
}

fn parse_ascii(kind: RadioKind, frame: &str) -> Vec<Event> {
    let mut out = vec![Event::Raw(frame.to_string())];
    let b = frame.as_bytes();
    if frame.starts_with("FA") || frame.starts_with("FB") {
        if let Some(hz) = digits(&frame[2..frame.len() - 1]) {
            out.push(Event::Freq(hz));
        }
    } else if kind == RadioKind::Yaesu && frame.starts_with("MD0") && b.len() >= 4 {
        if let Some(m) = yaesu_mode_from(b[3]) {
            out.push(Event::Mode(m));
        }
    } else if frame.starts_with("MD") && b.len() >= 3 {
        if let Some(m) = kenwood_mode_from(b[2]) {
            out.push(Event::Mode(m));
        }
    } else if frame.starts_with("IF") {
        out.extend(parse_if(frame));
    } else if kind == RadioKind::Yaesu && (frame.starts_with("TX0") || frame == "RX;") {
        out.push(Event::Ptt(false));
    } else if kind == RadioKind::Yaesu && frame.starts_with("TX") {
        out.push(Event::Ptt(true));
    } else if frame.starts_with("RX") {
        out.push(Event::Ptt(false));
    } else if frame.starts_with("TX") {
        out.push(Event::Ptt(true));
    }
    out
}

/// TS-480/TS-2000 IF layout. Frequency is 11 digits at [2..13]. TX is at index 28, mode at 29.
fn parse_if(frame: &str) -> Vec<Event> {
    let b = frame.as_bytes();
    let mut out = Vec::new();
    if b.len() >= 14 && b[b.len() - 1] == b';' {
        if let Some(hz) = digits(&frame[2..13]) {
            out.push(Event::Freq(hz));
        }
    }
    if b.len() >= 38 {
        out.push(Event::Ptt(b[28] == b'1'));
        if let Some(m) = kenwood_mode_from(b[29]) {
            out.push(Event::Mode(m));
        }
    }
    out
}

fn digits(s: &str) -> Option<u64> {
    let t = s.trim().trim_end_matches(';');
    if t.is_empty() || !t.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

fn parse_civ(kind: RadioKind, frame: &[u8]) -> Option<Vec<Event>> {
    if frame.len() < 6 || frame[0] != 0xFE || frame[1] != 0xFE || *frame.last()? != 0xFD {
        return None;
    }
    let src = frame[3];
    if src == CONTROLLER {
        return None;
    }
    let data = &frame[4..frame.len() - 1];
    if data.is_empty() {
        return None;
    }
    let mut out = vec![Event::Raw(hex(frame))];
    match data[0] {
        0x00 | 0x03 | 0x05 if data.len() >= 6 => {
            if let Some(hz) = decode_bcd5(&data[1..6]) {
                out.push(Event::Freq(hz));
            }
        }
        0x01 | 0x04 | 0x06 if data.len() >= 2 => {
            let mode = if kind == RadioKind::Icom { icom_mode_from(data[1]) } else { g90_mode_from(data[1]) };
            if let Some(m) = mode {
                out.push(Event::Mode(m));
            }
        }
        0x1C if data.len() >= 3 && data[1] == 0x00 => out.push(Event::Ptt(data[2] == 0x01)),
        0xFB => {}
        0xFA => {}
        _ => {}
    }
    Some(out)
}

fn hex(frame: &[u8]) -> String {
    frame.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn format_events(events: &[Event]) -> String {
    let mut s = String::new();
    for e in events {
        let line = match e {
            Event::Freq(hz) => format!("F {hz}"),
            Event::Mode(m) => format!("M {}", m.name()),
            Event::Ptt(on) => format!("P {}", u8::from(*on)),
            Event::Raw(r) => format!("R {r}"),
        };
        s.push_str(&line);
        s.push('\n');
    }
    s
}

pub fn format_frames(kind: RadioKind, frames: &[String]) -> String {
    let mut events = Vec::new();
    for f in frames {
        events.extend(parse_ascii(kind, f));
    }
    format_events(&events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kenwood_sets_eleven_digit_frequency_mode_and_ptt() {
        let hz = command(RadioKind::Kenwood, 0, Op::SetFreq, 14_074_000, Mode::Usb);
        assert_eq!(hz, b"FA00014074000;");
        assert_eq!(command(RadioKind::Kenwood, 0, Op::GetFreq, 0, Mode::Usb), b"FA;");
        assert_eq!(command(RadioKind::Kenwood, 0, Op::SetMode, 0, Mode::Usb), b"MD2;");
        assert_eq!(command(RadioKind::Kenwood, 0, Op::SetMode, 0, Mode::Lsb), b"MD1;");
        assert_eq!(command(RadioKind::Kenwood, 0, Op::PttOn, 0, Mode::Usb), b"TX;");
        assert_eq!(command(RadioKind::Kenwood, 0, Op::PttOff, 0, Mode::Usb), b"RX;");
        assert_eq!(command(RadioKind::Kenwood, 0, Op::Open, 0, Mode::Usb), b"AI0;");
        assert_eq!(command(RadioKind::TruSdx, 0, Op::PttOn, 0, Mode::Usb), b"TX0;");
        assert_eq!(baud(RadioKind::TruSdx), 115200);
    }

    #[test]
    fn kenwood_replies_carry_frequency_mode_and_ptt() {
        let mut p = Parser::new(RadioKind::Kenwood);
        let s = p.feed(b"FA00014074000;MD2;");
        assert!(s.contains("F 14074000"), "{s}");
        assert!(s.contains("M USB"), "{s}");
        let frame = if_frame(b'0');
        assert_eq!(frame.len(), 38, "{frame} {}", frame.len());
        let s = p.feed(frame.as_bytes());
        assert!(s.contains("F 14074000"), "{s}");
        assert!(s.contains("M USB"), "{s}");
        assert!(s.contains("P 0"), "{s}");
        let s = p.feed(if_frame(b'1').as_bytes());
        assert!(s.contains("P 1"), "{s}");
    }

    #[test]
    fn g90_civ_frequency_mode_and_ptt_match_the_manual() {
        let set = command(RadioKind::G90, G90_ADDR, Op::SetFreq, 14_074_000, Mode::Usb);
        assert_eq!(set, [0xFE, 0xFE, 0x70, 0xE0, 0x05, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]);
        let read = command(RadioKind::G90, G90_ADDR, Op::GetFreq, 0, Mode::Usb);
        assert_eq!(read, [0xFE, 0xFE, 0x70, 0xE0, 0x03, 0xFD]);
        let mode = command(RadioKind::G90, G90_ADDR, Op::SetMode, 0, Mode::Usb);
        assert_eq!(mode, [0xFE, 0xFE, 0x70, 0xE0, 0x06, 0x01, 0xFD]);
        let cw = command(RadioKind::G90, G90_ADDR, Op::SetMode, 0, Mode::Cw);
        assert_eq!(cw, [0xFE, 0xFE, 0x70, 0xE0, 0x06, 0x03, 0xFD]);
        let ptt = command(RadioKind::G90, G90_ADDR, Op::PttOn, 0, Mode::Usb);
        assert_eq!(ptt, [0xFE, 0xFE, 0x70, 0xE0, 0x1C, 0x00, 0x01, 0xFD]);
        let off = command(RadioKind::G90, G90_ADDR, Op::PttOff, 0, Mode::Usb);
        assert_eq!(off, [0xFE, 0xFE, 0x70, 0xE0, 0x1C, 0x00, 0x00, 0xFD]);
        assert_eq!(baud(RadioKind::G90), 19200);

        let mut p = Parser::new(RadioKind::G90);
        let echo = [0xFE, 0xFE, 0x70, 0xE0, 0x03, 0xFD];
        let reply = [0xFE, 0xFE, 0xE0, 0x70, 0x03, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD];
        let mut bytes = echo.to_vec();
        bytes.extend(reply);
        let s = p.feed(&bytes);
        assert!(!s.contains("from echo") && s.contains("F 14074000"), "{s}");
        assert_eq!(s.matches("F ").count(), 1, "{s}");
        let mode_reply = [0xFE, 0xFE, 0xE0, 0x70, 0x04, 0x01, 0xFD];
        let s = p.feed(&mode_reply);
        assert!(s.contains("M USB"), "{s}");
        let ptt_reply = [0xFE, 0xFE, 0xE0, 0x70, 0x1C, 0x00, 0x01, 0xFD];
        let s = p.feed(&ptt_reply);
        assert!(s.contains("P 1"), "{s}");
    }

    #[test]
    fn icom_mode_set_includes_a_filter_byte() {
        let mode = command(RadioKind::Icom, 0x94, Op::SetMode, 0, Mode::Usb);
        assert_eq!(mode, [0xFE, 0xFE, 0x94, 0xE0, 0x06, 0x01, 0x01, 0xFD]);
        let fm = command(RadioKind::Icom, 0x94, Op::SetMode, 0, Mode::Fm);
        assert_eq!(fm[5], 0x05);
    }

    #[test]
    fn yaesu_uses_nine_digits_and_tx0_means_receive() {
        let hz = command(RadioKind::Yaesu, 0, Op::SetFreq, 14_074_000, Mode::Usb);
        assert_eq!(hz, b"FA014074000;");
        assert_eq!(command(RadioKind::Yaesu, 0, Op::PttOn, 0, Mode::Usb), b"TX1;");
        assert_eq!(command(RadioKind::Yaesu, 0, Op::PttOff, 0, Mode::Usb), b"TX0;");
        assert_eq!(command(RadioKind::Yaesu, 0, Op::SetMode, 0, Mode::Dig), b"MD06;");
        let mut p = Parser::new(RadioKind::Yaesu);
        let s = p.feed(b"FA014074000;MD02;TX0;");
        assert!(s.contains("F 14074000"), "{s}");
        assert!(s.contains("M USB"), "{s}");
        assert!(s.contains("P 0"), "{s}");
    }

    fn if_frame(tx: u8) -> String {
        let mut p = String::from("IF00014074000");
        p.push_str("     ");
        p.push_str("+0000");
        p.push_str("000");
        p.push_str("00");
        p.push(tx as char);
        p.push('2');
        p.push_str("0000");
        p.push_str("00");
        p.push('0');
        p.push(';');
        p
    }

    #[test]
    fn seven_mhz_bcd_round_trips() {
        let set = command(RadioKind::G90, G90_ADDR, Op::SetFreq, 7_074_000, Mode::Lsb);
        assert_eq!(&set[5..10], &[0x00, 0x40, 0x07, 0x07, 0x00]);
        let mut reply = vec![0xFE, 0xFE, 0xE0, 0x70, 0x03];
        reply.extend_from_slice(&set[5..10]);
        reply.push(0xFD);
        let s = Parser::new(RadioKind::G90).feed(&reply);
        assert!(s.contains("F 7074000"), "{s}");
    }
}
