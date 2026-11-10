//! Custom DNS (Krunk 0.6.0): the engine resolves through Chromium's own DNS-over-HTTPS client
//! in secure mode (never the system resolver), and that client is pointed at either the
//! user's DoH resolvers directly or at a loopback stub this module runs. The stub answers
//! `https://127.0.0.1:<port>/dns-query` (RFC 8484, GET or POST) by asking the user's ordered
//! resolver list — UDP, TCP, DoT, DoH (h2 and h3) and DoQ through `hickory-proto` — with a
//! per-resolver timeout and fallback to the next. Its certificate is generated in memory at
//! every start (P-256, never written to disk) and trusted only through the engine's
//! `--ignore-certificate-errors-spki-list` switch. A small tokio runtime on its own thread
//! does the work; nothing here is `unsafe`.

use crate::log::{info, warning};
use hickory_proto::op::Message;
use hickory_proto::runtime::{TokioRuntimeProvider, TokioTime};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use hickory_proto::xfer::{DnsExchange, DnsHandle, DnsMultiplexer, DnsRequest, DnsRequestOptions, FirstAnswer};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Request, Response, StatusCode};
use lb_core::dns::{Dns, Kind, Resolver, RESOLVER_TIMEOUT_MS};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// The stub's certificate and key for this run, made before the engine starts (the SPKI
/// hash goes on its command line).
pub struct Identity {
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
    /// Base64 of SHA-256 over the SubjectPublicKeyInfo (what the engine's switch wants).
    pub spki_sha256: String,
}

static IDENTITY: OnceLock<Identity> = OnceLock::new();

/// Generates (once) the stub's identity; `None` if the crypto failed.
pub fn identity() -> Option<&'static Identity> {
    if let Some(i) = IDENTITY.get() {
        return Some(i);
    }
    let key = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string(), "localhost".to_string()]).ok()?;
    let spki = key.key_pair.public_key_der();
    let digest = ring::digest::digest(&ring::digest::SHA256, &spki);
    let spki_sha256 = lb_core::dns::base64(digest.as_ref());
    let id = Identity { cert_der: key.cert.der().to_vec(), key_der: key.key_pair.serialize_der(), spki_sha256 };
    let _ = IDENTITY.set(id);
    IDENTITY.get()
}

/// The engine switch that trusts the stub's key (and nothing else) for this run.
pub fn engine_switch() -> Option<String> {
    identity().map(|i| format!("--ignore-certificate-errors-spki-list={}", i.spki_sha256))
}

/// What the engine's DoH client is pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Chromium's default: the system resolver, upgraded to DoH when it is known to have it.
    System,
    /// The user's DoH resolvers, given to Chromium as they are (every entry is DoH over h2).
    Direct(Vec<String>),
    /// The loopback stub.
    Stub(String),
}

impl Route {
    /// `(dns_over_https.mode, dns_over_https.templates)`.
    pub fn prefs(&self) -> (&'static str, String) {
        match self {
            Route::System => ("automatic", String::new()),
            Route::Direct(t) => ("secure", t.join(" ")),
            Route::Stub(t) => ("secure", t.clone()),
        }
    }
}

/// Per-resolver counters for the settings page.
#[derive(Default)]
struct Stat {
    queries: AtomicU64,
    failures: AtomicU64,
    last_ms: AtomicU64,
    last_error: Mutex<String>,
}

struct Shared {
    config: watch::Receiver<Arc<Vec<Resolver>>>,
    /// Open connections per resolver index (dropped and remade after an error).
    conns: tokio::sync::Mutex<HashMap<usize, DnsExchange>>,
    stats: Mutex<Vec<Arc<Stat>>>,
    tls: Arc<rustls::ClientConfig>,
    /// 0.14.0: the VPN runs: every query is refused (nothing goes over the system's network).
    refusing: AtomicBool,
}

/// The running resolver: the stub on `port`, serving the current list.
pub struct Engine {
    handle: tokio::runtime::Handle,
    shared: Arc<Shared>,
    config: watch::Sender<Arc<Vec<Resolver>>>,
    pub port: u16,
}

/// Starts the runtime and the stub; `None` if the port or the TLS setup failed.
pub fn start(dns: &Dns) -> Option<Engine> {
    let id = identity()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("krunk-dns")
        .build()
        .ok()?;
    let handle = runtime.handle().clone();
    // The runtime lives as long as the process: its thread parks on it.
    std::thread::Builder::new()
        .name("krunk-dns-rt".into())
        .spawn(move || runtime.block_on(std::future::pending::<()>()))
        .ok()?;
    let (config, rx) = watch::channel(Arc::new(usable(dns)));
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = Arc::new(rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth());
    let shared = Arc::new(Shared {
        config: rx,
        conns: tokio::sync::Mutex::new(HashMap::new()),
        stats: Mutex::new(usable(dns).iter().map(|_| Arc::new(Stat::default())).collect()),
        tls,
        refusing: AtomicBool::new(false),
    });
    let cert = rustls::pki_types::CertificateDer::from(id.cert_der.clone());
    let key = rustls::pki_types::PrivateKeyDer::try_from(id.key_der.clone()).ok()?;
    let mut server = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(vec![cert], key).ok()?;
    server.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    let listener = handle.block_on(tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))).ok()?;
    let port = listener.local_addr().ok()?.port();
    let shared2 = shared.clone();
    handle.spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let acceptor = acceptor.clone();
            let shared = shared2.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else { return };
                let io = hyper_util::rt::TokioIo::new(tls);
                let service = hyper::service::service_fn(move |req| handle_http(shared.clone(), req));
                let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    info!("dns: loopback DoH stub on 127.0.0.1:{port} with {} resolver(s)", config.borrow().len());
    Some(Engine { handle, shared, config, port })
}

/// The supported resolvers of the list, in order.
fn usable(dns: &Dns) -> Vec<Resolver> {
    dns.resolvers.iter().filter(|r| r.kind.supported()).cloned().collect()
}

impl Engine {
    pub fn template(&self) -> String {
        format!("https://127.0.0.1:{}/dns-query", self.port)
    }

    /// A new list: connections are remade on the next query.
    pub fn configure(&self, dns: &Dns) {
        let list = usable(dns);
        *self.shared.stats.lock().unwrap_or_else(|e| e.into_inner()) =
            list.iter().map(|_| Arc::new(Stat::default())).collect();
        let _ = self.config.send(Arc::new(list));
        let shared = self.shared.clone();
        self.handle.spawn(async move { shared.conns.lock().await.clear() });
    }

    /// While the VPN runs every query gets REFUSED (and the Test button an error); open
    /// connections to the resolvers are dropped.
    pub fn set_refusing(&self, on: bool) {
        if self.shared.refusing.swap(on, Relaxed) != on && on {
            let shared = self.shared.clone();
            self.handle.spawn(async move { shared.conns.lock().await.clear() });
        }
    }

    /// A handle for the Test button's worker thread.
    pub fn tester(&self) -> Tester {
        Tester { handle: self.handle.clone(), shared: self.shared.clone() }
    }

    /// The counters for the settings page.
    pub fn status(&self) -> serde_json::Value {
        let stats = self.shared.stats.lock().unwrap_or_else(|e| e.into_inner());
        let rows: Vec<serde_json::Value> = stats
            .iter()
            .map(|s| {
                serde_json::json!({
                    "queries": s.queries.load(Relaxed),
                    "failures": s.failures.load(Relaxed),
                    "lastMs": s.last_ms.load(Relaxed),
                    "lastError": s.last_error.lock().map(|e| e.clone()).unwrap_or_default(),
                })
            })
            .collect();
        serde_json::json!({ "port": self.port, "resolvers": rows })
    }
}

async fn handle_http(
    shared: Arc<Shared>,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let reply = |status: StatusCode, body: Vec<u8>, kind: &str| {
        Response::builder()
            .status(status)
            .header("content-type", kind)
            .header("cache-control", "no-store")
            .body(Full::new(Bytes::from(body)))
            .unwrap_or_default()
    };
    if req.uri().path() != "/dns-query" {
        return Ok(reply(StatusCode::NOT_FOUND, b"not found".to_vec(), "text/plain"));
    }
    let query: Option<Vec<u8>> = if req.method() == hyper::Method::GET {
        req.uri()
            .query()
            .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("dns=")))
            .and_then(|b| lb_core::dns::base64url_decode(b).ok())
    } else {
        let body = req.into_body().collect().await?.to_bytes();
        (body.len() <= 65_535).then(|| body.to_vec())
    };
    let Some(query) = query.filter(|q| q.len() >= 12) else {
        return Ok(reply(StatusCode::BAD_REQUEST, b"bad request".to_vec(), "text/plain"));
    };
    if shared.refusing.load(Relaxed) {
        return Ok(match lb_core::dns::refused_answer(&query) {
            Some(answer) => reply(StatusCode::OK, answer, "application/dns-message"),
            None => reply(StatusCode::BAD_GATEWAY, b"refused while the VPN runs".to_vec(), "text/plain"),
        });
    }
    match resolve(&shared, &query).await {
        Ok(answer) => Ok(reply(StatusCode::OK, answer, "application/dns-message")),
        Err(e) => Ok(reply(StatusCode::BAD_GATEWAY, e.into_bytes(), "text/plain")),
    }
}

/// Asks the resolvers in order; the first answer wins.
async fn resolve(shared: &Arc<Shared>, query: &[u8]) -> Result<Vec<u8>, String> {
    let list = shared.config.borrow().clone();
    let mut last = String::from("no resolver configured");
    for (i, r) in list.iter().enumerate() {
        match query_one(shared, i, r, query).await {
            Ok(a) => return Ok(a),
            Err(e) => {
                if i + 1 < list.len() {
                    info!("dns: {} failed ({e}); trying the next resolver", r.address());
                }
                last = format!("{}: {e}", r.address());
            }
        }
    }
    Err(last)
}

/// One query through one resolver, with its connection kept for the next query.
async fn query_one(shared: &Arc<Shared>, index: usize, r: &Resolver, query: &[u8]) -> Result<Vec<u8>, String> {
    let stat = shared.stats.lock().ok().and_then(|s| s.get(index).cloned());
    let t = Instant::now();
    let result = tokio::time::timeout(Duration::from_millis(RESOLVER_TIMEOUT_MS), async {
        let exchange = {
            let mut conns = shared.conns.lock().await;
            match conns.get(&index) {
                Some(e) => e.clone(),
                None => {
                    let e = connect(shared, r).await?;
                    conns.insert(index, e.clone());
                    e
                }
            }
        };
        let msg = Message::from_bytes(query).map_err(|e| format!("bad query: {e}"))?;
        let id = msg.id();
        let request = DnsRequest::new(msg, DnsRequestOptions::default());
        // A page start fires dozens of queries at once; a connection takes 32 in flight and
        // says "busy" for the rest, which just wait a moment (the 2 s budget still applies).
        let response = loop {
            match exchange.send(request.clone()).first_answer().await {
                Ok(r) => break r,
                Err(e) if e.is_busy() => tokio::time::sleep(Duration::from_millis(20)).await,
                Err(e) => return Err(e.to_string()),
            }
        };
        let mut answer = response.into_message();
        answer.set_id(id);
        answer.to_bytes().map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|_| Err(format!("no answer within {RESOLVER_TIMEOUT_MS} ms")));
    if let Some(s) = stat {
        s.queries.fetch_add(1, Relaxed);
        s.last_ms.store(t.elapsed().as_millis() as u64, Relaxed);
        match &result {
            Ok(_) => {
                if let Ok(mut e) = s.last_error.lock() {
                    e.clear();
                }
            }
            Err(e) => {
                s.failures.fetch_add(1, Relaxed);
                if let Ok(mut l) = s.last_error.lock() {
                    *l = e.clone();
                }
            }
        }
    }
    if result.is_err() {
        shared.conns.lock().await.remove(&index);
    }
    result
}

/// The socket address of a resolver: its IP, or one lookup of its name through the system
/// resolver (logged: it is the only time Windows' resolver is used).
async fn socket_addr(r: &Resolver) -> Result<SocketAddr, String> {
    if let Some(a) = r.socket_addr() {
        return Ok(a);
    }
    let name = r.host.clone();
    let mut addrs =
        tokio::net::lookup_host((name.as_str(), r.port)).await.map_err(|e| format!("lookup of {name}: {e}"))?;
    let addr = addrs.next().ok_or_else(|| format!("{name} has no address"))?;
    info!("dns: {name} looked up through the system resolver (bootstrap): {}", addr.ip());
    Ok(addr)
}

async fn connect(shared: &Arc<Shared>, r: &Resolver) -> Result<DnsExchange, String> {
    let addr = socket_addr(r).await?;
    let name = r.server_name().unwrap_or_else(|| addr.ip().to_string());
    let provider = TokioRuntimeProvider::new();
    let timeout = Duration::from_millis(RESOLVER_TIMEOUT_MS);
    let (exchange, background): (DnsExchange, tokio::task::JoinHandle<()>) = match r.kind {
        Kind::Udp => {
            let conn = hickory_proto::udp::UdpClientStream::builder(addr, provider).with_timeout(Some(timeout)).build();
            let (e, bg) = DnsExchange::connect::<_, _, TokioTime>(conn).await.map_err(|e| e.to_string())?;
            (
                e,
                tokio::spawn(async move {
                    let _ = bg.await;
                }),
            )
        }
        Kind::Tcp => {
            let (stream, handle) = hickory_proto::tcp::TcpClientStream::new(addr, None, Some(timeout), provider);
            let mp = DnsMultiplexer::with_timeout(stream, handle, timeout, None);
            let (e, bg) = DnsExchange::connect::<_, _, TokioTime>(mp).await.map_err(|e| e.to_string())?;
            (
                e,
                tokio::spawn(async move {
                    let _ = bg.await;
                }),
            )
        }
        Kind::Tls => {
            let (stream, handle) = hickory_proto::rustls::tls_client_connect(addr, name, shared.tls.clone(), provider);
            let mp = DnsMultiplexer::with_timeout(stream, handle, timeout, None);
            let (e, bg) = DnsExchange::connect::<_, _, TokioTime>(mp).await.map_err(|e| e.to_string())?;
            (
                e,
                tokio::spawn(async move {
                    let _ = bg.await;
                }),
            )
        }
        Kind::Https => {
            let conn = hickory_proto::h2::HttpsClientStreamBuilder::with_client_config(shared.tls.clone(), provider)
                .build(addr, name, r.path.clone());
            let (e, bg) = DnsExchange::connect::<_, _, TokioTime>(conn).await.map_err(|e| e.to_string())?;
            (
                e,
                tokio::spawn(async move {
                    let _ = bg.await;
                }),
            )
        }
        Kind::Quic => {
            let mut b = hickory_proto::quic::QuicClientStream::builder();
            b.crypto_config((*shared.tls).clone());
            let conn = b.build(addr, name);
            let (e, bg) = DnsExchange::connect::<_, _, TokioTime>(conn).await.map_err(|e| e.to_string())?;
            (
                e,
                tokio::spawn(async move {
                    let _ = bg.await;
                }),
            )
        }
        Kind::H3 => {
            let mut b = hickory_proto::h3::H3ClientStream::builder();
            b.crypto_config((*shared.tls).clone());
            let conn = b.build(addr, name, r.path.clone());
            let (e, bg) = DnsExchange::connect::<_, _, TokioTime>(conn).await.map_err(|e| e.to_string())?;
            (
                e,
                tokio::spawn(async move {
                    let _ = bg.await;
                }),
            )
        }
        Kind::DnsCrypt => return Err("DNSCrypt is not supported".into()),
    };
    drop(background);
    Ok(exchange)
}

/// Runs a resolver test off the UI thread.
#[derive(Clone)]
pub struct Tester {
    handle: tokio::runtime::Handle,
    shared: Arc<Shared>,
}

impl Tester {
    /// Resolves `example.com` through resolver `index` alone; the latency or the error.
    pub fn test(&self, index: usize) -> Result<u32, String> {
        let shared = self.shared.clone();
        self.handle.block_on(async move {
            if shared.refusing.load(Relaxed) {
                return Err("Custom DNS is not used while the VPN runs (names are resolved inside the tunnel)".into());
            }
            let list = shared.config.borrow().clone();
            let r = list.get(index).ok_or("No such resolver")?.clone();
            let mut q = Message::new();
            q.set_id(0x4b52)
                .set_message_type(hickory_proto::op::MessageType::Query)
                .set_op_code(hickory_proto::op::OpCode::Query)
                .set_recursion_desired(true)
                .add_query(hickory_proto::op::Query::query(
                    hickory_proto::rr::Name::from_ascii("example.com.").map_err(|e| e.to_string())?,
                    hickory_proto::rr::RecordType::A,
                ));
            let bytes = q.to_bytes().map_err(|e| e.to_string())?;
            shared.conns.lock().await.remove(&index);
            let t = Instant::now();
            query_one(&shared, index, &r, &bytes).await?;
            Ok(t.elapsed().as_millis() as u32)
        })
    }
}

/// Where the engine should resolve for these settings (`engine` is the running stub).
pub fn route(dns: &Dns, engine: Option<&Engine>) -> Route {
    if !dns.active() {
        return Route::System;
    }
    let list = usable(dns);
    let all_doh = list.iter().all(|r| {
        r.kind == Kind::Https && r.server_name().is_some() && !r.ip.is_empty()
            || r.kind == Kind::Https && r.server_name().is_none()
    });
    if all_doh && engine.is_none() {
        return Route::Direct(list.iter().map(|r| r.address()).collect());
    }
    match engine {
        Some(e) => Route::Stub(e.template()),
        None => {
            warning!("dns: the stub is not running; the engine keeps the system resolver");
            Route::System
        }
    }
}
