//! The TCP/IP stack over a packet tunnel: smoltcp with `Medium::Ip` on the tunnel's address
//! and MTU. The tunnel pushes decrypted packets in with [`Netstack::inject`]; the stack's
//! outgoing packets go to the current [`PacketSink`]. A tokio task polls the stack; TCP
//! connections ([`TcpStream`]) are tokio `AsyncRead`/`AsyncWrite` streams through smoltcp's
//! async wakers, and [`Netstack::udp_exchange`] does one UDP request/answer (DNS).

use crate::IpNet;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Instant as SmolInstant;
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv6Address};
use std::collections::VecDeque;
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;

/// TCP buffers per connection: the receive buffer sets the window (scaled).
const TCP_RX: usize = 256 * 1024;
const TCP_TX: usize = 128 * 1024;
/// The most TCP connections open at once (Chromium uses at most 32 per proxy).
pub const MAX_TCP: usize = 256;
/// Incoming packets waiting for the stack, by count and by bytes; more are dropped (TCP
/// retransmits).
const RX_QUEUE: usize = 4096;
const RX_QUEUE_BYTES: usize = 16 * 1024 * 1024;
/// A TCP connection that gets no ACK for this long is aborted.
const TCP_TIMEOUT: Duration = Duration::from_secs(60);
/// A closed connection that never finishes its FIN handshake is aborted after this long.
const LINGER: Duration = Duration::from_secs(30);

/// Where the stack's outgoing IP packets go (the tunnel's encrypt side).
pub trait PacketSink: Send + Sync {
    fn send(&self, packet: &[u8]);
}

struct QueueDevice {
    rx: VecDeque<Vec<u8>>,
    /// The bytes in `rx`.
    rx_queued: usize,
    tx: Vec<Vec<u8>>,
    mtu: usize,
}

impl QueueDevice {
    /// Queues an incoming packet unless the queue is full (by count or by bytes).
    fn push_rx(&mut self, packet: &[u8]) -> bool {
        if self.rx.len() >= RX_QUEUE || self.rx_queued + packet.len() > RX_QUEUE_BYTES {
            return false;
        }
        self.rx_queued += packet.len();
        self.rx.push_back(packet.to_vec());
        true
    }
}

struct Rx(Vec<u8>);
struct Tx<'a>(&'a mut Vec<Vec<u8>>);

impl RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl TxToken for Tx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0; len];
        let r = f(&mut buf);
        self.0.push(buf);
        r
    }
}

impl Device for QueueDevice {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx<'a>;
    fn receive(&mut self, _: SmolInstant) -> Option<(Rx, Tx<'_>)> {
        let p = self.rx.pop_front()?;
        self.rx_queued -= p.len();
        Some((Rx(p), Tx(&mut self.tx)))
    }
    fn transmit(&mut self, _: SmolInstant) -> Option<Tx<'_>> {
        Some(Tx(&mut self.tx))
    }
    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ip;
        c.max_transmission_unit = self.mtu;
        c
    }
}

struct Stack {
    iface: Interface,
    dev: QueueDevice,
    sockets: SocketSet<'static>,
    addrs: Vec<IpNet>,
    /// Dropped TCP sockets that still finish their close: handle, when, polled since.
    dead: Vec<(SocketHandle, Instant, bool)>,
    next_port: u16,
}

struct Inner {
    stack: Mutex<Stack>,
    wake: Notify,
    sink: RwLock<Option<Arc<dyn PacketSink>>>,
    start: Instant,
    rx_bytes: AtomicU64,
    tx_bytes: AtomicU64,
    tcp_open: AtomicUsize,
    closed: AtomicBool,
}

/// The stack. Cloning shares it.
#[derive(Clone)]
pub struct Netstack {
    inner: Arc<Inner>,
}

impl Default for Netstack {
    fn default() -> Self {
        Netstack::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    fn now(&self) -> SmolInstant {
        SmolInstant::from_millis(i64::try_from(self.start.elapsed().as_millis()).unwrap_or(i64::MAX))
    }
}

impl Netstack {
    /// A stack with no address yet; its poll task runs on the current tokio runtime until
    /// [`Netstack::close`].
    pub fn new() -> Netstack {
        let mut dev = QueueDevice { rx: VecDeque::new(), rx_queued: 0, tx: Vec::new(), mtu: 1420 };
        let mut cfg = Config::new(HardwareAddress::Ip);
        cfg.random_seed = u64::from_le_bytes(crate::random::<8>());
        let start = Instant::now();
        let iface = Interface::new(cfg, &mut dev, SmolInstant::from_millis(0));
        let port = 49152 + (u16::from_le_bytes(crate::random::<2>()) % 16000);
        let inner = Arc::new(Inner {
            stack: Mutex::new(Stack {
                iface,
                dev,
                sockets: SocketSet::new(vec![]),
                addrs: Vec::new(),
                dead: Vec::new(),
                next_port: port,
            }),
            wake: Notify::new(),
            sink: RwLock::new(None),
            start,
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            tcp_open: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
        });
        tokio::spawn(poll_task(inner.clone()));
        Netstack { inner }
    }

    /// Sets our tunnel addresses (at most one IPv4 and one IPv6) and the MTU. When the
    /// addresses change, open TCP connections are reset (they belong to the old address).
    pub fn configure(&self, addrs: Vec<IpNet>, mtu: usize) {
        let mut v4 = addrs.iter().find(|a| a.addr.is_ipv4()).copied();
        let v6 = addrs.iter().find(|a| a.addr.is_ipv6()).copied();
        if let Some(a) = v4.as_mut() {
            a.prefix = 32;
        }
        let addrs: Vec<IpNet> = v4.into_iter().chain(v6.map(|a| IpNet { prefix: 128, ..a })).collect();
        let mut st = lock(&self.inner.stack);
        let mtu = mtu.clamp(576, 9000);
        let mtu_changed = st.dev.mtu != mtu;
        st.dev.mtu = mtu;
        if st.addrs == addrs && !mtu_changed {
            return;
        }
        abort_all(&mut st);
        if mtu_changed {
            // smoltcp copies the device's capabilities (the MTU, and with it the TCP segment
            // size) into the interface when it is made: a new MTU needs a new interface.
            // Before 0.14.1 a profile's smaller MTU never reached it, so full 1420-byte packets
            // overflowed the tunnel's buffer (boringtun panics; the browser aborted).
            let mut cfg = Config::new(HardwareAddress::Ip);
            cfg.random_seed = u64::from_le_bytes(crate::random::<8>());
            let now = self.inner.now();
            let Stack { iface, dev, .. } = &mut *st;
            *iface = Interface::new(cfg, dev, now);
            st.addrs.clear();
        }
        st.iface.update_ip_addrs(|ips| {
            ips.clear();
            for a in &addrs {
                let _ = ips.push(IpCidr::new(IpAddress::from(a.addr), a.prefix));
            }
        });
        // Medium::Ip has no neighbours: a default route to any gateway sends everything to the
        // device (the tunnel).
        let routes = st.iface.routes_mut();
        routes.remove_default_ipv4_route();
        routes.remove_default_ipv6_route();
        if v4.is_some() {
            let _ = routes.add_default_ipv4_route(Ipv4Address::new(169, 254, 0, 1));
        }
        if v6.is_some() {
            let _ = routes.add_default_ipv6_route(Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
        }
        st.addrs = addrs;
        drop(st);
        self.inner.wake.notify_one();
    }

    /// Our addresses (`/32`, `/128`).
    pub fn addresses(&self) -> Vec<IpNet> {
        lock(&self.inner.stack).addrs.clone()
    }

    pub fn has_ipv6(&self) -> bool {
        self.addresses().iter().any(|a| a.addr.is_ipv6())
    }

    /// Sets where outgoing packets go; `None` drops them (the tunnel is down).
    pub fn set_sink(&self, sink: Option<Arc<dyn PacketSink>>) {
        *self.inner.sink.write().unwrap_or_else(PoisonError::into_inner) = sink;
        self.inner.wake.notify_one();
    }

    /// A decrypted IP packet from the tunnel.
    pub fn inject(&self, packet: &[u8]) {
        if packet.is_empty() || packet.len() > 65535 {
            return;
        }
        self.inner.rx_bytes.fetch_add(packet.len() as u64, Ordering::Relaxed);
        let mut st = lock(&self.inner.stack);
        st.dev.push_rx(packet);
        drop(st);
        self.inner.wake.notify_one();
    }

    /// Bytes of IP packets received from / sent into the tunnel.
    pub fn counters(&self) -> (u64, u64) {
        (self.inner.rx_bytes.load(Ordering::Relaxed), self.inner.tx_bytes.load(Ordering::Relaxed))
    }

    /// Stops the poll task and aborts every connection.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let mut st = lock(&self.inner.stack);
        abort_all(&mut st);
        drop(st);
        self.inner.wake.notify_one();
    }

    fn local_ip(st: &Stack, v6: bool) -> Option<IpAddr> {
        st.addrs.iter().find(|a| a.addr.is_ipv6() == v6).map(|a| a.addr)
    }

    /// The next free local port (49152–65000), skipping ports still in use.
    fn port(st: &mut Stack) -> u16 {
        let used: Vec<u16> = st
            .sockets
            .iter()
            .filter_map(|(_, s)| match s {
                smoltcp::socket::Socket::Tcp(t) => t.local_endpoint().map(|e| e.port),
                smoltcp::socket::Socket::Udp(u) => Some(u.endpoint().port),
                #[allow(unreachable_patterns)]
                _ => None,
            })
            .collect();
        loop {
            let p = st.next_port;
            st.next_port = if p >= 65000 { 49152 } else { p + 1 };
            if !used.contains(&p) {
                return p;
            }
        }
    }

    /// Opens a TCP connection through the tunnel; waits until it is established.
    pub async fn tcp_connect(&self, dst: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        if self.inner.tcp_open.load(Ordering::Relaxed) >= MAX_TCP {
            return Err(io::Error::other("too many connections through the VPN"));
        }
        let stream = {
            let mut st = lock(&self.inner.stack);
            let Some(local) = Self::local_ip(&st, dst.is_ipv6()) else {
                return Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "no tunnel address for this family"));
            };
            let mut s =
                tcp::Socket::new(tcp::SocketBuffer::new(vec![0; TCP_RX]), tcp::SocketBuffer::new(vec![0; TCP_TX]));
            s.set_nagle_enabled(false);
            s.set_timeout(Some(smoltcp::time::Duration::from_millis(TCP_TIMEOUT.as_millis() as u64)));
            let st = &mut *st;
            let port = Self::port(st);
            s.connect(st.iface.context(), IpEndpoint::from(dst), (IpAddress::from(local), port))
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "cannot connect to this address"))?;
            let handle = st.sockets.add(s);
            self.inner.tcp_open.fetch_add(1, Ordering::Relaxed);
            TcpStream { inner: self.inner.clone(), handle, closed: false }
        };
        self.inner.wake.notify_one();
        let established = poll_fn(|cx| {
            let mut st = lock(&stream.inner.stack);
            let s = st.sockets.get_mut::<tcp::Socket>(stream.handle);
            match s.state() {
                tcp::State::Established | tcp::State::CloseWait => Poll::Ready(Ok(())),
                tcp::State::Closed | tcp::State::TimeWait => {
                    Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionRefused, "connection refused")))
                }
                _ => {
                    s.register_send_waker(cx.waker());
                    s.register_recv_waker(cx.waker());
                    Poll::Pending
                }
            }
        });
        match tokio::time::timeout(timeout, established).await {
            Ok(Ok(())) => Ok(stream),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "connection timed out")),
        }
    }

    /// Sends one UDP datagram to `dst` and waits for the first answer from it.
    pub async fn udp_exchange(&self, dst: SocketAddr, payload: &[u8], timeout: Duration) -> io::Result<Vec<u8>> {
        let sock = {
            let mut st = lock(&self.inner.stack);
            let Some(local) = Self::local_ip(&st, dst.is_ipv6()) else {
                return Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "no tunnel address for this family"));
            };
            let mut s = udp::Socket::new(
                udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 16384]),
                udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 2], vec![0; 4096]),
            );
            let port = Self::port(&mut st);
            s.bind((IpAddress::from(local), port)).map_err(|_| io::Error::other("udp bind"))?;
            s.send_slice(payload, IpEndpoint::from(dst)).map_err(|_| io::Error::other("udp send"))?;
            let handle = st.sockets.add(s);
            UdpSock { inner: self.inner.clone(), handle }
        };
        self.inner.wake.notify_one();
        let want = IpEndpoint::from(dst);
        let answer = poll_fn(|cx| {
            let mut st = lock(&sock.inner.stack);
            let s = st.sockets.get_mut::<udp::Socket>(sock.handle);
            loop {
                match s.recv() {
                    Ok((data, meta)) if meta.endpoint == want => return Poll::Ready(data.to_vec()),
                    Ok(_) => continue,
                    Err(_) => {
                        s.register_recv_waker(cx.waker());
                        return Poll::Pending;
                    }
                }
            }
        });
        tokio::time::timeout(timeout, answer).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no answer"))
    }
}

fn abort_all(st: &mut Stack) {
    for (_, sock) in st.sockets.iter_mut() {
        if let smoltcp::socket::Socket::Tcp(s) = sock {
            s.abort();
        }
    }
}

async fn poll_task(inner: Arc<Inner>) {
    loop {
        if inner.closed.load(Ordering::Acquire) {
            return;
        }
        let (out, delay) = {
            let mut guard = lock(&inner.stack);
            let st = &mut *guard;
            let now = inner.now();
            st.iface.poll(now, &mut st.dev, &mut st.sockets);
            reap(st, &inner);
            let out = std::mem::take(&mut st.dev.tx);
            let delay = st.iface.poll_delay(now, &st.sockets);
            (out, delay)
        };
        if !out.is_empty() {
            let sink = inner.sink.read().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(sink) = sink {
                for p in &out {
                    inner.tx_bytes.fetch_add(p.len() as u64, Ordering::Relaxed);
                    sink.send(p);
                }
            }
        }
        let delay = delay.map_or(Duration::from_secs(1), |d| Duration::from_micros(d.total_micros()));
        if delay.is_zero() {
            tokio::task::yield_now().await;
            continue;
        }
        tokio::select! {
            _ = inner.wake.notified() => {}
            _ = tokio::time::sleep(delay.min(Duration::from_secs(1))) => {}
        }
    }
}

/// Removes dropped TCP sockets once they are closed (after at least one poll, so the RST or
/// FIN went out), and aborts the ones that linger.
fn reap(st: &mut Stack, inner: &Inner) {
    let mut i = 0;
    while i < st.dead.len() {
        let (h, since, polled) = st.dead[i];
        let s = st.sockets.get_mut::<tcp::Socket>(h);
        let done = matches!(s.state(), tcp::State::Closed | tcp::State::TimeWait);
        if done && polled {
            st.sockets.remove(h);
            st.dead.swap_remove(i);
            inner.tcp_open.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        if since.elapsed() > LINGER {
            s.abort();
        }
        st.dead[i].2 = true;
        i += 1;
    }
}

/// A TCP connection through the tunnel.
pub struct TcpStream {
    inner: Arc<Inner>,
    handle: SocketHandle,
    closed: bool,
}

impl AsyncRead for TcpStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let mut st = lock(&this.inner.stack);
        let s = st.sockets.get_mut::<tcp::Socket>(this.handle);
        if s.can_recv() {
            let n = s.recv_slice(buf.initialize_unfilled()).unwrap_or(0);
            buf.advance(n);
            drop(st);
            // The window opened: let the stack tell the peer.
            this.inner.wake.notify_one();
            return Poll::Ready(Ok(()));
        }
        if !s.may_recv() {
            // The peer's FIN is a clean end; a reset, a timeout or an abort (the tunnel went
            // away) is not, so a cut-off response is never taken for a complete one.
            return Poll::Ready(match s.recv_slice(&mut []) {
                Err(tcp::RecvError::Finished) => Ok(()),
                _ => Err(io::Error::new(io::ErrorKind::ConnectionReset, "the connection was reset")),
            });
        }
        s.register_recv_waker(cx.waker());
        Poll::Pending
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let mut st = lock(&this.inner.stack);
        let s = st.sockets.get_mut::<tcp::Socket>(this.handle);
        if !s.may_send() {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "the connection is closed")));
        }
        if s.can_send() {
            let n = s.send_slice(data).unwrap_or(0);
            drop(st);
            this.inner.wake.notify_one();
            return Poll::Ready(Ok(n));
        }
        s.register_send_waker(cx.waker());
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        lock(&this.inner.stack).sockets.get_mut::<tcp::Socket>(this.handle).close();
        this.closed = true;
        this.inner.wake.notify_one();
        Poll::Ready(Ok(()))
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let mut st = lock(&self.inner.stack);
        let s = st.sockets.get_mut::<tcp::Socket>(self.handle);
        if self.closed {
            // FIN is queued: let the remaining data and the FIN go out.
            s.close();
        } else {
            s.abort();
        }
        st.dead.push((self.handle, Instant::now(), false));
        drop(st);
        self.inner.wake.notify_one();
    }
}

struct UdpSock {
    inner: Arc<Inner>,
    handle: SocketHandle,
}

impl Drop for UdpSock {
    fn drop(&mut self) {
        lock(&self.inner.stack).sockets.remove(self.handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Two stacks wired back to back: what one sends, the other receives.
    struct Wire(Netstack);
    impl PacketSink for Wire {
        fn send(&self, packet: &[u8]) {
            self.0.inject(packet);
        }
    }

    /// Passes packets on and remembers the largest.
    struct Measure(Netstack, Arc<AtomicUsize>);
    impl PacketSink for Measure {
        fn send(&self, packet: &[u8]) {
            self.1.fetch_max(packet.len(), Ordering::Relaxed);
            self.0.inject(packet);
        }
    }

    /// 0.14.1: a profile's MTU below the default reaches the TCP/IP stack. Before, smoltcp kept
    /// the 1420 it was made with, sent packets larger than the tunnel's buffer, and boringtun's
    /// panic aborted Krunk on the first page through a WireGuard profile with `MTU = 1280`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_smaller_mtu_limits_the_packets_sent() {
        let a = Netstack::new();
        let b = Netstack::new();
        a.configure(vec![IpNet::parse("10.0.0.1/24").unwrap()], 1280);
        b.configure(vec![IpNet::parse("10.0.0.2/24").unwrap()], 1420);
        let largest = Arc::new(AtomicUsize::new(0));
        a.set_sink(Some(Arc::new(Measure(b.clone(), largest.clone()))));
        b.set_sink(Some(Arc::new(Wire(a.clone()))));
        let listen = {
            let mut st = lock(&b.inner.stack);
            let mut s =
                tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 65536]), tcp::SocketBuffer::new(vec![0; 65536]));
            s.listen(8080).unwrap();
            let h = st.sockets.add(s);
            b.inner.tcp_open.fetch_add(1, Ordering::Relaxed);
            TcpStream { inner: b.inner.clone(), handle: h, closed: false }
        };
        let server = tokio::spawn(async move {
            let mut listen = listen;
            poll_fn(|cx| {
                let mut st = lock(&listen.inner.stack);
                let s = st.sockets.get_mut::<tcp::Socket>(listen.handle);
                if s.may_recv() {
                    return Poll::Ready(());
                }
                s.register_recv_waker(cx.waker());
                Poll::Pending
            })
            .await;
            let mut got = vec![0u8; 100_000];
            listen.read_exact(&mut got).await.unwrap();
        });
        let mut c = a.tcp_connect("10.0.0.2:8080".parse().unwrap(), Duration::from_secs(5)).await.unwrap();
        c.write_all(&vec![1u8; 100_000]).await.unwrap();
        server.await.unwrap();
        let max = largest.load(Ordering::Relaxed);
        assert!(max > 1000 && max <= 1280, "largest packet {max}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tcp_and_udp_between_two_stacks() {
        let a = Netstack::new();
        let b = Netstack::new();
        a.configure(vec![IpNet::parse("10.0.0.1/24").unwrap()], 1400);
        b.configure(vec![IpNet::parse("10.0.0.2/24").unwrap()], 1400);
        a.set_sink(Some(Arc::new(Wire(b.clone()))));
        b.set_sink(Some(Arc::new(Wire(a.clone()))));

        // A listener on b, written directly against smoltcp.
        let listen = {
            let mut st = lock(&b.inner.stack);
            let mut s =
                tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 65536]), tcp::SocketBuffer::new(vec![0; 65536]));
            s.listen(8080).unwrap();
            let h = st.sockets.add(s);
            b.inner.tcp_open.fetch_add(1, Ordering::Relaxed);
            TcpStream { inner: b.inner.clone(), handle: h, closed: false }
        };
        let server = tokio::spawn(async move {
            let mut listen = listen;
            poll_fn(|cx| {
                let mut st = lock(&listen.inner.stack);
                let s = st.sockets.get_mut::<tcp::Socket>(listen.handle);
                if s.may_recv() {
                    return Poll::Ready(());
                }
                s.register_recv_waker(cx.waker());
                Poll::Pending
            })
            .await;
            let mut got = vec![0u8; 5];
            listen.read_exact(&mut got).await.unwrap();
            assert_eq!(&got, b"hello");
            let big = vec![7u8; 300_000];
            listen.write_all(&big).await.unwrap();
            listen.shutdown().await.unwrap();
        });
        let mut c = a.tcp_connect("10.0.0.2:8080".parse().unwrap(), Duration::from_secs(5)).await.unwrap();
        c.write_all(b"hello").await.unwrap();
        let mut back = Vec::new();
        c.read_to_end(&mut back).await.unwrap();
        assert_eq!(back.len(), 300_000);
        server.await.unwrap();
        assert!(a.counters().0 > 300_000 && a.counters().1 > 0);

        // Nobody listens on 81: the peer answers with RST.
        let e = a.tcp_connect("10.0.0.2:81".parse().unwrap(), Duration::from_secs(5)).await.err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused);
        // No IPv6 address: unreachable.
        let e = a.tcp_connect("[fd00::1]:80".parse().unwrap(), Duration::from_secs(1)).await.err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::AddrNotAvailable);

        // UDP echo on b.
        let echo = {
            let mut st = lock(&b.inner.stack);
            let mut s = udp::Socket::new(
                udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 4096]),
                udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 4096]),
            );
            s.bind(53).unwrap();
            UdpSock { inner: b.inner.clone(), handle: st.sockets.add(s) }
        };
        let echo_task = tokio::spawn(async move {
            poll_fn(|cx| {
                let mut st = lock(&echo.inner.stack);
                let s = st.sockets.get_mut::<udp::Socket>(echo.handle);
                if let Ok((d, m)) = s.recv() {
                    let d = d.to_vec();
                    s.send_slice(&d, m.endpoint).unwrap();
                    drop(st);
                    echo.inner.wake.notify_one();
                    return Poll::Ready(());
                }
                s.register_recv_waker(cx.waker());
                Poll::Pending
            })
            .await;
            echo
        });
        let r = a.udp_exchange("10.0.0.2:53".parse().unwrap(), b"ping", Duration::from_secs(5)).await.unwrap();
        assert_eq!(r, b"ping");
        drop(echo_task.await.unwrap());
        let e = a.udp_exchange("10.0.0.2:54".parse().unwrap(), b"x", Duration::from_millis(300)).await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);

        // Reconfiguring to a new address resets connections.
        a.configure(vec![IpNet::parse("10.0.0.9/24").unwrap()], 1400);
        assert_eq!(a.addresses()[0].to_string(), "10.0.0.9/32");
        a.close();
        b.close();
    }

    /// A listening socket on `b`, port `port`, as a stream once a client is in.
    fn listen(b: &Netstack, port: u16) -> TcpStream {
        let mut st = lock(&b.inner.stack);
        let mut s = tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 65536]), tcp::SocketBuffer::new(vec![0; 65536]));
        s.listen(port).unwrap();
        let h = st.sockets.add(s);
        b.inner.tcp_open.fetch_add(1, Ordering::Relaxed);
        TcpStream { inner: b.inner.clone(), handle: h, closed: false }
    }

    async fn accepted(s: &TcpStream) {
        poll_fn(|cx| {
            let mut st = lock(&s.inner.stack);
            let sock = st.sockets.get_mut::<tcp::Socket>(s.handle);
            if sock.state() == tcp::State::Established {
                return Poll::Ready(());
            }
            sock.register_recv_waker(cx.waker());
            Poll::Pending
        })
        .await;
    }

    /// A reset (the peer's RST, or the stack aborting the connection) is an error for the
    /// reader, never a clean end of the stream.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reset_is_not_a_clean_end() {
        let a = Netstack::new();
        let b = Netstack::new();
        a.configure(vec![IpNet::parse("10.0.0.1/24").unwrap()], 1400);
        b.configure(vec![IpNet::parse("10.0.0.2/24").unwrap()], 1400);
        a.set_sink(Some(Arc::new(Wire(b.clone()))));
        b.set_sink(Some(Arc::new(Wire(a.clone()))));

        // The server writes a little and then aborts (RST).
        let server = listen(&b, 8080);
        let task = tokio::spawn(async move {
            let mut server = server;
            accepted(&server).await;
            server.write_all(b"partial").await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(server); // not shut down: abort
        });
        let mut c = a.tcp_connect("10.0.0.2:8080".parse().unwrap(), Duration::from_secs(5)).await.unwrap();
        let mut got = Vec::new();
        let e = tokio::time::timeout(Duration::from_secs(5), c.read_to_end(&mut got)).await.unwrap().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionReset);
        assert_eq!(got, b"partial");
        task.await.unwrap();

        // The server ends cleanly (FIN): a clean end.
        let server = listen(&b, 8081);
        let task = tokio::spawn(async move {
            let mut server = server;
            accepted(&server).await;
            server.write_all(b"whole").await.unwrap();
            server.shutdown().await.unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
        });
        let mut c = a.tcp_connect("10.0.0.2:8081".parse().unwrap(), Duration::from_secs(5)).await.unwrap();
        let mut got = Vec::new();
        c.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"whole");
        task.await.unwrap();

        // The tunnel goes away (the stack aborts everything): a reset too.
        let server = listen(&b, 8082);
        let task = tokio::spawn(async move {
            let s = server;
            accepted(&s).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let mut c = a.tcp_connect("10.0.0.2:8082".parse().unwrap(), Duration::from_secs(5)).await.unwrap();
        abort_all(&mut lock(&a.inner.stack));
        let mut buf = [0u8; 8];
        let e = c.read(&mut buf).await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionReset);
        task.abort();
        a.close();
        b.close();
    }

    #[test]
    fn the_queue_is_capped_in_bytes_too() {
        let mut d = QueueDevice { rx: VecDeque::new(), rx_queued: 0, tx: Vec::new(), mtu: 1420 };
        let big = vec![0u8; 65535];
        let mut n = 0;
        while d.push_rx(&big) {
            n += 1;
        }
        assert_eq!(n, RX_QUEUE_BYTES / 65535);
        assert!(d.rx_queued <= RX_QUEUE_BYTES);
        // Small packets still fit until the count cap.
        assert!(d.push_rx(&[1, 2, 3]));
        let _ = d.receive(SmolInstant::from_millis(0));
        assert_eq!(d.rx_queued, (n - 1) * 65535 + 3);
        let mut small = QueueDevice { rx: VecDeque::new(), rx_queued: 0, tx: Vec::new(), mtu: 1420 };
        let mut m = 0;
        while small.push_rx(&[0u8; 20]) {
            m += 1;
        }
        assert_eq!(m, RX_QUEUE);
    }
}
