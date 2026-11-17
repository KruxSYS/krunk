//! The running VPN: its runtime, the connect/reconnect loop with back-off, the status.

use crate::dns::Resolver;
use crate::net::Netstack;
use crate::{now_ms, Credentials, IpNet, Profile, StartError, State, Status};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::{oneshot, watch};

/// How a tunnel session ended.
pub(crate) enum SessionEnd {
    /// `stop` was asked for.
    Stopped,
    /// Lost or never made: try again after the back-off.
    Retry(String),
    /// Refused for good (password, certificate): no retry. Only OpenVPN refuses for good.
    #[cfg_attr(not(feature = "openvpn"), allow(dead_code))]
    Failed(String),
}

/// What the tunnel, the proxy and the status share.
pub(crate) struct Shared {
    pub(crate) net: Netstack,
    pub(crate) dns: Resolver,
    routes: RwLock<Vec<IpNet>>,
    connected: watch::Sender<bool>,
    /// The current session reached Connected (resets the back-off).
    session_connected: AtomicBool,
    status: Mutex<Status>,
    /// Held while `on_status` runs, so reports arrive in the order of the changes.
    report: Mutex<()>,
    on_status: Box<dyn Fn(&Status) + Send + Sync>,
}

impl Shared {
    pub(crate) fn is_connected(&self) -> bool {
        *self.connected.borrow()
    }

    /// Resolves as soon as the tunnel is not connected (at once if it is not).
    pub(crate) async fn disconnected(&self) {
        let mut rx = self.connected.subscribe();
        let _ = rx.wait_for(|c| !*c).await;
    }

    /// The tunnel's routes (WireGuard AllowedIPs, OpenVPN routes and redirect-gateway).
    pub(crate) fn set_routes(&self, routes: Vec<IpNet>) {
        *self.routes.write().unwrap_or_else(PoisonError::into_inner) = routes;
    }

    pub(crate) fn routes_allow(&self, ip: IpAddr) -> bool {
        self.routes.read().unwrap_or_else(PoisonError::into_inner).iter().any(|n| n.contains(ip))
    }

    fn snapshot(&self) -> Status {
        let mut s = self.status.lock().unwrap_or_else(PoisonError::into_inner).clone();
        (s.rx_bytes, s.tx_bytes) = self.net.counters();
        s
    }

    /// Changes the state (ignored once stopped) and reports it.
    pub(crate) fn set_state(&self, state: State, error: Option<String>) {
        self.update(state, error, None);
    }

    /// The tunnel is up with our address `ip`.
    pub(crate) fn connected(&self, ip: Option<String>) {
        self.update(State::Connected, None, Some(ip));
    }

    /// Reports the current status (the 2 s ticker).
    fn report(&self) {
        let _order = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        (self.on_status)(&self.snapshot());
    }

    fn update(&self, state: State, error: Option<String>, ip: Option<Option<String>>) {
        let _order = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        let snapshot = {
            let mut s = self.status.lock().unwrap_or_else(PoisonError::into_inner);
            if s.state == State::Stopped {
                return;
            }
            if s.state == state && s.error == error && ip.as_ref().is_none_or(|ip| *ip == s.tunnel_ip) {
                return;
            }
            if s.state != state {
                s.since_ms = now_ms();
            }
            s.state = state;
            s.error = error;
            if let Some(ip) = ip {
                s.tunnel_ip = ip;
            }
            if state != State::Connected {
                s.tunnel_ip = None;
            } else {
                self.session_connected.store(true, Ordering::Release);
            }
            self.connected.send_replace(state == State::Connected);
            let mut snap = s.clone();
            (snap.rx_bytes, snap.tx_bytes) = self.net.counters();
            snap
        };
        (self.on_status)(&snapshot);
    }
}

/// Exponential back-off: 1 s, 2 s, 4 s … 60 s.
struct Backoff(u32);

impl Backoff {
    fn next(&mut self) -> Duration {
        let d = Duration::from_secs((1u64 << self.0.min(6)).min(60));
        self.0 += 1;
        d
    }
}

/// The proxy's owner check: `(proxy_local, peer)` of a new connection, where `proxy_local`
/// is the proxy's end (`127.0.0.1:<port>`) and `peer` the client's; true lets it in.
pub type PeerAllowed = Box<dyn Fn(SocketAddr, SocketAddr) -> bool + Send + Sync>;

/// How the runtime thread ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shutdown {
    /// Everything stops, the proxy last.
    All,
    /// The tunnel stops; the proxy keeps its port and answers 503 until the process ends.
    KeepProxy,
}

/// The running VPN. Dropping it stops everything; the proxy closes last.
pub struct Vpn {
    addr: SocketAddr,
    shared: Arc<Shared>,
    shutdown: Option<oneshot::Sender<Shutdown>>,
    thread: Option<JoinHandle<()>>,
}

impl Vpn {
    /// Binds the proxy on 127.0.0.1:0, then connects the tunnel in the background and
    /// reconnects with back-off. `on_status` is called on the VPN's runtime threads at each
    /// state change and every ~2 s while connected (bytes counters); it must not call
    /// [`Vpn::stop`] or drop the `Vpn`. `peer_allowed(proxy_local, peer)` is the owner check
    /// (always true on Linux); it runs on a blocking thread for each new proxy connection. The
    /// listener is the crate's [`crate::bind_loopback`] (exclusive on Windows).
    pub fn start(
        profile: Profile,
        creds: Option<Credentials>,
        peer_allowed: PeerAllowed,
        on_status: Box<dyn Fn(&Status) + Send + Sync>,
    ) -> Result<Vpn, StartError> {
        let creds = match &profile {
            #[cfg(feature = "openvpn")]
            Profile::OpenVpn(p) => {
                let c = creds.or_else(|| p.inline_credentials().cloned());
                if p.needs_credentials() && c.is_none() {
                    return Err(StartError::NeedsCredentials);
                }
                c
            }
            Profile::WireGuard(_) => {
                drop(creds);
                None
            }
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("krunk-vpn")
            .build()?;
        let std_listener = crate::listen::bind_loopback()?;
        std_listener.set_nonblocking(true)?;
        let addr = std_listener.local_addr()?;
        let server = profile.server();

        let _guard = runtime.enter();
        let listener = tokio::net::TcpListener::from_std(std_listener)?;
        let shared = Arc::new(Shared {
            net: Netstack::new(),
            dns: Resolver::new(),
            routes: RwLock::new(Vec::new()),
            connected: watch::channel(false).0,
            session_connected: AtomicBool::new(false),
            status: Mutex::new(Status {
                state: State::Connecting,
                error: None,
                since_ms: now_ms(),
                rx_bytes: 0,
                tx_bytes: 0,
                tunnel_ip: None,
                server,
            }),
            report: Mutex::new(()),
            on_status,
        });
        let (stop_tunnel, stop_rx) = watch::channel(false);
        let (stop_proxy, proxy_rx) = watch::channel(false);
        let peer_allowed: Arc<dyn Fn(SocketAddr, SocketAddr) -> bool + Send + Sync> = Arc::from(peer_allowed);
        let proxy = runtime.spawn(crate::proxy::serve(listener, shared.clone(), peer_allowed, proxy_rx));
        let tunnel = runtime.spawn(run(shared.clone(), profile, creds, stop_rx));
        let ticker = runtime.spawn(tick(shared.clone()));
        drop(_guard);

        let (shutdown, shutdown_rx) = oneshot::channel::<Shutdown>();
        let sh = shared.clone();
        let thread = std::thread::Builder::new().name("krunk-vpn-rt".into()).spawn(move || {
            runtime.block_on(async move {
                let how = shutdown_rx.await.unwrap_or(Shutdown::All);
                // Fail-closed while shutting down: the proxy answers 503 until it closes.
                sh.set_state(State::Stopped, None);
                let _ = stop_tunnel.send(true);
                let _ = tokio::time::timeout(Duration::from_secs(6), tunnel).await;
                ticker.abort();
                sh.net.close();
                if how == Shutdown::KeepProxy {
                    // The port stays Krunk's (answering 503) until the process ends.
                    std::future::pending::<()>().await;
                }
                let _ = stop_proxy.send(true);
                let _ = tokio::time::timeout(Duration::from_secs(1), proxy).await;
            });
            runtime.shutdown_timeout(Duration::from_secs(2));
        })?;
        Ok(Vpn { addr, shared, shutdown: Some(shutdown), thread: Some(thread) })
    }

    /// The proxy, e.g. `127.0.0.1:53127`.
    pub fn proxy_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn status(&self) -> Status {
        self.shared.snapshot()
    }

    /// Graceful stop: the proxy answers 503 until it is closed, after the tunnel.
    pub fn stop(self) {
        drop(self);
    }

    /// At exit: the tunnel stops (in the background, without waiting) and the proxy keeps
    /// its port, answering 503, until the process ends, so nothing else can take the port the
    /// engine still points at while its last requests end.
    pub fn stop_for_exit(mut self) {
        if let Some(s) = self.shutdown.take() {
            let _ = s.send(Shutdown::KeepProxy);
        }
        // Not joined: the runtime thread lives as long as the process.
        self.thread.take();
    }
}

impl Drop for Vpn {
    fn drop(&mut self) {
        if let Some(s) = self.shutdown.take() {
            let _ = s.send(Shutdown::All);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Resolves when a stop was asked for (or the VPN is gone).
pub(crate) async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|s| *s).await;
}

/// Bytes counters every 2 s while connected.
async fn tick(shared: Arc<Shared>) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if shared.is_connected() {
            shared.report();
        }
    }
}

/// Connect, and reconnect with back-off, until stopped or failed for good.
async fn run(shared: Arc<Shared>, profile: Profile, creds: Option<Credentials>, mut stop: watch::Receiver<bool>) {
    #[cfg(not(feature = "openvpn"))]
    drop(creds);
    shared.report();
    let mut backoff = Backoff(0);
    loop {
        shared.session_connected.store(false, Ordering::Release);
        // Each session starts with an empty DNS cache (new servers, maybe a new exit).
        shared.dns.clear_cache();
        let end = match &profile {
            Profile::WireGuard(cfg) => crate::wg::session(cfg, &shared, &mut stop).await,
            #[cfg(feature = "openvpn")]
            Profile::OpenVpn(p) => crate::ovpn::session(p, creds.as_ref(), &shared, &mut stop).await,
        };
        if shared.session_connected.load(Ordering::Acquire) {
            backoff = Backoff(0);
        }
        shared.net.set_sink(None);
        match end {
            SessionEnd::Stopped => return,
            SessionEnd::Failed(why) => {
                shared.set_state(State::Failed, Some(why));
                stopped(&mut stop).await;
                return;
            }
            SessionEnd::Retry(why) => {
                shared.set_state(State::Reconnecting, Some(why));
                let wait = backoff.next();
                tokio::select! {
                    _ = stopped(&mut stop) => return,
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn backoff_grows_and_caps() {
        let mut b = Backoff(0);
        let v: Vec<u64> = (0..9).map(|_| b.next().as_secs()).collect();
        assert_eq!(v, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }

    /// While the tunnel is not up the proxy answers 503 to every request and never connects
    /// anywhere; the PID hook can refuse a peer; stop is clean.
    #[test]
    fn fail_closed_while_not_connected() {
        // An endpoint that never answers (TEST-NET-1, port 9 discard).
        let text = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.66.0.2/32\n\
                    [Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nAllowedIPs = 0.0.0.0/0\nEndpoint = 192.0.2.1:9\n";
        let (profile, info) = crate::import("t.conf", text).unwrap();
        assert_eq!(info.server, "192.0.2.1:9");
        let seen = Arc::new(Mutex::new(Vec::<State>::new()));
        let seen2 = seen.clone();
        let refuse = Arc::new(AtomicBool::new(false));
        let refuse2 = refuse.clone();
        let vpn = Vpn::start(
            profile,
            None,
            Box::new(move |local, peer| {
                assert!(peer.ip().is_loopback() && local.ip().is_loopback());
                assert_ne!(local.port(), peer.port());
                !refuse2.load(Ordering::Relaxed)
            }),
            Box::new(move |s| seen2.lock().unwrap().push(s.state)),
        )
        .unwrap();
        let addr = vpn.proxy_addr();
        assert!(addr.ip().is_loopback() && addr.port() != 0);
        assert_eq!(vpn.status().state, State::Connecting);
        for req in [
            "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n",
            "GET http://10.0.0.1/ HTTP/1.1\r\n\r\n",
        ] {
            let mut c = std::net::TcpStream::connect(addr).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            c.write_all(req.as_bytes()).unwrap();
            let mut r = String::new();
            c.read_to_string(&mut r).unwrap();
            assert!(r.starts_with("HTTP/1.1 503 VPN not connected\r\n"), "{r}");
            assert!(r.ends_with("VPN not connected\n"));
        }
        // A bad request still gets its 400 (parsing comes first).
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(b"GET /x HTTP/1.1\r\n\r\n").unwrap();
        let mut r = String::new();
        c.read_to_string(&mut r).unwrap();
        assert!(r.starts_with("HTTP/1.1 400 "), "{r}");
        // A refused peer is closed without an answer.
        refuse.store(true, Ordering::Relaxed);
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let _ = c.write_all(b"GET http://example.com/ HTTP/1.1\r\n\r\n");
        let mut r = Vec::new();
        let _ = c.read_to_end(&mut r);
        assert!(r.is_empty());
        vpn.stop();
        assert!(std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_err());
        let seen = seen.lock().unwrap();
        assert_eq!(seen.first(), Some(&State::Connecting));
        assert_eq!(seen.last(), Some(&State::Stopped));
    }

    /// At exit the tunnel stops but the proxy keeps its port, answering 503.
    #[test]
    fn stop_for_exit_keeps_the_port() {
        let text = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.66.0.2/32\n\
                    [Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nAllowedIPs = 0.0.0.0/0\nEndpoint = 192.0.2.1:9\n";
        let (profile, _) = crate::import("t.conf", text).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let s2 = stopped.clone();
        let vpn = Vpn::start(
            profile,
            None,
            Box::new(|_, _| true),
            Box::new(move |s| {
                if s.state == State::Stopped {
                    s2.store(true, Ordering::SeqCst);
                }
            }),
        )
        .unwrap();
        let addr = vpn.proxy_addr();
        vpn.stop_for_exit();
        let t = std::time::Instant::now();
        while !stopped.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(stopped.load(Ordering::SeqCst));
        std::thread::sleep(Duration::from_millis(300));
        let mut c = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(b"GET http://example.com/ HTTP/1.1\r\n\r\n").unwrap();
        let mut r = String::new();
        c.read_to_string(&mut r).unwrap();
        assert!(r.starts_with("HTTP/1.1 503 VPN not connected\r\n"), "{r}");
    }
}
