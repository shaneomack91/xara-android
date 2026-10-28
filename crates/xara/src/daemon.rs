use crate::audio::{self, Audio, Backend};
use crate::cli::{self, UiArgs, Usage, num};
use crate::ptt::{PttCtl, PttSpec, PttTiming};
use crate::record::{self, Recorder};
use crate::signals;
use crate::station::{self, DEFAULT_DRIVE_DB, Modem, Options};
use crate::tnc::Tnc;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use xara_arq::{fm, hf};
use xara_host::kiss::KissServer;
use xara_host::server::Host;
use xara_phy::fm::level::FmBandwidth;
use xara_phy::hf::level::{Bandwidth, Record};
use xara_ui::messages::Info;
use xara_ui::server::{Access, UiServer, new_token, stored_token, valid_token};
use xara_ui::settings::{self, Device, Devices, Entry, Init, Pairs, Settings};
use xara_ui::telemetry::{self, EVENT_QUEUE};

const USAGE: Usage = Usage {
    name: "xara",
    synopsis: "usage: xara [--config FILE] --call CALL --capture SOURCE\n            \
               --playback SINK [options]",
    tagline: "a VARA-compatible HF/FM modem",
    sections: &["minimum config", "options", "PTT settings"],
};

const SECOND_SIGNAL_GRACE: Duration = Duration::from_secs(2);
const EXIT_RESTART_FAILED: u8 = 75;
const TOKEN_ENV: &str = "XARA_UI_TOKEN";
const RECORD_PART_ENV: &str = "XARA_RECORD_PART";

struct Args {
    audio: Backend,
    capture: String,
    playback: String,
    call: String,
    digi: String,
    bw: Bandwidth,
    fm: Option<FmBandwidth>,
    port: u16,
    kiss_port: Option<u16>,
    persistence: f64,
    addr: String,
    drive_db: f64,
    quiet: bool,
    ui: UiArgs,
    ui_control: bool,
    ui_token: Option<String>,
    state_dir: Option<String>,
    registered: bool,
    offer_level: Option<usize>,
    radio: bool,
    ptt: Option<PttSpec>,
    timing: PttTiming,
    /// Delay in the audio path that xara does not add itself (a phone's own speaker and mic,
    /// a network audio link): allowed for in its timing, with nothing inserted.
    audio_latency: Duration,
    record: Option<PathBuf>,
    /// A text log of everything xara logs, its frames and once a second the audio levels.
    debug_log: Option<PathBuf>,
    hf_given: bool,
    monitor: bool,
    monitor_frames: bool,
    heard: Vec<String>,
    /// (tr)uSDX plays the CAT audio stream on its own speaker (`UA1;` instead of `UA2;`).
    trusdx_speaker: bool,
}

fn finite(k: &str, v: &str) -> Result<f64, String> {
    let x: f64 = num(k, v)?;
    if x.is_finite() { Ok(x) } else { Err(format!("{k}: {v} is not a finite number")) }
}

fn defaults() -> Args {
    Args {
        audio: Backend::default(),
        capture: String::new(),
        playback: String::new(),
        call: String::new(),
        digi: String::new(),
        bw: Bandwidth::Hz2300,
        fm: None,
        port: 8300,
        kiss_port: Some(xara_host::kiss::DEFAULT_PORT),
        persistence: xara_arq::hf::broadcast::DEFAULT_PERSISTENCE,
        addr: crate::DEFAULT_ADDR.into(),
        drive_db: DEFAULT_DRIVE_DB,
        quiet: false,
        ui: UiArgs::default(),
        ui_control: false,
        ui_token: None,
        state_dir: None,
        registered: false,
        offer_level: None,
        radio: false,
        ptt: None,
        timing: PttTiming::default(),
        audio_latency: Duration::ZERO,
        record: None,
        debug_log: None,
        hf_given: false,
        monitor: false,
        monitor_frames: false,
        heard: Vec::new(),
        trusdx_speaker: false,
    }
}

fn take(a: &mut Args, k: &str, mut v: impl FnMut() -> Result<String, String>) -> Result<(), String> {
    if a.ui.take(k, &mut v)? {
        return Ok(());
    }
    match k {
        "--audio" => a.audio = v()?.parse()?,
        "--capture" => a.capture = v()?,
        "--playback" => a.playback = v()?,
        "--call" => a.call = cli::call(&v()?)?,
        "--digi" => {
            a.digi = v()?.to_ascii_uppercase();
            if !xara_arq::cq::valid_call(&a.digi) {
                return Err(format!("bad digipeater call {}", a.digi));
            }
        }
        "--hf" => {
            a.bw = v()?.parse()?;
            a.hf_given = true;
        }
        "--fm" => {
            a.fm = Some(match v()?.to_ascii_lowercase().as_str() {
                "narrow" => FmBandwidth::Narrow,
                "wide" => FmBandwidth::Wide,
                m => return Err(format!("bad FM mode {m}")),
            })
        }
        "--port" => a.port = num(k, &v()?)?,
        "--kiss-port" => {
            a.kiss_port = match v()?.as_str() {
                "off" | "OFF" => None,
                p => Some(num(k, p)?),
            }
        }
        "--persistence" => a.persistence = finite(k, &v()?)?.clamp(0.0, 1.0),
        "--addr" => a.addr = v()?,
        "--drive-db" => a.drive_db = finite(k, &v()?)?,
        "--quiet" => a.quiet = true,
        "--ui-control" => a.ui_control = true,
        "--ui-token" => {
            let t = v()?;
            if !valid_token(&t) {
                return Err("--ui-token must be 8-128 characters of A-Z, a-z, 0-9, - and _".into());
            }
            a.ui_token = Some(t);
        }
        "--state-dir" => a.state_dir = Some(v()?),
        "--radio" => a.radio = true,
        "--ptt" => a.ptt = Some(PttSpec::parse(&v()?)?),
        "--ptt-lead-ms" => a.timing.lead = Duration::from_millis(num(k, &v()?)?),
        "--ptt-tail-ms" => a.timing.tail = Duration::from_millis(num(k, &v()?)?),
        "--ptt-max-s" => a.timing.max_on = Duration::from_secs(num(k, &v()?)?),
        "--audio-latency-ms" => a.audio_latency = Duration::from_millis(num(k, &v()?)?),
        "--record" => a.record = Some(PathBuf::from(v()?)),
        "--debug-log" => a.debug_log = Some(PathBuf::from(v()?)),
        "--monitor" => a.monitor = true,
        "--monitor-frames" => a.monitor_frames = true,
        "--monitor-call" => a.heard.push(cli::call(&v()?)?),
        "--trusdx-speaker" => a.trusdx_speaker = true,
        #[cfg(feature = "registered")]
        "--registered" => a.registered = true,
        #[cfg(not(feature = "registered"))]
        "--registered" => return Err("not built with registered features".into()),
        #[cfg(feature = "registered")]
        "--offer-level" => a.offer_level = Some(num(k, &v()?)?),
        _ => return Err(format!("unknown option {k}")),
    }
    Ok(())
}

fn parse(argv: &[String]) -> Result<Option<Args>, String> {
    if let Some(m) = argv.iter().find(|k| cli::is_meta(k)) {
        cli::meta(m, &USAGE);
        return Ok(None);
    }
    build(argv, &Pairs::new()).map(Some)
}

fn build(argv: &[String], saved: &Pairs) -> Result<Args, String> {
    let mut a = defaults();
    let mut it = argv.iter();
    while let Some(k) = it.next() {
        take(&mut a, k, || it.next().cloned().ok_or(format!("{k} needs a value")))?;
    }
    for (k, v) in saved {
        if !settings::KEYS.contains(&k.as_str()) {
            return Err(format!("unknown setting {k}"));
        }
        let flag = format!("--{k}");
        match k.as_str() {
            "debug-log" | "record" => {
                let p = match v.as_str() {
                    "yes" => Some(state_dir(&a).ok_or(format!("{k} needs a state directory"))?.join(saved_file(k))),
                    _ => None,
                };
                if k == "record" { a.record = p } else { a.debug_log = p }
            }
            "monitor-call" => {
                a.heard.clear();
                for c in v.split_whitespace() {
                    take(&mut a, &flag, || Ok(c.to_string()))?;
                }
            }
            _ if settings::SWITCHES.contains(&k.as_str()) && v == "no" => *switch(&mut a, k) = false,
            _ => take(&mut a, &flag, || Ok(v.clone()))?,
        }
    }
    check(&a)?;
    Ok(a)
}

fn switch<'a>(a: &'a mut Args, k: &str) -> &'a mut bool {
    match k {
        "registered" => &mut a.registered,
        "monitor" => &mut a.monitor,
        _ => &mut a.monitor_frames,
    }
}

/// Where the debug-log and record settings write, in the state directory: one log, and a
/// recording for each start, named by its UTC time as the macOS app names them.
fn saved_file(k: &str) -> String {
    if k == "debug-log" {
        return "debug.log".into();
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let t: String = record::utc(now).chars().filter(char::is_ascii_digit).collect();
    format!("xara-{}-{}.flac", &t[..8], &t[8..])
}

fn check(a: &Args) -> Result<(), String> {
    if a.capture.is_empty() || a.playback.is_empty() {
        return Err("--capture and --playback are required".into());
    }
    if a.call.is_empty() {
        return Err("--call is required".into());
    }
    if a.call.split('-').next().is_some_and(|c| c.eq_ignore_ascii_case("N0CALL")) {
        return Err("--call: N0CALL is not a callsign".into());
    }
    if a.port == u16::MAX {
        return Err("--port must be below 65535: the data port is --port + 1".into());
    }
    if a.ui.port.is_none() && (a.ui_control || a.ui_token.is_some() || !a.ui.hosts.is_empty()) {
        return Err("--ui-control, --ui-token and --ui-host need --ui-port".into());
    }
    if a.ui_token.is_some() && !a.ui_control {
        return Err("--ui-token needs --ui-control".into());
    }
    if a.hf_given && a.fm.is_some() {
        return Err("--hf and --fm cannot both be given".into());
    }
    if a.monitor_frames && !a.monitor {
        return Err("--monitor-frames needs --monitor".into());
    }
    if a.monitor && a.fm.is_some() {
        return Err("--monitor decodes VARA HF; it is not available with --fm".into());
    }
    if !a.digi.is_empty() && a.fm.is_none() {
        return Err("--digi needs --fm".into());
    }
    if a.drive_db > 0.0 {
        return Err("--drive-db must be <= 0".into());
    }
    match (a.radio, &a.ptt) {
        (false, Some(_)) => return Err("--ptt needs --radio".into()),
        (true, None) => return Err("--radio needs --ptt (use --ptt vox for VOX)".into()),
        _ => {}
    }
    match (a.audio == Backend::Trusdx, a.ptt == Some(PttSpec::Trusdx)) {
        (false, true) => return Err("--ptt trusdx needs --audio trusdx".into()),
        (true, false) if a.radio => return Err("--audio trusdx keys the radio over CAT: use --ptt trusdx".into()),
        _ => {}
    }
    if a.trusdx_speaker && a.audio != Backend::Trusdx {
        return Err("--trusdx-speaker needs --audio trusdx".into());
    }
    if a.timing.lead > Duration::from_secs(1) || a.timing.tail > Duration::from_secs(1) {
        return Err("--ptt-lead-ms and --ptt-tail-ms must be <= 1000".into());
    }
    if a.audio_latency > Duration::from_secs(1) {
        return Err("--audio-latency-ms must be <= 1000".into());
    }
    if a.timing.max_on.is_zero() || a.timing.max_on > Duration::from_secs(120) {
        return Err("--ptt-max-s must be 1..120".into());
    }
    Ok(())
}

fn config_args(path: &Path) -> Result<Vec<String>, String> {
    let at = |e: String| format!("{}: {e}", path.display());
    let text = std::fs::read_to_string(path).map_err(|e| at(e.to_string()))?;
    let mut probe = defaults();
    let mut out = Vec::new();
    for Entry { line, key, value } in settings::entries(&text).map_err(at)? {
        let at = |e: String| at(format!("line {line}: {e}"));
        if key == "config" {
            return Err(at("config cannot be set from a config file".into()));
        }
        let flag = format!("--{key}");
        let mut wants_value = false;
        take(&mut probe, &flag, || {
            wants_value = true;
            value.clone().ok_or(format!("{key} needs a value"))
        })
        .map_err(at)?;
        match (wants_value, value.as_deref()) {
            (true, _) => out.extend(std::iter::once(flag).chain(value)),
            (false, None | Some("yes" | "true" | "on")) => out.push(flag),
            (false, Some("no" | "false" | "off")) => {}
            (false, Some(_)) => return Err(at(format!("{key} is a switch: write {key}, or {key} = yes|no"))),
        }
    }
    Ok(out)
}

fn default_config() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("xara/xara.conf")).filter(|p| p.exists())
}

fn with_config(argv: &[String]) -> Result<(Option<PathBuf>, Vec<String>), String> {
    if let Some(m) = argv.iter().find(|k| cli::is_meta(k)) {
        return Ok((None, vec![m.clone()]));
    }
    let (mut path, mut rest) = (None, Vec::new());
    let mut it = argv.iter();
    while let Some(k) = it.next() {
        if k != "--config" {
            rest.push(k.clone());
        } else if path.replace(PathBuf::from(it.next().ok_or("--config needs a value")?)).is_some() {
            return Err("--config given twice".into());
        }
    }
    let Some(path) = path.or_else(default_config) else {
        return Ok((None, rest));
    };
    let mut all = config_args(&path)?;
    all.extend(rest);
    Ok((Some(path), all))
}

fn lead_samples(a: &Args) -> usize {
    a.timing.lead.as_millis() as usize * (audio::RATE / 1000) as usize
}

fn state_dir(a: &Args) -> Option<PathBuf> {
    let env = std::env::var("STATE_DIRECTORY").ok();
    let dir = a.state_dir.clone().or_else(|| env.and_then(|e| e.split(':').next().map(String::from)))?;
    (!dir.is_empty()).then(|| PathBuf::from(dir))
}

fn make_state_dir(d: &Path) -> Result<Option<String>, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let err = |e: std::io::Error| format!("{}: {e}", d.display());
    if !d.exists() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(d).map_err(err)?;
        return Ok(None);
    }
    let mode = std::fs::metadata(d).map_err(err)?.permissions().mode();
    Ok((mode & 0o022 != 0).then(|| format!("{} is writable by other users (mode {:o})", d.display(), mode & 0o777)))
}

fn ui_token(a: &Args, dir: Option<&Path>, inherited: Option<String>) -> std::io::Result<Option<String>> {
    if !a.ui_control {
        return Ok(None);
    }
    if let Some(t) = a.ui_token.clone().or(inherited) {
        return Ok(Some(t));
    }
    match dir.map(stored_token) {
        Some(Ok(t)) => Ok(Some(t)),
        Some(Err(e)) => {
            say!("UI token not saved ({e}); it changes when xara starts again");
            new_token().map(Some)
        }
        None => new_token().map(Some),
    }
}

fn check_saved_playback(a: &Args, saved: &Pairs) -> Result<(), String> {
    if !saved.iter().any(|(k, _)| k == "playback") {
        return Ok(());
    }
    audio::check_playback(a.audio, &a.playback, a.radio).map_err(|e| e.to_string())
}

fn with_saved(argv: &[String], saved: &Pairs, fm: bool) -> Result<Args, String> {
    match (fm, saved.iter().find(|(k, _)| k == "fm" || k == "hf")) {
        (false, Some((k, _))) if k == "fm" => return Err("HF or FM is chosen on the command line".into()),
        (true, Some((k, _))) if k == "hf" => return Err("hf is for HF; this station runs FM".into()),
        _ => {}
    }
    let a = build(argv, saved)?;
    if a.radio && saved.iter().any(|(k, _)| k == "playback") {
        return Err("with --radio the playback device is chosen on the command line".into());
    }
    Ok(a)
}

fn overridden(base: &Args, a: &Args, saved: &Pairs) {
    let (was, now) = (effective(base), effective(a));
    for (k, v) in now.iter().filter(|kv| saved.iter().any(|(s, _)| *s == kv.0) && !was.contains(kv)) {
        if let Some((_, old)) = was.iter().find(|(w, _)| w == k) {
            say!("saved setting {k} = {v} overrides {old}");
        }
    }
}

fn effective(a: &Args) -> Pairs {
    let mode = match a.fm {
        Some(bw) => ("fm", format!("{bw:?}").to_ascii_lowercase()),
        None => ("hf", a.bw.hz().to_string()),
    };
    [
        ("call", a.call.clone()),
        mode,
        ("drive-db", a.drive_db.to_string()),
        ("capture", a.capture.clone()),
        ("playback", a.playback.clone()),
        ("ptt-lead-ms", a.timing.lead.as_millis().to_string()),
        ("ptt-tail-ms", a.timing.tail.as_millis().to_string()),
        ("audio-latency-ms", a.audio_latency.as_millis().to_string()),
        ("registered", yes(a.registered)),
        ("monitor", yes(a.monitor)),
        ("monitor-frames", yes(a.monitor_frames)),
        ("monitor-call", a.heard.join(" ")),
        ("debug-log", yes(a.debug_log.is_some())),
        ("record", yes(a.record.is_some())),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn yes(on: bool) -> String {
    (if on { "yes" } else { "no" }).into()
}

fn devices(b: Backend) -> std::io::Result<Devices> {
    let (capture, playback) = audio::list(b)?;
    let d = |v: Vec<(String, String)>| v.into_iter().map(|(name, desc)| Device { name, desc }).collect();
    Ok(Devices { backend: b.name(), capture: d(capture), playback: d(playback) })
}

fn list_devices(argv: &[String]) -> Result<Done, String> {
    let b = match argv.iter().position(|k| k == "--audio") {
        Some(i) => argv.get(i + 1).ok_or("--audio needs a value")?.parse()?,
        None => Backend::default(),
    };
    let d = devices(b).map_err(|e| format!("listing {} devices: {e}", b.name()))?;
    for (what, list) in [("capture", d.capture), ("playback", d.playback)] {
        println!("{} {what}:", d.backend);
        for Device { name, desc } in list {
            println!("  {name}{}", if desc.is_empty() || desc == name { String::new() } else { format!("  ({desc})") });
        }
    }
    Ok(Done::Exit(0))
}

fn persist_drive(dir: &std::path::Path, db: f64) {
    let r = settings::load(dir).and_then(|saved| {
        let next = settings::merge(&saved, &vec![("drive-db".to_string(), db.to_string())]);
        settings::store(dir, &next).map_err(|e| e.to_string())
    });
    match r {
        Ok(()) => say!("drive level {db} dB saved"),
        Err(e) => say!("drive level not saved: {e}"),
    }
}

struct Tick {
    keyed: bool,
    tuning: bool,
    restart: bool,
}

struct Pumped {
    audio: std::io::Result<()>,
    ptt: std::io::Result<()>,
}

struct Outcome {
    drive_db: f64,
    restart: bool,
    pumped: Pumped,
}

fn run(
    a: &Args,
    io: Box<dyn Audio>,
    settings: Arc<Settings>,
    stop: &Arc<AtomicBool>,
    rec: Option<&mut Recorder>,
) -> std::io::Result<Outcome> {
    // The PTT lead is silence xara inserts and then allows for; the audio latency is only allowed for.
    let latency = a.audio_latency.as_millis() as usize * (audio::RATE / 1000) as usize;
    let opts = Options { drive_db: a.drive_db, tx_latency: (lead_samples(a) + latency) as i64 };
    let host = Host::bind(&a.addr, a.port)?;
    let mode = match a.fm {
        Some(bw) => format!("FM {bw:?}"),
        None => format!("{} Hz", a.bw.hz()),
    };
    xara_host::log::line(format_args!(
        "xara {}: {} {mode}, cmd {} data {}, {} -> {}",
        env!("CARGO_PKG_VERSION"),
        a.call,
        host.cmd_addr,
        host.data_addr,
        a.capture,
        a.playback
    ));
    match a.fm {
        Some(bw) => {
            let mut cfg = fm::Config::new(bw, &a.call);
            cfg.host.registered = a.registered;
            let mut station = station::fm::Station::new(cfg, opts);
            station.set_digipeater(&a.digi);
            if !a.digi.is_empty() {
                say!("FM digipeater {}", a.digi);
            }
            serve(a, io, Tnc::new(station, host), settings, stop, rec)
        }
        None => {
            let mut cfg = hf::Config {
                offer_level: a.offer_level.map(|n| Record::clamped(n, Record::MAX)),
                ..hf::Config::new(a.bw, &a.call)
            };
            cfg.host.registered = a.registered;
            let mut station = station::hf::Station::new(cfg, opts);
            station.set_persistence(a.persistence);
            serve(a, io, Tnc::new(station, host), settings, stop, rec)
        }
    }
}

fn serve<M: Modem>(
    a: &Args,
    mut io: Box<dyn Audio>,
    mut tnc: Tnc<M>,
    settings: Arc<Settings>,
    stop: &Arc<AtomicBool>,
    rec: Option<&mut Recorder>,
) -> std::io::Result<Outcome> {
    if let Some(p) = a.kiss_port {
        let k = KissServer::bind(&a.addr, p)?;
        say!("KISS {}", k.addr);
        tnc.set_kiss(k);
    }
    tnc.set_echo(!a.quiet || a.debug_log.is_some());
    tnc.set_level_log(a.debug_log.is_some());
    let _ui = match a.ui.port {
        Some(p) => {
            let (t, feed) = telemetry::channel(EVENT_QUEUE);
            let (mode, span_hz) = if a.fm.is_some() { ("fm", crate::FM_SPAN_HZ) } else { ("hf", crate::HF_SPAN_HZ) };
            let info = Info { version: env!("CARGO_PKG_VERSION"), mode, span_hz };
            let token = a.ui_token.clone();
            let access = Access {
                control: a.ui_control,
                token: token.clone(),
                hosts: a.ui.hosts.clone(),
                settings: Some(settings),
            };
            let ui = UiServer::bind(a.ui.addr(), p, feed, info, access)?;
            say!("UI http://{}/", ui.addr);
            if let (true, Some(t)) = (a.ui_control, token) {
                say!("UI controls enabled at http://{}/?token={t}: Tune keys the transmitter", ui.addr);
            }
            tnc.set_telemetry(t);
            Some(ui)
        }
        None => None,
    };
    // Without an app to show it, received text goes to stdout.
    if a.monitor && !crate::rxtext::has_sink() {
        crate::rxtext::set_sink(|s| print!("{s}"));
    }
    let passive = match a.monitor {
        true => Some(crate::rxtext::Passive::spawn(
            a.bw,
            &[&[a.call.clone()][..], &a.heard][..].concat(),
            a.monitor_frames,
        )?),
        false => None,
    };
    let pumped = pump(a, io.as_mut(), stop, rec, |x, out, forced| {
        if let Some(p) = &passive {
            p.feed(x);
        }
        tnc.process(x, out);
        tnc.force_ptt_off(forced);
        Tick { keyed: tnc.keyed(), tuning: tnc.station().tuning(), restart: tnc.restart_requested() }
    });
    Ok(Outcome { drive_db: tnc.station().options().drive_db, restart: tnc.restart_requested(), pumped })
}

fn pump(
    a: &Args,
    io: &mut dyn Audio,
    stop: &Arc<AtomicBool>,
    mut rec: Option<&mut Recorder>,
    mut process: impl FnMut(&[f64], &mut [f64], bool) -> Tick,
) -> Pumped {
    let mut ptt_result = Ok(());
    let mut ptt = match &a.ptt {
        Some(spec) => {
            let dev = match spec.open() {
                Ok(d) => d,
                Err(e) => return Pumped { audio: Err(e), ptt: Ok(()) },
            };
            say!(
                "RADIO MODE: transmitting into {:?}, PTT {}, lead {} ms, tail {:?}, watchdog {:?}",
                a.playback,
                dev.describe(),
                a.timing.lead.as_millis(),
                a.timing.tail,
                a.timing.max_on
            );
            Some(PttCtl::spawn(dev, a.timing, stop.clone()))
        }
        None => None,
    };
    let mut tx = audio::TxAudio::new(lead_samples(a));
    let (mut x, mut out) = (vec![0.0; audio::BLOCK], vec![0.0; audio::BLOCK]);
    let (mut rx, mut sent) = (vec![0i16; audio::BLOCK], vec![0i16; audio::BLOCK]);
    audio::listen::clear();
    let audio = (|| {
        while !stop.load(Ordering::SeqCst) {
            io.read(&mut rx)?;
            x.iter_mut().zip(&rx).for_each(|(v, &s)| *v = audio::sample(s));
            let Tick { keyed, tuning, restart } = process(&x, &mut out, ptt.as_ref().is_some_and(PttCtl::forced_off));
            if keyed {
                if let Err(e) = ptt.as_mut().map_or(Ok(()), |p| p.set(true)) {
                    ptt_result = Err(e);
                    break;
                }
            }
            let forced = ptt.as_ref().is_some_and(PttCtl::forced_off);
            tx.process(&mut out, forced);
            sent.iter_mut().zip(&out).for_each(|(s, &v)| *s = audio::quantize(v));
            io.write(&sent)?;
            audio::listen::push(&rx, &sent);
            if let Some(r) = rec.as_deref_mut() {
                r.push(&rx, &sent, record::ptt(keyed, forced, tuning));
            }
            if !keyed {
                if let Err(e) = ptt.as_mut().map_or(Ok(()), |p| p.set(false)) {
                    ptt_result = Err(e);
                    break;
                }
            }
            if restart {
                break;
            }
        }
        Ok(())
    })();
    if let Some(mut p) = ptt {
        let done = p.finish();
        if ptt_result.is_ok() {
            ptt_result = done;
        }
    }
    Pumped { audio, ptt: ptt_result }
}

fn open_audio(
    argv: &[String],
    a: Args,
    applied: Option<&Pairs>,
    error: &mut Option<String>,
) -> Result<(Args, Box<dyn Audio>), std::io::Error> {
    let devices = |k: &str| k == "capture" || k == "playback";
    audio::trusdx::set_speaker(a.trusdx_speaker);
    match audio::open(a.audio, &a.capture, &a.playback, a.radio) {
        Ok(io) => Ok((a, io)),
        Err(e) if applied.is_some_and(|s| s.iter().any(|(k, _)| devices(k))) => {
            say!("saved audio devices failed ({e}); using the command line's");
            *error = Some(format!("saved audio devices failed: {e}"));
            let rest: Pairs = applied.into_iter().flatten().filter(|(k, _)| !devices(k)).cloned().collect();
            let base = with_saved(argv, &rest, a.fm.is_some()).map_err(std::io::Error::other)?;
            audio::trusdx::set_speaker(base.trusdx_speaker);
            let io = audio::open(base.audio, &base.capture, &base.playback, base.radio)?;
            Ok((base, io))
        }
        Err(e) => Err(e),
    }
}

fn restart(argv: &[String], token: Option<&str>, record_part: Option<u32>) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let err = match std::env::current_exe() {
        Ok(exe) => {
            let mut c = std::process::Command::new(exe);
            if let Some(t) = token {
                c.env(TOKEN_ENV, t);
            }
            if let Some(n) = record_part {
                c.env(RECORD_PART_ENV, n.to_string());
            }
            if let Some(a0) = std::env::args_os().next() {
                c.arg0(a0);
            }
            c.args(argv).exec()
        }
        Err(e) => e,
    };
    say!("restart failed: {err}");
    ExitCode::from(EXIT_RESTART_FAILED)
}

/// The `xara` command: runs one session, then re-executes itself when the web UI asks for a restart.
pub fn main() -> ExitCode {
    let inherited = std::env::var(TOKEN_ENV).ok().filter(|t| valid_token(t));
    let part: u32 = std::env::var(RECORD_PART_ENV).ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    unsafe {
        std::env::remove_var(TOKEN_ENV);
        std::env::remove_var(RECORD_PART_ENV);
    }
    let cmdline: Vec<String> = std::env::args().skip(1).collect();
    let stop = Arc::new(AtomicBool::new(false));
    let sigs = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];
    let signaled = match signals::trap(&sigs, stop.clone(), || {
        crate::ptt::wait_released(SECOND_SIGNAL_GRACE);
    }) {
        Ok(s) => s,
        Err(e) => {
            say!("signals: {e}");
            return ExitCode::FAILURE;
        }
    };
    match session(&cmdline, &stop, inherited, part) {
        Ok(_) if signaled.load(Ordering::SeqCst) => ExitCode::from(130),
        Ok(Done::Exit(c)) => ExitCode::from(c),
        Ok(Done::Restart { token, part }) => {
            say!("restarting");
            restart(&cmdline, token.as_deref(), part)
        }
        Err(e) => {
            say!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs xara inside another program (such as an iOS app) until `stop` is set, restarting in
/// place when the web UI asks. Signals are left to the host program. Returns the exit status
/// the command would have had.
pub fn run_embedded(cmdline: &[String], stop: &Arc<AtomicBool>) -> u8 {
    let (mut token, mut part) = (None, 0);
    loop {
        match session(cmdline, stop, token.take(), part) {
            Ok(_) if stop.load(Ordering::SeqCst) => return 0,
            Ok(Done::Exit(c)) => return c,
            Ok(Done::Restart { token: t, part: p }) => {
                say!("restarting");
                (token, part) = (t, p.unwrap_or(part));
            }
            Err(e) => {
                say!("{e}");
                return 1;
            }
        }
    }
}

enum Done {
    Exit(u8),
    Restart { token: Option<String>, part: Option<u32> },
}

fn session(cmdline: &[String], stop: &Arc<AtomicBool>, inherited: Option<String>, part: u32) -> Result<Done, String> {
    let (config, argv) = match with_config(cmdline) {
        Ok(c) => c,
        Err(e) => {
            xara_host::log::line(format_args!("{e}"));
            return Ok(Done::Exit(2));
        }
    };
    if argv.iter().any(|k| k == "--list-devices") {
        return list_devices(&argv);
    }
    let base = match parse(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return Ok(Done::Exit(0)),
        Err(e) => {
            xara_host::log::line(format_args!("{e}\n{}\nsee {} --help", USAGE.synopsis, USAGE.name));
            return Ok(Done::Exit(2));
        }
    };
    crate::log::init("xara", base.quiet);
    if let Some(p) = &config {
        say!("config {}", p.display());
    }
    let dir = state_dir(&base);
    let mut error = None;
    let saved = match &dir {
        Some(d) => make_state_dir(d)
            .map(|w| {
                if let Some(w) = w {
                    say!("warning: {w}");
                }
            })
            .and_then(|()| settings::load(d))
            .unwrap_or_else(|e| {
                error = Some(e);
                Pairs::new()
            }),
        None => Pairs::new(),
    };
    let fm = base.fm.is_some();
    let (a, applied) = match saved.is_empty() {
        true => (base, false),
        false => match with_saved(&argv, &saved, fm) {
            Ok(a) => {
                overridden(&base, &a, &saved);
                (a, true)
            }
            Err(e) => {
                error = Some(format!("saved settings ignored: {e}"));
                (base, false)
            }
        },
    };
    if let Some(e) = &error {
        say!("{e}");
    }
    let (mut a, io) = open_audio(&argv, a, applied.then_some(&saved), &mut error).map_err(|e| e.to_string())?;
    if let Some(d) = &dir {
        say!("settings in {}", d.join(settings::FILE).display());
    }
    a.ui_token = ui_token(&a, dir.as_deref(), inherited).map_err(|e| format!("UI token: {e}"))?;
    let (backend, check) = (a.audio, argv.clone());
    let settings = Arc::new(Settings::new(Init {
        dir: dir.clone(),
        values: effective(&a),
        saved,
        error,
        validate: Box::new(move |kv| with_saved(&check, kv, fm).and_then(|a| check_saved_playback(&a, kv))),
        list: Box::new(move || devices(backend)),
    }));
    if let Err(e) = xara_host::log::set_file(a.debug_log.as_deref()) {
        say!("--debug-log {}: {e}", a.debug_log.as_deref().unwrap_or(Path::new("")).display());
    } else if let Some(p) = &a.debug_log {
        say!("debug log {} (call {}, capture {}, playback {})", p.display(), a.call, a.capture, a.playback);
    }
    let mut rec = match &a.record {
        Some(p) => {
            let p = record::part(p, part);
            let mode = match a.fm {
                Some(bw) => format!("FM {bw:?}"),
                None => format!("HF {}", a.bw.hz()),
            };
            let r = Recorder::open(&p, &mode).map_err(|e| format!("--record {}: {e}", p.display()))?;
            say!("recording rx, tx and PTT to {}", p.display());
            Some(r)
        }
        None => None,
    };
    let o = run(&a, io, settings, stop, rec.as_mut()).unwrap_or_else(|e| Outcome {
        drive_db: a.drive_db,
        restart: false,
        pumped: Pumped { audio: Err(e), ptt: Ok(()) },
    });
    drop(rec);
    let Pumped { audio, ptt } = o.pumped;
    if let (Some(d), Ok(())) = (&dir, &audio) {
        if o.drive_db != a.drive_db {
            persist_drive(d, o.drive_db);
        }
    }
    if let Err(e) = &ptt {
        say!("{e}");
    }
    if crate::ptt::panicked() {
        say!("stopped after a panic");
        return Ok(Done::Exit(101));
    }
    if ptt.is_err() {
        return Ok(Done::Exit(1));
    }
    if stop.load(Ordering::SeqCst) {
        return Ok(Done::Exit(0));
    }
    audio.map_err(|e| e.to_string())?;
    match o.restart {
        true => Ok(Done::Restart { token: a.ui_token.clone(), part: a.record.as_ref().map(|_| part + 1) }),
        false => Ok(Done::Exit(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn kv(p: &[(&str, &str)]) -> Pairs {
        p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn saved_settings_cannot_move_radio_playback() {
        let radio = argv("--call N2CALL --capture c --playback radio_pcm --radio --ptt vox");
        assert!(with_saved(&radio, &kv(&[("playback", "other_card")]), false).is_err_and(|e| e.contains("--radio")));
        let a = with_saved(&radio, &kv(&[("call", "N1CALL"), ("capture", "c2")]), false).unwrap();
        assert_eq!((a.call.as_str(), a.capture.as_str(), a.playback.as_str()), ("N1CALL", "c2", "radio_pcm"));
        let null = argv("--call N2CALL --capture c --playback xara_tx");
        assert_eq!(with_saved(&null, &kv(&[("playback", "other_tx")]), false).unwrap().playback, "other_tx");
        assert!(!with_saved(&null, &kv(&[("playback", "--radio")]), false).unwrap().radio);
        assert!(with_saved(&null, &kv(&[("radio", "")]), false).is_err_and(|e| e.contains("unknown setting")));
        assert!(with_saved(&null, &kv(&[("ptt", "vox")]), false).is_err_and(|e| e.contains("unknown setting")));
    }

    #[test]
    fn saved_values_are_values_not_options() {
        let null = argv("--call N2CALL --capture c --playback xara_tx");
        assert_eq!(with_saved(&null, &kv(&[("capture", "-h")]), false).unwrap().capture, "-h");
        assert!(with_saved(&null, &kv(&[("call", "--help")]), false).is_err_and(|e| e.contains("bad call")));
        assert!(with_saved(&argv("--call N2CALL --capture c --playback p -h"), &Pairs::new(), false).is_err());
    }

    #[test]
    fn a_real_call_is_required() {
        assert_eq!(parse(&argv("--capture c --playback p")).err().as_deref(), Some("--call is required"));
        for c in ["N0CALL", "n0call", "N0CALL-7"] {
            let e = parse(&argv(&format!("--call {c} --capture c --playback p"))).err();
            assert!(e.is_some_and(|e| e.contains("N0CALL is not a callsign")), "{c}");
        }
        let a = argv("--call N2CALL --capture c --playback p");
        assert!(with_saved(&a, &kv(&[("call", "N0CALL")]), false).is_err_and(|e| e.contains("N0CALL")));
        assert_eq!(parse(&a).unwrap().unwrap().call, "N2CALL");
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xara-main-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn conf(name: &str, text: &str) -> PathBuf {
        let d = scratch(name);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("xara.conf");
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn config_lines_become_options_the_command_line_overrides() {
        let p = conf(
            "conf-ok",
            "# station\ncall = N1CALL\n\ncapture = plughw:CARD=Device,DEV=0\nplayback=null\nradio\nptt = vox\n\
             quiet = no\nui-port = 8080\nui-host = a.lan\nui-host = b.lan\nui-control = yes\n",
        );
        let want = "--call N1CALL --capture plughw:CARD=Device,DEV=0 --playback null --radio --ptt vox \
                    --ui-port 8080 --ui-host a.lan --ui-host b.lan --ui-control";
        assert_eq!(config_args(&p).unwrap(), argv(want));
        let (path, all) = with_config(&argv(&format!("--call N2CALL --config {}", p.display()))).unwrap();
        assert_eq!(path.as_deref(), Some(p.as_path()));
        let a = parse(&all).unwrap().unwrap();
        assert_eq!((a.call.as_str(), a.radio, a.quiet, a.ui.port), ("N2CALL", true, false, Some(8080)));
        assert_eq!(a.ui.hosts, ["a.lan", "b.lan"]);
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn audio_latency_is_bounded_and_can_be_saved() {
        let a = parse(&argv("--call N2CALL --capture c --playback p --audio-latency-ms 300")).unwrap().unwrap();
        assert_eq!(a.audio_latency, Duration::from_millis(300));
        assert!(parse(&argv("--call N2CALL --capture c --playback p --audio-latency-ms 1001")).is_err());
        let saved = vec![("audio-latency-ms".to_string(), "250".to_string())];
        let b = with_saved(&argv("--call N2CALL --capture c --playback p"), &saved, false).unwrap();
        assert_eq!(b.audio_latency, Duration::from_millis(250));
    }

    #[test]
    fn saved_switches_turn_options_on_and_off() {
        let base = argv("--call N2CALL --capture c --playback p --monitor --monitor-call W1AW --registered");
        let a =
            with_saved(&base, &kv(&[("monitor-frames", "yes"), ("monitor-call", "N1CALL K1ABC-7")]), false).unwrap();
        assert!(a.registered && a.monitor && a.monitor_frames);
        assert_eq!(a.heard, ["N1CALL", "K1ABC-7"]);
        let b = with_saved(&base, &kv(&[("registered", "no"), ("monitor", "no")]), false).unwrap();
        assert!(!b.registered && !b.monitor);
        assert!(with_saved(&base, &kv(&[("monitor", "no"), ("monitor-frames", "yes")]), false).is_err());
        assert!(with_saved(&base, &kv(&[("debug-log", "yes")]), false).is_err_and(|e| e.contains("state directory")));
        let dir = argv("--call N2CALL --capture c --playback p --state-dir /var/lib/xara/x --record /tmp/a.flac");
        let c = with_saved(&dir, &kv(&[("debug-log", "yes"), ("record", "no")]), false).unwrap();
        assert_eq!(c.debug_log.as_deref(), Some(Path::new("/var/lib/xara/x/debug.log")));
        assert_eq!(c.record, None);
        let r = with_saved(&dir, &kv(&[("record", "yes")]), false).unwrap().record.unwrap();
        assert!(r.starts_with("/var/lib/xara/x") && r.extension().is_some_and(|e| e == "flac"));
        assert_eq!(effective(&c).iter().find(|(k, _)| k == "debug-log").unwrap().1, "yes");
    }

    #[test]
    fn debug_log_takes_a_path() {
        let a = parse(&argv("--call N2CALL --capture c --playback p --debug-log /tmp/x.log")).unwrap().unwrap();
        assert_eq!(a.debug_log.as_deref(), Some(Path::new("/tmp/x.log")));
        assert!(parse(&argv("--call N2CALL --capture c --playback p --debug-log")).is_err());
    }

    #[test]
    fn record_takes_a_path_from_the_command_line_or_a_config() {
        let a = parse(&argv("--call N2CALL --capture c --playback p --record /tmp/x.wav")).unwrap().unwrap();
        assert_eq!(a.record.as_deref(), Some(Path::new("/tmp/x.wav")));
        assert!(parse(&argv("--call N2CALL --capture c --playback p --record")).is_err());
        let p = conf("conf-record", "record = cap.flac\n");
        assert_eq!(config_args(&p).unwrap(), argv("--record cap.flac"));
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn config_errors_name_the_file_and_line() {
        for (text, want) in [
            ("call = N0CALL\ncolour = red\n", "line 2: unknown option --colour"),
            ("call\n", "line 1: call needs a value"),
            ("radio = maybe\n", "line 1: radio is a switch"),
            ("drive-db = loud\n", "line 1: --drive-db:"),
            ("config = other.conf\n", "line 1: config cannot be set"),
            ("call N0CALL\n", "line 1: expected key = value"),
        ] {
            let p = conf("conf-bad", text);
            let e = config_args(&p).unwrap_err();
            assert!(e.starts_with(&format!("{}: {want}", p.display())), "{text:?}: {e}");
        }
        scratch("conf-bad");
        assert!(with_config(&argv("--config /nonexistent/xara.conf")).is_err_and(|e| e.contains("/nonexistent")));
        assert_eq!(with_config(&argv("--config a --config b")), Err("--config given twice".into()));
        assert_eq!(with_config(&argv("--config /nonexistent/xara.conf -h")), Ok((None, argv("-h"))));
    }

    #[test]
    fn state_dir_is_created_private_and_a_shared_one_warned_about() {
        use std::os::unix::fs::PermissionsExt;
        let top = scratch("state");
        let d = top.join("a/b");
        assert_eq!(make_state_dir(&d), Ok(None));
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(&d), mode(&top.join("a"))), (0o700, 0o700));
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(make_state_dir(&d), Ok(None));
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(make_state_dir(&d).unwrap().is_some_and(|w| w.contains("775")));
        assert_eq!(mode(&d), 0o775);
        std::fs::remove_dir_all(&top).unwrap();
    }

    #[test]
    fn controls_always_get_a_token() {
        let base = "--call N2CALL --capture c --playback p --ui-port 8080 --ui-control";
        let a = parse(&argv(base)).unwrap().unwrap();
        let t = ui_token(&a, None, None).unwrap().unwrap();
        assert!(valid_token(&t) && ui_token(&a, None, None).unwrap() != Some(t));
        assert_eq!(ui_token(&a, None, Some("from-restart-1".into())).unwrap().as_deref(), Some("from-restart-1"));
        let given = parse(&argv(&format!("{base} --ui-token given-token-1"))).unwrap().unwrap();
        assert_eq!(ui_token(&given, None, Some("x".repeat(9))).unwrap().as_deref(), Some("given-token-1"));
        let dir = scratch("token");
        make_state_dir(&dir).unwrap();
        let kept = ui_token(&a, Some(&dir), None).unwrap();
        assert!(kept.is_some() && ui_token(&a, Some(&dir), None).unwrap() == kept);
        let view = parse(&argv("--call N2CALL --capture c --playback p --ui-port 8080")).unwrap().unwrap();
        assert_eq!(ui_token(&view, Some(&dir), None).unwrap(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(alsa)]
    #[test]
    fn a_save_is_refused_when_startup_would_refuse_its_playback() {
        let null = argv("--call N2CALL --audio alsa --capture null --playback null");
        let validate = |p: &Pairs| with_saved(&null, p, false).and_then(|a| check_saved_playback(&a, p));
        for bad in ["hw:1,0", "plughw:CARD=Device,DEV=0", "default", "pulse"] {
            assert!(validate(&kv(&[("playback", bad)])).is_err_and(|e| e.contains("--radio")), "{bad}");
        }
        let pipe = validate(&kv(&[("playback", "file:'|touch /nonexistent/x'")]));
        assert!(pipe.is_err_and(|e| e.contains("runs a command")));
        assert_eq!(validate(&kv(&[("playback", "null"), ("call", "N1CALL")])), Ok(()));
        assert_eq!(validate(&kv(&[("capture", "hw:1,0")])), Ok(()));
    }

    #[cfg(alsa)]
    #[test]
    fn a_refused_saved_device_keeps_the_other_saved_values() {
        let null = argv("--call N2CALL --audio alsa --capture null --playback null --drive-db -5");
        let saved = kv(&[("call", "N1CALL"), ("drive-db", "-12"), ("playback", "hw:1,0")]);
        let a = with_saved(&null, &saved, false).unwrap();
        let mut error = None;
        let (a, _io) = open_audio(&null, a, Some(&saved), &mut error).unwrap();
        assert_eq!((a.call.as_str(), a.drive_db, a.playback.as_str()), ("N1CALL", -12.0, "null"));
        assert!(error.is_some_and(|e| e.contains("saved audio devices failed")));
    }
}
