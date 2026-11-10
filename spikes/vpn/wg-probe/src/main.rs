//! M9 spike: boringtun `noise` + smoltcp, no OS adapter.
//!
//! `wg-probe self`   - two in-memory `Tunn`s handshake, then a TCP SYN made by smoltcp is
//!                     encapsulated by the client and decapsulated by the "server".
//! `wg-probe connect <endpoint> <server_pub_b64> <client_priv_b64> <client_ip> <inside_ip:port>`
//!                   - real handshake over UDP, then `GET /` to the inside server through
//!                     smoltcp TCP inside the tunnel; prints the HTTP response.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant as StdInstant};

use base64::Engine as _;
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr};

/// smoltcp device backed by two packet queues: `rx` = decrypted packets from the tunnel,
/// `tx` = packets smoltcp wants to send into the tunnel.
#[derive(Default)]
struct QueueDevice {
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
    mtu: usize,
}
struct Rx(Vec<u8>);
struct Tx<'a>(&'a mut VecDeque<Vec<u8>>);
impl RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}
impl TxToken for Tx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0; len];
        let r = f(&mut buf);
        self.0.push_back(buf);
        r
    }
}
impl Device for QueueDevice {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx<'a>;
    fn receive(&mut self, _: Instant) -> Option<(Rx, Tx<'_>)> {
        let p = self.rx.pop_front()?;
        Some((Rx(p), Tx(&mut self.tx)))
    }
    fn transmit(&mut self, _: Instant) -> Option<Tx<'_>> {
        Some(Tx(&mut self.tx))
    }
    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ip;
        c.max_transmission_unit = self.mtu;
        c
    }
}

fn now(start: StdInstant) -> Instant {
    Instant::from_millis(start.elapsed().as_millis() as i64)
}

fn netstack(dev: &mut QueueDevice, ip: Ipv4Addr) -> Interface {
    let mut cfg = Config::new(HardwareAddress::Ip);
    let mut seed = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut seed);
    cfg.random_seed = u64::from_le_bytes(seed);
    let mut iface = Interface::new(cfg, dev, Instant::from_millis(0));
    iface.update_ip_addrs(|a| a.push(IpCidr::new(IpAddress::Ipv4(ip), 32)).unwrap());
    // Medium::Ip has no ARP; any gateway just marks "everything goes to the device".
    iface.routes_mut().add_default_ipv4_route(Ipv4Addr::new(10, 255, 255, 254)).unwrap();
    iface
}

fn tcp_socket() -> tcp::Socket<'static> {
    tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 65535]), tcp::SocketBuffer::new(vec![0; 65535]))
}

fn self_test() -> bool {
    let a = StaticSecret::random_from_rng(rand_core::OsRng);
    let b = StaticSecret::random_from_rng(rand_core::OsRng);
    let (pa, pb) = (PublicKey::from(&a), PublicKey::from(&b));
    let mut client = Tunn::new(a, pb, None, Some(25), 1, None);
    let mut server = Tunn::new(b, pa, None, None, 2, None);
    let mut buf = vec![0u8; 2048];
    let mut out = vec![0u8; 2048];

    // smoltcp produces a TCP SYN to 10.0.0.1:80.
    let start = StdInstant::now();
    let mut dev = QueueDevice { mtu: 1420, ..Default::default() };
    let mut iface = netstack(&mut dev, Ipv4Addr::new(10, 66, 0, 2));
    let mut sockets = SocketSet::new(vec![]);
    let h = sockets.add(tcp_socket());
    sockets.get_mut::<tcp::Socket>(h).connect(iface.context(), (IpAddress::v4(10, 0, 0, 1), 80), 49152).unwrap();
    iface.poll(now(start), &mut dev, &mut sockets);
    let syn = dev.tx.pop_front().expect("smoltcp emitted no SYN");
    println!("smoltcp SYN: {} bytes", syn.len());

    // First encapsulate has no session: it queues the SYN and returns a handshake initiation.
    let TunnResult::WriteToNetwork(init) = client.encapsulate(&syn, &mut buf) else { return false };
    println!("handshake initiation: {} bytes", init.len());
    let init = init.to_vec();
    let TunnResult::WriteToNetwork(resp) = server.decapsulate(None, &init, &mut out) else { return false };
    let resp = resp.to_vec();
    println!("handshake response: {} bytes", resp.len());
    // Client processes the response; then flushes the queued SYN (repeat with empty input).
    let mut sent = Vec::new();
    match client.decapsulate(None, &resp, &mut buf) {
        TunnResult::WriteToNetwork(p) => sent.push(p.to_vec()),
        r => println!("client after response: {}", name(&r)),
    }
    loop {
        match client.decapsulate(None, &[], &mut buf) {
            TunnResult::WriteToNetwork(p) => sent.push(p.to_vec()),
            _ => break,
        }
    }
    let mut ok = false;
    for d in sent {
        match server.decapsulate(None, &d, &mut out) {
            TunnResult::WriteToTunnelV4(p, src) => {
                println!("server got {} byte IPv4 packet from {src} (== SYN: {})", p.len(), p == &syn[..]);
                ok = p == &syn[..];
            }
            r => println!("server: {}", name(&r)),
        }
    }
    ok
}

fn name(r: &TunnResult) -> String {
    match r {
        TunnResult::Done => "Done".into(),
        TunnResult::Err(e) => format!("Err({e:?})"),
        TunnResult::WriteToNetwork(p) => format!("WriteToNetwork({})", p.len()),
        TunnResult::WriteToTunnelV4(p, a) => format!("WriteToTunnelV4({}, {a})", p.len()),
        TunnResult::WriteToTunnelV6(p, a) => format!("WriteToTunnelV6({}, {a})", p.len()),
    }
}

fn key(s: &str) -> [u8; 32] {
    base64::engine::general_purpose::STANDARD.decode(s).expect("base64").try_into().expect("32 bytes")
}

fn connect(endpoint: &str, server_pub: &str, client_priv: &str, client_ip: &str, inside: &str) -> bool {
    let ep: SocketAddr = std::net::ToSocketAddrs::to_socket_addrs(endpoint).unwrap().next().unwrap();
    let inside: SocketAddrV4 = inside.parse().unwrap();
    let sock = UdpSocket::bind("0.0.0.0:0").unwrap();
    sock.connect(ep).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    let mut tunn =
        Tunn::new(StaticSecret::from(key(client_priv)), PublicKey::from(key(server_pub)), None, Some(25), 7, None);

    let start = StdInstant::now();
    let mut dev = QueueDevice { mtu: 1420, ..Default::default() };
    let mut iface = netstack(&mut dev, client_ip.parse().unwrap());
    let mut sockets = SocketSet::new(vec![]);
    let h = sockets.add(tcp_socket());
    sockets
        .get_mut::<tcp::Socket>(h)
        .connect(iface.context(), (IpAddress::Ipv4(*inside.ip()), inside.port()), 49152)
        .unwrap();

    let (mut buf, mut out) = (vec![0u8; 65536], vec![0u8; 65536]);
    let (mut sent_req, mut handshaken) = (false, false);
    let mut body = Vec::new();
    while start.elapsed() < Duration::from_secs(15) {
        iface.poll(now(start), &mut dev, &mut sockets);
        while let Some(p) = dev.tx.pop_front() {
            if let TunnResult::WriteToNetwork(d) = tunn.encapsulate(&p, &mut buf) {
                sock.send(d).unwrap();
            }
        }
        if let TunnResult::WriteToNetwork(d) = tunn.update_timers(&mut buf) {
            sock.send(d).unwrap();
        }
        if let Ok(n) = sock.recv(&mut out) {
            let mut r = tunn.decapsulate(Some(ep.ip()), &out[..n], &mut buf);
            loop {
                match r {
                    TunnResult::WriteToNetwork(d) => {
                        if !handshaken {
                            handshaken = true;
                            println!("handshake complete after {:?}", start.elapsed());
                        }
                        sock.send(d).unwrap();
                        r = tunn.decapsulate(None, &[], &mut buf);
                        continue;
                    }
                    TunnResult::WriteToTunnelV4(p, _) => dev.rx.push_back(p.to_vec()),
                    TunnResult::WriteToTunnelV6(p, _) => dev.rx.push_back(p.to_vec()),
                    TunnResult::Err(e) => println!("decapsulate error: {e:?}"),
                    TunnResult::Done => {}
                }
                break;
            }
        }
        let s = sockets.get_mut::<tcp::Socket>(h);
        if s.may_send() && !sent_req {
            let req = format!("GET / HTTP/1.0\r\nHost: {inside}\r\nConnection: close\r\n\r\n");
            s.send_slice(req.as_bytes()).unwrap();
            sent_req = true;
        }
        if s.can_recv() {
            let mut b = [0u8; 4096];
            let n = s.recv_slice(&mut b).unwrap();
            body.extend_from_slice(&b[..n]);
        }
        if sent_req && !s.is_active() {
            break;
        }
        if sent_req && !s.may_recv() && !body.is_empty() {
            s.close();
        }
    }
    println!("--- response via tunnel ({} bytes) ---\n{}", body.len(), String::from_utf8_lossy(&body));
    !body.is_empty()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ok = match args.get(1).map(String::as_str) {
        Some("connect") if args.len() == 7 => connect(&args[2], &args[3], &args[4], &args[5], &args[6]),
        _ => self_test(),
    };
    std::process::exit(if ok { 0 } else { 1 });
}
