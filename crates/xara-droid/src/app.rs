use crate::engine::{self, Running, StartReq};
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use xara_radio::{Mode, Op, Parser, RadioKind, Trusdx, baud, command, format_frames};

struct App {
    running: Option<Running>,
    parser: Parser,
    radio: RadioKind,
    addr: u8,
    trusdx: Trusdx,
    serial_out: VecDeque<u8>,
    cat_out: VecDeque<u8>,
    last_keyed: bool,
    tail_blocks: u32,
    tail_left: u32,
    manual: bool,
    tx: Vec<i16>,
}

impl App {
    fn new() -> Self {
        Self {
            running: None,
            parser: Parser::new(RadioKind::Kenwood),
            radio: RadioKind::Kenwood,
            addr: 0,
            trusdx: Trusdx::new(),
            serial_out: VecDeque::new(),
            cat_out: VecDeque::new(),
            last_keyed: false,
            tail_blocks: 10,
            tail_left: 0,
            manual: false,
            tx: Vec::new(),
        }
    }
}

static APP: LazyLock<Mutex<App>> = LazyLock::new(|| Mutex::new(App::new()));

fn lock() -> MutexGuard<'static, App> {
    APP.lock().unwrap_or_else(PoisonError::into_inner)
}

fn queue(q: &mut VecDeque<u8>, extra: &[u8], dst: &mut [u8]) -> usize {
    q.extend(extra);
    let n = dst.len().min(q.len());
    for slot in dst.iter_mut().take(n) {
        *slot = q.pop_front().unwrap();
    }
    n
}

pub fn block_len() -> usize {
    engine::block()
}

pub fn start(req: &StartReq) -> Result<String, String> {
    stop();
    let running = Running::start(req)?;
    let line = running.line.clone();
    let mut app = lock();
    app.running = Some(running);
    app.last_keyed = false;
    app.tx.clear();
    Ok(line)
}

pub fn process(rx: &[i16], tx: &mut [i16], force_off: bool) -> Result<bool, String> {
    let mut app = lock();
    let keyed = {
        let Some(running) = app.running.as_mut() else {
            return Err("modem is not running".into());
        };
        running.process(rx, tx, force_off)
    };
    app.last_keyed = keyed;
    Ok(keyed)
}

pub fn last_keyed() -> bool {
    lock().last_keyed
}

pub fn status() -> String {
    lock().running.as_ref().map(|r| r.line.clone()).unwrap_or_else(|| "stopped".into())
}

pub fn stop() {
    let mut app = lock();
    app.running = None;
    app.last_keyed = false;
    app.tail_left = 0;
    app.manual = false;
    app.serial_out.clear();
    app.cat_out.clear();
    app.tx.clear();
    app.trusdx.reset();
}

pub fn set_tail(ms: u32) {
    lock().tail_blocks = ms.min(1000).div_ceil(10);
}

pub fn set_manual(on: bool) {
    lock().manual = on;
}

pub fn cat_open(radio: i32, addr: i32) -> Result<(), String> {
    let kind = RadioKind::from_id(radio).ok_or("unknown radio")?;
    if !(0..=255).contains(&addr) {
        return Err("CI-V address must be 0-255".into());
    }
    let mut app = lock();
    app.radio = kind;
    app.addr = addr as u8;
    app.parser.set_kind(kind);
    Ok(())
}

pub fn radio_baud(radio: i32) -> i32 {
    RadioKind::from_id(radio).map(|k| baud(k) as i32).unwrap_or(9600)
}

pub fn cat_build(op: i32, hz: u64, mode: i32) -> Result<Vec<u8>, String> {
    let op = Op::from_id(op).ok_or("unknown CAT command")?;
    let mode = Mode::from_id(mode).unwrap_or(Mode::Usb);
    let app = lock();
    Ok(command(app.radio, app.addr, op, hz, mode))
}

pub fn cat_feed(bytes: &[u8], dst: &mut [u8]) -> usize {
    let mut app = lock();
    let text = app.parser.feed(bytes);
    queue(&mut app.cat_out, text.as_bytes(), dst)
}

pub fn trusdx_enable(speaker: bool) -> &'static [u8] {
    Trusdx::enable(speaker)
}

pub fn end_tx(dst: &mut [u8]) -> usize {
    let mut app = lock();
    let bytes = app.trusdx.tx(&[], false);
    queue(&mut app.serial_out, &bytes, dst)
}

/// One modem block over the (tr)uSDX serial stream.
/// Returns `(serial bytes written, CAT text bytes written, modem wants PTT)`.
pub fn cycle_trusdx(
    serial: &[u8],
    force_off: bool,
    out: &mut [u8],
    cat: &mut [u8],
) -> Result<(usize, usize, bool), String> {
    let n = block_len();
    let mut app = lock();
    let frames = app.trusdx.push(serial);
    let text = format_frames(RadioKind::TruSdx, &frames);
    app.cat_out.extend(text.as_bytes());
    let rx = app.trusdx.pull_rx(n);
    let mut tx = std::mem::take(&mut app.tx);
    if tx.len() != n {
        tx.resize(n, 0);
    }
    let keyed = {
        let Some(running) = app.running.as_mut() else {
            app.tx = tx;
            return Err("modem is not running".into());
        };
        running.process(&rx, &mut tx, force_off)
    };
    let keyed = keyed || app.manual;
    let send = if force_off {
        app.tail_left = 0;
        false
    } else if keyed {
        app.tail_left = app.tail_blocks;
        true
    } else if app.tail_left > 0 {
        app.tail_left -= 1;
        true
    } else {
        false
    };
    let wire = app.trusdx.tx(&tx, send);
    app.tx = tx;
    app.serial_out.extend(wire);
    app.last_keyed = keyed;
    let n_out = queue(&mut app.serial_out, &[], out);
    let n_cat = queue(&mut app.cat_out, &[], cat);
    Ok((n_out, n_cat, keyed))
}
