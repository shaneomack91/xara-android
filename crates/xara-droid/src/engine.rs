#[cfg(unix)]
mod unix {
    use std::net::SocketAddr;
    use xara::audio::{self, TxAudio};
    use xara::station::{self, Modem, Options};
    use xara::tnc::Tnc;
    use xara_phy::fm::level::FmBandwidth;
    use xara_phy::hf::level::Bandwidth;

    enum Which {
        Hf(Box<Tnc<station::hf::Station>>),
        Fm(Box<Tnc<station::fm::Station>>),
    }

    pub struct Running {
        which: Which,
        delay: TxAudio,
        x: Vec<f64>,
        y: Vec<f64>,
        pub line: String,
    }

    pub struct StartReq {
        pub call: String,
        pub mode: i32,
        pub addr: String,
        pub cmd_port: u16,
        pub kiss_port: u16,
        pub lead_ms: u32,
    }

    impl Running {
        pub fn start(req: &StartReq) -> Result<Self, String> {
            if req.lead_ms > 1000 {
                return Err("PTT lead must be <= 1000 ms".into());
            }
            if req.cmd_port == 0 || req.cmd_port == u16::MAX {
                return Err("command port must be 1-65534 (data port is the next one)".into());
            }
            if req.kiss_port == req.cmd_port || req.kiss_port == req.cmd_port + 1 {
                return Err("KISS port collides with the VARA command or data port".into());
            }
            let call = xara_host::command::valid_call(&req.call).ok_or_else(|| format!("bad call {}", req.call))?;
            if call.split('-').next().is_some_and(|c| c.eq_ignore_ascii_case("N0CALL")) {
                return Err("N0CALL is not a callsign".into());
            }
            xara::log::init("xara", true);
            let lead = req.lead_ms as usize * (audio::RATE / 1000) as usize;
            let opts = Options { drive_db: station::DEFAULT_DRIVE_DB, tx_latency: lead as i64 };
            let host = xara_host::server::Host::bind(&req.addr, req.cmd_port).map_err(|e| e.to_string())?;
            let (cmd, data) = (host.cmd_addr, host.data_addr);
            let (which, version, kiss) = match req.mode {
                0..=2 => {
                    let bw = match req.mode {
                        0 => Bandwidth::Hz500,
                        2 => Bandwidth::Hz2750,
                        _ => Bandwidth::Hz2300,
                    };
                    let mut cfg = xara_arq::hf::Config::new(bw, &call);
                    cfg.host.registered = crate::registered();
                    let mut station = station::hf::Station::new(cfg, opts);
                    station.set_persistence(xara_arq::hf::broadcast::DEFAULT_PERSISTENCE);
                    let mut tnc = Tnc::new(station, host);
                    tnc.set_echo(false);
                    let kiss = attach_kiss(&mut tnc, &req.addr, req.kiss_port)?;
                    (Which::Hf(Box::new(tnc)), <station::hf::Station as Modem>::VERSION, kiss)
                }
                3 | 4 => {
                    let bw = if req.mode == 4 { FmBandwidth::Wide } else { FmBandwidth::Narrow };
                    let mut cfg = xara_arq::fm::Config::new(bw, &call);
                    cfg.host.registered = crate::registered();
                    let mut station = station::fm::Station::new(cfg, opts);
                    station.set_digipeater("");
                    let mut tnc = Tnc::new(station, host);
                    tnc.set_echo(false);
                    let kiss = attach_kiss(&mut tnc, &req.addr, req.kiss_port)?;
                    (Which::Fm(Box::new(tnc)), <station::fm::Station as Modem>::VERSION, kiss)
                }
                _ => return Err("mode must be HF 500/2300/2750 or FM narrow/wide".into()),
            };
            let line = format!("{version}\ncmd {cmd}\ndata {data}\nkiss {}", kiss_text(kiss));
            Ok(Self { which, delay: TxAudio::new(lead), x: Vec::new(), y: Vec::new(), line })
        }

        pub fn process(&mut self, rx: &[i16], tx: &mut [i16], force_off: bool) -> bool {
            let n = rx.len().min(tx.len());
            if self.x.len() != n {
                self.x.resize(n, 0.0);
                self.y.resize(n, 0.0);
            }
            for (o, &s) in self.x.iter_mut().zip(rx) {
                *o = audio::sample(s);
            }
            let keyed = match &mut self.which {
                Which::Hf(t) => step(t, &self.x, &mut self.y, force_off),
                Which::Fm(t) => step(t, &self.x, &mut self.y, force_off),
            };
            self.delay.process(&mut self.y[..n], force_off);
            for (o, &s) in tx.iter_mut().zip(self.y.iter().take(n)) {
                *o = audio::quantize(s);
            }
            if tx.len() > n {
                tx[n..].fill(0);
            }
            keyed
        }
    }

    fn step<M: Modem>(tnc: &mut Tnc<M>, x: &[f64], y: &mut [f64], force_off: bool) -> bool {
        tnc.process(x, y);
        tnc.force_ptt_off(force_off);
        tnc.keyed()
    }

    fn attach_kiss<M: xara::station::Modem>(
        tnc: &mut Tnc<M>,
        addr: &str,
        port: u16,
    ) -> Result<Option<SocketAddr>, String> {
        if port == 0 {
            return Ok(None);
        }
        let kiss = xara_host::kiss::KissServer::bind(addr, port).map_err(|e| e.to_string())?;
        let bound = kiss.addr;
        tnc.set_kiss(kiss);
        Ok(Some(bound))
    }

    fn kiss_text(addr: Option<SocketAddr>) -> String {
        addr.map(|a| a.to_string()).unwrap_or_else(|| "off".into())
    }

    pub fn block() -> usize {
        audio::BLOCK
    }
}

#[cfg(unix)]
pub use unix::{Running, StartReq, block};

#[cfg(not(unix))]
pub struct StartReq {
    pub call: String,
    pub mode: i32,
    pub addr: String,
    pub cmd_port: u16,
    pub kiss_port: u16,
    pub lead_ms: u32,
}

#[cfg(not(unix))]
pub struct Running {
    pub line: String,
}

#[cfg(not(unix))]
impl Running {
    pub fn start(req: &StartReq) -> Result<Self, String> {
        let _ = (&req.call, req.mode, &req.addr, req.cmd_port, req.kiss_port, req.lead_ms);
        Err("the modem engine builds on Android and Linux; this host has the CAT library only".into())
    }

    pub fn process(&mut self, _rx: &[i16], tx: &mut [i16], _force_off: bool) -> bool {
        tx.fill(0);
        false
    }
}

#[cfg(not(unix))]
pub fn block() -> usize {
    480
}
