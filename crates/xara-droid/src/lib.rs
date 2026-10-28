mod app;
mod engine;

pub fn registered() -> bool {
    cfg!(feature = "registered")
}

#[cfg(test)]
mod tests {
    #[test]
    fn registered_speeds_follow_the_compile_flag() {
        assert_eq!(crate::registered(), cfg!(feature = "registered"));
    }
}

#[cfg(feature = "jni-bridge")]
mod jni_export;

#[cfg(all(test, unix))]
mod live {
    use super::app;
    use crate::engine::StartReq;
    use std::io::{ErrorKind, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    struct StopLater;
    impl Drop for StopLater {
        fn drop(&mut self) {
            app::stop();
        }
    }

    fn reserve() -> (u16, u16) {
        for p in (25000..45000).step_by(3) {
            let Ok(a) = TcpListener::bind(("127.0.0.1", p)) else { continue };
            let Ok(b) = TcpListener::bind(("127.0.0.1", p + 1)) else { continue };
            let Ok(c) = TcpListener::bind(("127.0.0.1", p + 2)) else { continue };
            drop((a, b, c));
            return (p, p + 2);
        }
        panic!("no free ports");
    }

    #[test]
    fn vara_and_kiss_ports_accept_a_client() {
        let _stop = StopLater;
        let (cmd, kiss) = reserve();
        let line = app::start(&StartReq {
            call: "W1AW".into(),
            mode: 1,
            addr: "127.0.0.1".into(),
            cmd_port: cmd,
            kiss_port: kiss,
            lead_ms: 0,
        })
        .unwrap();
        assert!(line.contains(&cmd.to_string()), "{line}");
        assert!(line.contains("VARA HF"), "{line}");
        let mut sock = TcpStream::connect(("127.0.0.1", cmd)).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        sock.write_all(b"VERSION\r").unwrap();
        let mut got = String::new();
        let deadline = Instant::now() + Duration::from_secs(8);
        let rx = vec![0i16; app::block_len()];
        let mut tx = vec![0i16; app::block_len()];
        while Instant::now() < deadline && !got.contains("VARA HF") {
            app::process(&rx, &mut tx, false).unwrap();
            let mut buf = [0u8; 1024];
            match sock.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => got.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => panic!("{e} so far {got}"),
            }
        }
        assert!(got.contains("VERSION VARA HF"), "{got}");
        assert!(TcpStream::connect(("127.0.0.1", kiss)).is_ok());
        app::stop();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if TcpListener::bind(("127.0.0.1", cmd)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("command port {cmd} still busy");
    }
}
