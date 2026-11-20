//! lshim: a TCP latency shim for the 0.26.0 page-load harness (runs on the test VM in front of
//! Web Page Replay, the same for every browser).
//!
//!   lshim.exe [--rtt MS] LISTEN=TARGET...      e.g. 127.0.0.1:18443=127.0.0.1:8081
//!
//! Each accepted connection waits one RTT before it is connected through (the TCP handshake),
//! and every chunk is delivered RTT/2 after it was read, in each direction. So a TLS handshake
//! costs one more RTT and every request/response turn one RTT, as on a real network with that
//! round-trip time (no bandwidth limit, no loss). Threads and `thread::sleep` (high-resolution
//! timers on Windows) keep the delays within about a millisecond.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn pump(mut from: TcpStream, mut to: TcpStream, delay: Duration) {
    let (tx, rx) = mpsc::channel::<(Instant, Option<Vec<u8>>)>();
    let writer = thread::spawn(move || {
        while let Ok((due, chunk)) = rx.recv() {
            let now = Instant::now();
            if due > now {
                thread::sleep(due - now);
            }
            match chunk {
                Some(c) => {
                    if to.write_all(&c).is_err() {
                        break;
                    }
                }
                None => {
                    let _ = to.shutdown(Shutdown::Write);
                    break;
                }
            }
        }
    });
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => {
                let _ = tx.send((Instant::now() + delay, None));
                break;
            }
            Ok(n) => {
                if tx.send((Instant::now() + delay, Some(buf[..n].to_vec()))).is_err() {
                    break;
                }
            }
        }
    }
    let _ = writer.join();
}

fn serve(listen: String, target: String, rtt: Duration) {
    let l = TcpListener::bind(&listen).unwrap_or_else(|e| panic!("bind {listen}: {e}"));
    eprintln!("lshim: {listen} -> {target}, rtt {} ms", rtt.as_millis());
    for client in l.incoming().flatten() {
        let target = target.clone();
        thread::spawn(move || {
            let _ = client.set_nodelay(true);
            thread::sleep(rtt);
            let Ok(up) = TcpStream::connect(&target) else { return };
            let _ = up.set_nodelay(true);
            let (Ok(c2), Ok(u2)) = (client.try_clone(), up.try_clone()) else { return };
            let half = rtt / 2;
            let a = thread::spawn(move || pump(client, up, half));
            pump(u2, c2, half);
            let _ = a.join();
        });
    }
}

fn main() {
    let mut rtt = Duration::from_millis(40);
    let mut maps = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--rtt" {
            rtt = Duration::from_millis(args.next().and_then(|v| v.parse().ok()).expect("--rtt MS"));
        } else if let Some((l, t)) = a.split_once('=') {
            maps.push((l.to_string(), t.to_string()));
        } else {
            eprintln!("usage: lshim [--rtt MS] LISTEN=TARGET...");
            std::process::exit(2);
        }
    }
    let handles: Vec<_> = maps.into_iter().map(|(l, t)| thread::spawn(move || serve(l, t, rtt))).collect();
    for h in handles {
        let _ = h.join();
    }
}
