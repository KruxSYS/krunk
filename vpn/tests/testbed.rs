//! Integration tests against `tools/vpn-testbed` (WireGuard, OpenVPN UDP + TCP, an inside
//! echo server and DNS). They only run with `KRUNK_VPN_TESTBED=1` and the stack up:
//!
//! ```text
//! docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml up -d --build
//! KRUNK_VPN_TESTBED=1 cargo test -p vpn --test testbed
//! docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml down -v --rmi local
//! ```
//! The tests run one at a time (one test stops and starts the servers).

use krunk_vpn::{import, Credentials, State, Status, Vpn};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

static SERIAL: Mutex<()> = Mutex::new(());

/// `None` (the test is skipped) unless KRUNK_VPN_TESTBED=1.
fn testbed() -> Option<MutexGuard<'static, ()>> {
    if std::env::var("KRUNK_VPN_TESTBED").as_deref() != Ok("1") {
        eprintln!("skipped: set KRUNK_VPN_TESTBED=1 with tools/vpn-testbed up");
        return None;
    }
    Some(SERIAL.lock().unwrap_or_else(|e| e.into_inner()))
}

fn compose(args: &[&str]) -> String {
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/vpn-testbed/compose.yaml");
    let out = Command::new("docker")
        .args(["compose", "-p", "vpn-testbed", "-f", file])
        .args(args)
        .output()
        .expect("docker compose");
    assert!(out.status.success(), "docker compose {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn state_file(service: &str, path: &str) -> String {
    compose(&["exec", "-T", service, "cat", path])
}

#[cfg_attr(not(feature = "openvpn"), allow(dead_code))]
fn creds(password: &str) -> Option<Credentials> {
    Some(Credentials { username: Zeroizing::new("krunk".into()), password: Zeroizing::new(password.into()) })
}

struct Running {
    vpn: Vpn,
    log: Arc<Mutex<Vec<Status>>>,
}

fn start(file: &str, text: &str, creds: Option<Credentials>) -> Running {
    let (profile, _info) = import(file, text).expect("import");
    let log = Arc::new(Mutex::new(Vec::new()));
    let l = log.clone();
    let vpn = Vpn::start(profile, creds, Box::new(|_, _| true), Box::new(move |s| l.lock().unwrap().push(s.clone())))
        .expect("start");
    Running { vpn, log }
}

fn wait_state(r: &Running, want: State, limit: Duration) -> Status {
    let t = Instant::now();
    loop {
        let s = r.vpn.status();
        if s.state == want {
            return s;
        }
        assert!(t.elapsed() < limit, "waited {limit:?} for {want:?}; now {s:?}; seen {:?}", r.log.lock().unwrap());
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Sends `req` to the proxy, returns the whole answer.
fn via_proxy(proxy: SocketAddr, req: &str) -> String {
    let mut c = TcpStream::connect(proxy).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    c.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = c.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

fn get(proxy: SocketAddr, url: &str) -> String {
    let host = url.trim_start_matches("http://").split('/').next().unwrap();
    via_proxy(proxy, &format!("GET {url} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: krunk-test\r\n\r\n"))
}

/// CONNECT, then a plain GET inside the tunnel.
fn connect_get(proxy: SocketAddr, authority: &str) -> String {
    let mut c = TcpStream::connect(proxy).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    c.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes()).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if c.read(&mut b).unwrap_or(0) == 0 {
            return String::from_utf8_lossy(&head).into_owned();
        }
        head.push(b[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 200 "), "{}", String::from_utf8_lossy(&head));
    c.write_all(b"GET / HTTP/1.0\r\nHost: inside\r\n\r\n").unwrap();
    let mut out = Vec::new();
    let _ = c.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// The echo body must name our tunnel address: the request went through the tunnel.
fn assert_inside(answer: &str, status: &Status) {
    let ip = status.tunnel_ip.as_deref().expect("tunnel ip");
    assert!(answer.starts_with("HTTP/1.0 200 OK\r\n"), "{answer}");
    assert!(answer.ends_with(&format!("\r\n\r\nclient={ip}\n")), "{answer}");
}

fn exercise(r: &Running, subnet: &str) {
    let status = wait_state(r, State::Connected, Duration::from_secs(30));
    let ip = status.tunnel_ip.clone().unwrap();
    assert!(ip.starts_with(subnet), "{ip}");
    let proxy = r.vpn.proxy_addr();
    // The name resolves through the tunnel's DNS (10.77.0.53); the echo sees the tunnel IP.
    assert_inside(&get(proxy, "http://inside.test/"), &status);
    let a = get(proxy, "http://10.77.0.10/x?y=1");
    assert_inside(&a, &status);
    // Forwarded requests are one per connection, both ways.
    assert!(a.contains("Connection: close\r\n"), "{a}");
    assert_inside(&connect_get(proxy, "10.77.0.10:80"), &status);
    assert_inside(&connect_get(proxy, "inside.test:80"), &status);
    // Outside the routes: 502, never direct.
    let a = get(proxy, "http://192.0.2.1/");
    assert!(a.starts_with("HTTP/1.1 502 "), "{a}");
    // A name the inside DNS does not know (it has no upstream).
    let a = get(proxy, "http://nope.invalid/");
    assert!(a.starts_with("HTTP/1.1 502 "), "{a}");
    let s = r.vpn.status();
    assert!(s.rx_bytes > 0 && s.tx_bytes > 0, "{s:?}");
}

#[test]
fn wireguard_through_the_proxy() {
    let Some(_g) = testbed() else { return };
    let r = start("wg.conf", &state_file("wg", "/state/wg/client.conf"), None);
    exercise(&r, "10.66.0.");
    r.vpn.stop();
}

#[cfg(feature = "openvpn")]
#[test]
fn openvpn_udp_through_the_proxy() {
    let Some(_g) = testbed() else { return };
    let r = start("udp.ovpn", &state_file("ovpn", "/state/ovpn/client-udp.ovpn"), creds("krunk-test"));
    exercise(&r, "10.88.0.");
    r.vpn.stop();
}

#[cfg(feature = "openvpn")]
#[test]
fn openvpn_tcp_through_the_proxy() {
    let Some(_g) = testbed() else { return };
    let r = start("tcp.ovpn", &state_file("ovpn", "/state/ovpn/client-tcp.ovpn"), creds("krunk-test"));
    exercise(&r, "10.89.0.");
    r.vpn.stop();
}

/// A refused password or certificate gives `failed`, without a retry loop, and the proxy
/// stays closed (503).
#[cfg(feature = "openvpn")]
#[test]
fn openvpn_refusals_fail() {
    let Some(_g) = testbed() else { return };
    for (file, password, needle) in [
        ("/state/ovpn/client-udp.ovpn", "wrong", "password"),
        ("/state/ovpn/client-tcp.ovpn", "wrong", "password"),
        ("/state/ovpn/client-tcp-badca.ovpn", "krunk-test", "certificate"),
    ] {
        let r = start("x.ovpn", &state_file("ovpn", file), creds(password));
        let s = wait_state(&r, State::Failed, Duration::from_secs(30));
        assert!(s.error.as_deref().unwrap_or("").contains(needle), "{file}: {s:?}");
        std::thread::sleep(Duration::from_secs(4));
        let s = r.vpn.status();
        assert_eq!(s.state, State::Failed, "{file}: retried? {:?}", r.log.lock().unwrap());
        let a = get(r.vpn.proxy_addr(), "http://inside.test/");
        assert!(a.starts_with("HTTP/1.1 503 VPN not connected\r\n"), "{a}");
        r.vpn.stop();
    }
    // Without credentials the start itself refuses.
    let (p, info) = import("udp.ovpn", &state_file("ovpn", "/state/ovpn/client-udp.ovpn")).unwrap();
    assert!(info.needs_credentials);
    assert!(matches!(
        Vpn::start(p, None, Box::new(|_, _| true), Box::new(|_| {})),
        Err(krunk_vpn::StartError::NeedsCredentials)
    ));
}

/// Starts the service again even when the test fails half-way.
#[cfg(feature = "openvpn")]
struct Restart(&'static str);

#[cfg(feature = "openvpn")]
impl Drop for Restart {
    fn drop(&mut self) {
        compose(&["start", self.0]);
    }
}

#[cfg(feature = "openvpn")]
fn stop_and_return(r: &Running, service: &'static str, limit_503: Duration) {
    let before = wait_state(r, State::Connected, Duration::from_secs(30));
    let proxy = r.vpn.proxy_addr();
    assert_inside(&get(proxy, "http://inside.test/"), &before);
    let restart = Restart(service);
    compose(&["stop", "-t", "1", service]);
    // Requests make the tunnel notice; then everything is 503 (fail-closed), never direct.
    let t = Instant::now();
    loop {
        let a = get(proxy, "http://10.77.0.10/");
        if a.starts_with("HTTP/1.1 503 VPN not connected\r\n") {
            break;
        }
        assert!(t.elapsed() < limit_503, "no 503 after {limit_503:?}: {a}");
        std::thread::sleep(Duration::from_millis(500));
    }
    let s = r.vpn.status();
    assert_eq!(s.state, State::Reconnecting, "{s:?}");
    assert!(s.error.is_some(), "{s:?}");
    for _ in 0..3 {
        assert!(get(proxy, "http://inside.test/").starts_with("HTTP/1.1 503 "));
        std::thread::sleep(Duration::from_secs(1));
    }
    drop(restart);
    let after = wait_state(r, State::Connected, Duration::from_secs(120));
    assert_inside(&get(proxy, "http://inside.test/"), &after);
    let seen: Vec<State> = r.log.lock().unwrap().iter().map(|s| s.state).collect();
    assert!(seen.contains(&State::Reconnecting), "{seen:?}");
}

#[cfg(feature = "openvpn")]
#[test]
fn server_stopped_gives_503_then_reconnects() {
    let Some(_g) = testbed() else { return };
    let wg = start("wg.conf", &state_file("wg", "/state/wg/client.conf"), None);
    stop_and_return(&wg, "wg", Duration::from_secs(45));
    wg.vpn.stop();
    let ovpn = start("tcp.ovpn", &state_file("ovpn", "/state/ovpn/client-tcp.ovpn"), creds("krunk-test"));
    stop_and_return(&ovpn, "ovpn", Duration::from_secs(45));
    let last = ovpn.log.lock().unwrap().last().cloned().unwrap();
    ovpn.vpn.stop();
    assert_eq!(last.state, State::Connected);
}
