//! A manual check of the `vpn` crate: import a WireGuard or OpenVPN file, start the VPN and
//! print the proxy address and every status change (JSON lines).
//!
//! ```text
//! cargo run -p vpn --example vpn-proxy -- <file.conf|file.ovpn> [--user U --pass P] [--seconds N]
//! curl -x http://<printed proxy> http://inside.test/
//! ```
//! Without `--seconds` it runs until Enter is pressed. The password can also come from
//! `KRUNK_VPN_PASS` (keeps it off the command line).

use krunk_vpn::{import, Credentials, Vpn};
use std::time::Duration;
use zeroize::Zeroizing;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut file = None;
    let (mut user, mut pass, mut seconds) = (None, std::env::var("KRUNK_VPN_PASS").ok(), None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--user" => user = it.next().cloned(),
            "--pass" => pass = it.next().cloned(),
            "--seconds" => seconds = it.next().and_then(|s| s.parse::<u64>().ok()),
            _ if file.is_none() => file = Some(a.clone()),
            _ => usage(),
        }
    }
    let Some(file) = file else { usage() };
    let text = match std::fs::read(&file) {
        Ok(b) if b.len() <= krunk_vpn::MAX_IMPORT_BYTES => String::from_utf8_lossy(&b).into_owned(),
        Ok(_) => fail("the file is larger than 64 KiB"),
        Err(e) => fail(&format!("cannot read {file}: {e}")),
    };
    let (profile, info) = match import(&file, &text) {
        Ok(p) => p,
        Err(e) => fail(&format!("import failed: {e}")),
    };
    println!("profile: {}", serde_json::to_string(&info).unwrap_or_default());
    let creds = user.map(|u| Credentials {
        username: Zeroizing::new(u),
        password: Zeroizing::new(pass.take().unwrap_or_default()),
    });
    let vpn = match Vpn::start(
        profile,
        creds,
        Box::new(|_, _| true),
        Box::new(|s| println!("status: {}", serde_json::to_string(s).unwrap_or_default())),
    ) {
        Ok(v) => v,
        Err(e) => fail(&e.to_string()),
    };
    println!("proxy: http://{}", vpn.proxy_addr());
    match seconds {
        Some(s) => std::thread::sleep(Duration::from_secs(s)),
        None => {
            println!("press Enter to stop");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
        }
    }
    println!("final: {}", serde_json::to_string(&vpn.status()).unwrap_or_default());
    vpn.stop();
}

fn usage() -> ! {
    eprintln!("usage: vpn-proxy <file.conf|file.ovpn> [--user U --pass P] [--seconds N]");
    std::process::exit(2);
}

fn fail(m: &str) -> ! {
    eprintln!("vpn-proxy: {m}");
    std::process::exit(1);
}
