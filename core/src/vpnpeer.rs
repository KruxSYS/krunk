//! The VPN proxy's owner check (0.14.0), without the Windows calls: the host gives it the TCP
//! table, the process list, creation times and image paths through [`System`].
//!
//! A connection to the proxy is let in only when its client end belongs to the browser process
//! itself, or to a descendant of it whose image, and the image of every process between the
//! two, is Krunk's own exe (the engine's helper processes are `Krunk.exe` subprocesses). Anything
//! the browser starts through the shell (a downloaded file opened, a mail program, an external
//! protocol handler, an installer) is a child with another image and never passes, nor does a
//! `Krunk.exe` that such a program started.
//!
//! The client end is found by the full 4-tuple of an ESTABLISHED row of the TCP table: local =
//! the peer's address, remote = the proxy's own end. Every link of the parent chain must have
//! started after its parent, so a parent id reused by Windows doesn't count. Verdicts are cached
//! by `(pid, creation time)`: a hit is re-checked against the process's creation time now, so a
//! reused pid never inherits a verdict. Refusals are cached briefly (a program knocking in a
//! loop costs little).

use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a process found to be Krunk's stays trusted, and how long a refusal holds.
pub const ALLOWED_TTL: Duration = Duration::from_secs(60);
pub const REFUSED_TTL: Duration = Duration::from_secs(2);
const CACHE_MAX: usize = 64;
/// The longest parent chain looked at.
const MAX_CHAIN: usize = 32;

/// One IPv4 row of the TCP table with its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TcpRow {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub established: bool,
    pub pid: u32,
}

/// What the check needs from the operating system.
pub trait System {
    /// The IPv4 TCP table, `None` if it could not be read.
    fn tcp_rows(&self) -> Option<Vec<TcpRow>>;
    /// `(pid, parent pid)` of every process.
    fn processes(&self) -> Vec<(u32, u32)>;
    /// When `pid` was created (any monotonic unit, e.g. FILETIME ticks); `None` if it is gone.
    fn created(&self, pid: u32) -> Option<u64>;
    /// The full path of `pid`'s executable.
    fn image(&self, pid: u32) -> Option<String>;
}

/// The process that owns the client end of the connection `peer` → `proxy`: the one
/// ESTABLISHED row with local = `peer` and remote = `proxy`. `None` when there is no such row,
/// or rows of more than one process.
pub fn owner(rows: &[TcpRow], proxy: SocketAddr, peer: SocketAddr) -> Option<u32> {
    let (SocketAddr::V4(proxy), SocketAddr::V4(peer)) = (proxy, peer) else { return None };
    let mut found = None;
    for r in rows.iter().filter(|r| r.established && r.local == peer && r.remote == proxy) {
        match found {
            None => found = Some(r.pid),
            Some(p) if p == r.pid => {}
            Some(_) => return None,
        }
    }
    found
}

/// Is the path `a` the same file name as `b` (Windows paths: case-insensitive, either slash)?
fn same_image(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().replace('/', "\\").to_lowercase();
    !a.trim().is_empty() && norm(a) == norm(b)
}

/// Why a connection was refused (for the log; never more than a process id and a file name).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not from loopback, or not IPv4.
    NotLoopback,
    /// No single ESTABLISHED row for the connection.
    NoOwner,
    /// The process is gone.
    Gone(u32),
    /// Not a descendant of the browser process (or a link started before its parent).
    NotDescendant(u32),
    /// A descendant, but it (or a process between) runs another program: the file name.
    OtherImage(u32, String),
    /// Refused a moment ago.
    Cached(u32),
}

#[derive(Default)]
struct Cache {
    allowed: Vec<((u32, u64), Instant)>,
    refused: Vec<((u32, u64), Instant)>,
}

impl Cache {
    fn hit(list: &mut Vec<((u32, u64), Instant)>, key: (u32, u64), ttl: Duration, now: Instant) -> bool {
        list.retain(|(_, at)| now.saturating_duration_since(*at) < ttl);
        list.iter().any(|(k, _)| *k == key)
    }

    fn put(list: &mut Vec<((u32, u64), Instant)>, key: (u32, u64), now: Instant) {
        list.retain(|(k, _)| *k != key);
        if list.len() >= CACHE_MAX {
            list.remove(0);
        }
        list.push((key, now));
    }
}

/// The owner check of one browser process.
pub struct PeerCheck {
    me: u32,
    me_created: u64,
    /// The browser process's own image (`Krunk.exe`).
    image: String,
    cache: Mutex<Cache>,
}

impl PeerCheck {
    /// The check for the browser process `me`; `None` when its own creation time or image
    /// can't be read (then nothing would pass).
    pub fn new(me: u32, sys: &dyn System) -> Option<PeerCheck> {
        let me_created = sys.created(me)?;
        let image = sys.image(me).filter(|i| !i.trim().is_empty())?;
        Some(PeerCheck { me, me_created, image, cache: Mutex::new(Cache::default()) })
    }

    /// May the connection from `peer` to the proxy's end `proxy` use the proxy?
    pub fn check(&self, sys: &dyn System, proxy: SocketAddr, peer: SocketAddr, now: Instant) -> Result<(), Refusal> {
        let loopback = |a: &SocketAddr| matches!(a.ip(), IpAddr::V4(ip) if ip.is_loopback());
        if !loopback(&peer) || !loopback(&proxy) {
            return Err(Refusal::NotLoopback);
        }
        let rows = sys.tcp_rows().ok_or(Refusal::NoOwner)?;
        let pid = owner(&rows, proxy, peer).ok_or(Refusal::NoOwner)?;
        if pid == self.me {
            return Ok(());
        }
        // The creation time now: a cached verdict holds only for this very process.
        let created = sys.created(pid).ok_or(Refusal::Gone(pid))?;
        let key = (pid, created);
        {
            let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if Cache::hit(&mut c.allowed, key, ALLOWED_TTL, now) {
                return Ok(());
            }
            if Cache::hit(&mut c.refused, key, REFUSED_TTL, now) {
                return Err(Refusal::Cached(pid));
            }
        }
        let verdict = self.examine(sys, pid);
        // The process may have ended (and its id been reused) while it was looked at.
        let verdict = match (verdict, sys.created(pid)) {
            (v, Some(c)) if c == created => v,
            _ => Err(Refusal::Gone(pid)),
        };
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        match &verdict {
            Ok(()) => Cache::put(&mut c.allowed, key, now),
            Err(Refusal::Gone(_)) => {}
            Err(_) => Cache::put(&mut c.refused, key, now),
        }
        verdict
    }

    /// The full check of `pid`: a descendant of the browser process, each link started after
    /// its parent and running Krunk's own exe.
    fn examine(&self, sys: &dyn System, pid: u32) -> Result<(), Refusal> {
        let procs = sys.processes();
        let chain = crate::memory::chain_to(pid, self.me, &procs).ok_or(Refusal::NotDescendant(pid))?;
        if chain.len() > MAX_CHAIN {
            return Err(Refusal::NotDescendant(pid));
        }
        let mut parent_start = self.me_created;
        // From the child of the browser process down to `pid`.
        for p in chain.iter().rev() {
            match sys.created(*p) {
                Some(t) if t >= parent_start => parent_start = t,
                Some(_) => return Err(Refusal::NotDescendant(pid)),
                None => return Err(Refusal::Gone(pid)),
            }
            let image = sys.image(*p).unwrap_or_default();
            if !same_image(&image, &self.image) {
                let file = image.rsplit(['\\', '/']).next().unwrap_or("").chars().take(64).collect();
                return Err(Refusal::OtherImage(*p, file));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::net::Ipv4Addr;

    const KRUNK: &str = r"C:\Program Files\Krunk\Krunk.exe";
    const PROXY: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 53127));

    #[derive(Default)]
    struct Fake {
        rows: Vec<TcpRow>,
        procs: Vec<(u32, u32)>,
        created: RefCell<HashMap<u32, u64>>,
        images: HashMap<u32, String>,
        table_reads: RefCell<u32>,
        image_reads: RefCell<u32>,
    }

    impl System for Fake {
        fn tcp_rows(&self) -> Option<Vec<TcpRow>> {
            *self.table_reads.borrow_mut() += 1;
            Some(self.rows.clone())
        }
        fn processes(&self) -> Vec<(u32, u32)> {
            self.procs.clone()
        }
        fn created(&self, pid: u32) -> Option<u64> {
            self.created.borrow().get(&pid).copied()
        }
        fn image(&self, pid: u32) -> Option<String> {
            *self.image_reads.borrow_mut() += 1;
            self.images.get(&pid).cloned()
        }
    }

    fn sa(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn row(client_port: u16, pid: u32) -> TcpRow {
        let SocketAddr::V4(proxy) = PROXY else { unreachable!() };
        TcpRow { local: SocketAddrV4::new(Ipv4Addr::LOCALHOST, client_port), remote: proxy, established: true, pid }
    }

    /// The browser (100) with a network service (101), a renderer (102), a downloaded program
    /// it opened (200) and a Krunk.exe that program started (201); 300 is unrelated.
    fn world() -> Fake {
        let mut f =
            Fake { procs: vec![(100, 4), (101, 100), (102, 100), (200, 100), (201, 200), (300, 4)], ..Fake::default() };
        for (pid, t) in [(100, 1000), (101, 1100), (102, 1200), (200, 1300), (201, 1400), (300, 50)] {
            f.created.borrow_mut().insert(pid, t);
        }
        for pid in [100, 101, 102, 201] {
            f.images.insert(pid, KRUNK.into());
        }
        f.images.insert(200, r"C:\Users\me\Downloads\setup.exe".into());
        f.images.insert(300, r"C:\Windows\System32\curl.exe".into());
        f.rows = vec![
            row(50001, 101),
            row(50002, 102),
            row(50003, 200),
            row(50004, 201),
            row(50005, 300),
            row(50006, 100),
            // The proxy's own end of each connection (local = proxy) never counts.
            TcpRow {
                local: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 53127),
                remote: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 50005),
                established: true,
                pid: 100,
            },
        ];
        f
    }

    #[test]
    fn the_owner_is_the_full_4_tuple_of_an_established_row() {
        let f = world();
        assert_eq!(owner(&f.rows, PROXY, sa(50001)), Some(101));
        // The same client port towards another server is another connection.
        assert_eq!(owner(&f.rows, sa(9999), sa(50001)), None);
        // The proxy's side of a connection (Krunk's row, local = proxy) is not its client.
        assert_eq!(owner(&f.rows, PROXY, sa(50005)), Some(300));
        let mut rows = f.rows.clone();
        rows.push(TcpRow { established: false, ..row(50010, 101) });
        assert_eq!(owner(&rows, PROXY, sa(50010)), None);
        // Two processes claim it: nobody.
        rows.push(row(50011, 101));
        rows.push(row(50011, 300));
        assert_eq!(owner(&rows, PROXY, sa(50011)), None);
        let v6: SocketAddr = "[::1]:50001".parse().unwrap();
        assert_eq!(owner(&rows, PROXY, v6), None);
    }

    #[test]
    fn only_krunk_and_its_own_subprocesses_pass() {
        let f = world();
        let c = PeerCheck::new(100, &f).unwrap();
        let now = Instant::now();
        assert_eq!(c.check(&f, PROXY, sa(50001), now), Ok(()), "network service");
        assert_eq!(c.check(&f, PROXY, sa(50002), now), Ok(()), "renderer");
        assert_eq!(c.check(&f, PROXY, sa(50006), now), Ok(()), "the browser itself");
        assert_eq!(c.check(&f, PROXY, sa(50003), now), Err(Refusal::OtherImage(200, "setup.exe".into())));
        // Krunk.exe, but started by the downloaded program.
        assert_eq!(c.check(&f, PROXY, sa(50004), now), Err(Refusal::OtherImage(200, "setup.exe".into())));
        assert_eq!(c.check(&f, PROXY, sa(50005), now), Err(Refusal::NotDescendant(300)));
        assert_eq!(c.check(&f, PROXY, sa(50099), now), Err(Refusal::NoOwner));
        let lan: SocketAddr = "192.168.1.5:50001".parse().unwrap();
        assert_eq!(c.check(&f, PROXY, lan, now), Err(Refusal::NotLoopback));
        // Images compare like Windows paths.
        assert!(same_image(r"c:\program files\krunk\KRUNK.EXE", KRUNK));
        assert!(same_image("C:/Program Files/Krunk/Krunk.exe", KRUNK));
        assert!(!same_image("", KRUNK));
        assert!(!same_image(r"C:\Program Files\Krunk\Krunk.exe.evil", KRUNK));
    }

    #[test]
    fn a_child_older_than_its_parent_is_a_reused_parent_id() {
        let f = world();
        f.created.borrow_mut().insert(101, 900); // "child" of 100 but older than it
        let c = PeerCheck::new(100, &f).unwrap();
        assert_eq!(c.check(&f, PROXY, sa(50001), Instant::now()), Err(Refusal::NotDescendant(101)));
    }

    #[test]
    fn verdicts_are_cached_by_pid_and_creation_time() {
        let f = world();
        let c = PeerCheck::new(100, &f).unwrap();
        let t0 = Instant::now();
        assert_eq!(c.check(&f, PROXY, sa(50001), t0), Ok(()));
        let reads = *f.image_reads.borrow();
        assert_eq!(c.check(&f, PROXY, sa(50001), t0 + Duration::from_secs(1)), Ok(()));
        assert_eq!(*f.image_reads.borrow(), reads, "a cache hit reads no image");
        // Every connection still looks its own row up.
        assert_eq!(*f.table_reads.borrow(), 2);

        // Pid 101 ends and Windows gives the id to another program: the cached verdict is
        // not its.
        let mut f2 = world();
        f2.created.borrow_mut().insert(101, 5000);
        f2.images.insert(101, r"C:\evil.exe".into());
        assert_eq!(
            c.check(&f2, PROXY, sa(50001), t0 + Duration::from_secs(2)),
            Err(Refusal::OtherImage(101, "evil.exe".into()))
        );

        // A refusal holds briefly, then the process is looked at again.
        let t1 = t0 + Duration::from_secs(10);
        assert!(matches!(c.check(&f, PROXY, sa(50005), t1), Err(Refusal::NotDescendant(300))));
        let reads = *f.image_reads.borrow();
        assert_eq!(c.check(&f, PROXY, sa(50005), t1 + Duration::from_millis(500)), Err(Refusal::Cached(300)));
        assert_eq!(*f.image_reads.borrow(), reads);
        assert!(matches!(c.check(&f, PROXY, sa(50005), t1 + REFUSED_TTL), Err(Refusal::NotDescendant(300))));

        // Allowed verdicts expire too.
        let reads = *f.image_reads.borrow();
        assert_eq!(c.check(&f, PROXY, sa(50001), t0 + ALLOWED_TTL + Duration::from_secs(1)), Ok(()));
        assert!(*f.image_reads.borrow() > reads);
    }

    #[test]
    fn a_process_gone_meanwhile_is_refused_and_not_cached() {
        let mut f = world();
        f.created.borrow_mut().remove(&102);
        let c = PeerCheck::new(100, &f).unwrap();
        assert_eq!(c.check(&f, PROXY, sa(50002), Instant::now()), Err(Refusal::Gone(102)));
        f.created.borrow_mut().insert(102, 1200);
        assert_eq!(c.check(&f, PROXY, sa(50002), Instant::now()), Ok(()));
        // Without its own image the check can't work: nothing is created.
        f.images.remove(&100);
        assert!(PeerCheck::new(100, &f).is_none());
    }
}
