//! M9 spike: link OpenVPN 3 Core (openvpn-connect, external-tun + vendor) and evaluate an inline
//! profile without connecting. Exit code 0 = the profile parsed and the capabilities look right.

use openvpn_connect::{Config, ExternalTun, capabilities, copyright, crypto_self_test, evaluate_config, platform};

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

fn main() {
    let caps = capabilities();
    println!("platform: {}", platform());
    println!("copyright: {}", copyright().lines().next().unwrap_or(""));
    println!("capabilities: {caps:?}");
    let self_test = crypto_self_test();
    println!("crypto self-test: {}", if self_test.is_empty() { "ok (empty)".to_owned() } else { self_test.lines().take(3).collect::<Vec<_>>().join(" | ") });
    let _tun: Box<dyn ExternalTun> = Box::new(NullTun);

    let ok = match evaluate_config(&Config::new(PROFILE)) {
        Ok(ev) => {
            println!("eval ok: remote={}:{}/{} autologin={} servers={}", ev.remote_host, ev.remote_port, ev.remote_proto, ev.autologin, ev.servers.len());
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
