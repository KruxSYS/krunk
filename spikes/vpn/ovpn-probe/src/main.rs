//! M9 spike: link OpenVPN 3 Core (openvpn-connect, external-tun + vendor) and evaluate an inline
//! profile without connecting. Exit code 0 = the profile parsed and the capabilities look right.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use openvpn_connect::{
    Client, Config, Credentials, Event, EventHandler, ExternalTun, ExternalTunConfig, ExternalTunInfo, ExternalTunIo,
    ExternalTunStartConfig, capabilities, copyright, crypto_self_test, evaluate_config, platform,
};

// Test-only CA generated for the spike (no key kept); OpenVPN parses it during evaluation.
const PROFILE: &str = r"client
dev tun
proto udp
remote vpn.example.test 1194
remote vpn.example.test 443 tcp
resolv-retry infinite
nobind
auth-user-pass
cipher AES-256-GCM
data-ciphers AES-256-GCM:CHACHA20-POLY1305
verb 3
<ca>
-----BEGIN CERTIFICATE-----
MIIBejCCASGgAwIBAgIUAecf+zsereYcFyJAY5Kw5GoN0P0wCgYIKoZIzj0EAwIw
EzERMA8GA1UEAwwIcHJvYmUtY2EwHhcNMjYxMTEwMTYzNzE2WhcNMzYxMTA3MTYz
NzE2WjATMREwDwYDVQQDDAhwcm9iZS1jYTBZMBMGByqGSM49AgEGCCqGSM49AwEH
A0IABKIOoJUQsmektJOukRpVDF7kS4JMquy2xTTwfTwkim11L4NWUcwRzJGYQPbm
PXaw3vHFblpKk2S8JXw/KwwoQMWjUzBRMB0GA1UdDgQWBBREjN7pYAtw7RsOPZs5
NrMrxYdt6TAfBgNVHSMEGDAWgBREjN7pYAtw7RsOPZs5NrMrxYdt6TAPBgNVHRMB
Af8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCIA8+fcJ5s1xvIORlZwEV628VD+qH
WsGQyQBatI0mNsWAAiARGFrD/m8QX5DVtKf+jThJW0arJCQwnbBPWhJctLrNFw==
-----END CERTIFICATE-----
</ca>
";

/// Shape of the packet sink Krunk's netstack would implement (not started here).
struct NullTun;
impl ExternalTun for NullTun {}

/// `connect` mode: a real session with an external TUN that only records what Core pushes.
#[derive(Default)]
struct ProbeTun {
    io: Mutex<Option<ExternalTunIo>>,
    ip: Mutex<String>,
    rx_packets: Mutex<usize>,
    ping_ok: Mutex<bool>,
}

fn csum(b: &[u8]) -> u16 {
    let mut s: u32 = b.chunks(2).map(|c| u32::from(c[0]) << 8 | u32::from(*c.get(1).unwrap_or(&0))).sum();
    while s > 0xffff {
        s = (s & 0xffff) + (s >> 16);
    }
    !(s as u16)
}

/// IPv4 ICMP echo request `src -> dst`, as a netstack would hand it to the tunnel.
fn ping(src: [u8; 4], dst: [u8; 4]) -> Vec<u8> {
    let mut p = vec![0x45, 0, 0, 36, 0x12, 0x34, 0x40, 0, 64, 1, 0, 0];
    p.extend_from_slice(&src);
    p.extend_from_slice(&dst);
    let c = csum(&p);
    p[10..12].copy_from_slice(&c.to_be_bytes());
    let mut icmp = vec![8, 0, 0, 0, 0x4b, 0x52, 0, 1, b'k', b'r', b'u', b'n', b'k', b'!', b'!', b'!'];
    let c = csum(&icmp);
    icmp[2..4].copy_from_slice(&c.to_be_bytes());
    p.extend_from_slice(&icmp);
    p
}
impl ExternalTun for ProbeTun {
    fn configure(&self, c: &ExternalTunConfig) -> bool {
        println!("tun configure: layer={} mtu={}", c.layer, c.mtu);
        true
    }
    fn start(&self, c: &ExternalTunStartConfig, io: ExternalTunIo) {
        let opts: Vec<&str> = c.options.lines().collect();
        println!("tun start: cipher={} options={:?}", c.cipher_algorithm, opts);
        if let Some(ip) = c.options.lines().find_map(|o| o.strip_prefix("ifconfig ")) {
            *self.ip.lock().unwrap() = ip.split(' ').next().unwrap_or("").to_owned();
        }
        let _ = io.pre_tun_config();
        let _ = io.pre_route_config();
        let _ = io.connected();
        *self.io.lock().unwrap() = Some(io);
    }
    fn send(&self, packet: &[u8]) -> bool {
        *self.rx_packets.lock().unwrap() += 1;
        let icmp_reply = packet.len() >= 28 && packet[9] == 1 && packet[20] == 0;
        println!(
            "tun send: {} byte packet from Core: {}.{}.{}.{} -> {}.{}.{}.{} proto {}{}",
            packet.len(),
            packet[12],
            packet[13],
            packet[14],
            packet[15],
            packet[16],
            packet[17],
            packet[18],
            packet[19],
            packet[9],
            if icmp_reply { " (ICMP echo reply)" } else { "" }
        );
        if icmp_reply {
            *self.ping_ok.lock().unwrap() = true;
        }
        true
    }
    fn info(&self) -> ExternalTunInfo {
        ExternalTunInfo {
            name: "krunk-probe".into(),
            vpn_ipv4: self.ip.lock().unwrap().clone(),
            mtu: 1500,
            ..Default::default()
        }
    }
}
struct Handler(Arc<ProbeTun>, Arc<Mutex<bool>>);
impl EventHandler for Handler {
    fn event(&self, e: Event) {
        println!("event: {} {}{}", e.name, e.info, if e.error { " (error)" } else { "" });
        if e.name == "CONNECTED" {
            *self.1.lock().unwrap() = true;
        }
    }
    fn log(&self, t: &str) {
        if std::env::var_os("OVPN_PROBE_LOG").is_some() {
            println!("log: {}", t.trim_end());
        }
    }
    fn external_tun(&self) -> Option<&dyn ExternalTun> {
        Some(self.0.as_ref())
    }
}

fn connect(profile: &str, creds: Option<(&str, &str)>) -> bool {
    let tun = Arc::new(ProbeTun::default());
    let connected = Arc::new(Mutex::new(false));
    let client = Client::new(Handler(tun.clone(), connected.clone())).expect("client");
    let mut config = Config::new(profile);
    // Username/password-only profiles (no <cert>/<key>): OpenVPN 3 otherwise expects External PKI
    // ("Missing External PKI alias"). Same as `setenv CLIENT_CERT 0` in the profile.
    config.disable_client_cert = !profile.contains("<cert>");
    let ev = client.evaluate(&config).expect("evaluate");
    println!("profile: {}:{}/{}", ev.remote_host, ev.remote_port, ev.remote_proto);
    if let Some((u, p)) = creds {
        client.provide_credentials(&Credentials::new(u, p)).expect("credentials");
    }
    let c2 = client.clone();
    let tun2 = tun.clone();
    let watcher = std::thread::spawn(move || {
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(100));
            if *connected.lock().unwrap() {
                let ip: Vec<u8> = tun2.ip.lock().unwrap().split('.').filter_map(|o| o.parse().ok()).collect();
                if let (Ok(src), Some(io)) = (<[u8; 4]>::try_from(ip), tun2.io.lock().unwrap().as_ref()) {
                    println!(
                        "injecting ICMP echo {src:?} -> 10.77.0.10: {:?}",
                        io.receive(&ping(src, [10, 77, 0, 10]))
                    );
                }
                std::thread::sleep(Duration::from_secs(2));
                break;
            }
        }
        if let Some(info) = c2.connection_info() {
            println!(
                "connection info: vpn_ip4={} server={}:{} proto={}",
                info.vpn_ip4, info.server_host, info.server_port, info.server_proto
            );
        }
        let ok = *connected.lock().unwrap() && *tun2.ping_ok.lock().unwrap();
        c2.stop();
        ok
    });
    let status = client.connect();
    println!("connect returned: {status:?}");
    let ok = watcher.join().unwrap();
    println!("pushed ip: {:?}, packets from Core: {}", tun.ip.lock().unwrap(), tun.rx_packets.lock().unwrap());
    ok
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("connect") {
        let profile = std::fs::read_to_string(&args[2]).expect("read profile");
        let creds = (args.len() >= 5).then(|| (args[3].as_str(), args[4].as_str()));
        std::process::exit(if connect(&profile, creds) { 0 } else { 1 });
    }
    let caps = capabilities();
    println!("platform: {}", platform());
    println!("copyright: {}", copyright().lines().next().unwrap_or(""));
    println!("capabilities: {caps:?}");
    let self_test = crypto_self_test();
    println!(
        "crypto self-test: {}",
        if self_test.is_empty() {
            "ok (empty)".to_owned()
        } else {
            self_test.lines().take(3).collect::<Vec<_>>().join(" | ")
        }
    );
    let _tun: Box<dyn ExternalTun> = Box::new(NullTun);

    let ok = match evaluate_config(&Config::new(PROFILE)) {
        Ok(ev) => {
            println!(
                "eval ok: remote={}:{}/{} autologin={} servers={}",
                ev.remote_host,
                ev.remote_port,
                ev.remote_proto,
                ev.autologin,
                ev.servers.len()
            );
            ev.remote_host == "vpn.example.test" && !ev.autologin
        }
        Err(e) => {
            println!("eval error: {e}");
            false
        }
    };
    // A broken profile must be rejected by Core's parser.
    let bad = evaluate_config(&Config::new("client\nremote\n<ca>\nnot a cert\n</ca>\n"));
    println!("bad profile -> {:?}", bad.as_ref().map(|e| &e.remote_host));
    std::process::exit(if ok && bad.is_err() && caps.external_tun { 0 } else { 1 });
}
