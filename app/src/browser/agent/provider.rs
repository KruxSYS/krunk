//! Provider calls from the browser process (0.12.0, WP-A): builds the request with
//! `lb_core::agent::provider`, adds the key (unwrapped from DPAPI only for this call), sends it
//! with `fetch::request` and decodes the streamed answer.
//!
//! Nothing is sent while AI is off (every entry point checks it). The log gets the HTTP status,
//! the duration and the error kind only: never the address, the body or the key.

use super::super::Browser;
use crate::ffi::cef::fetch::{self, FetchError, Fetched, JobId, Req};
use crate::log::{info, warning};
use lb_core::agent::provider::{self as prov, Decoder, HttpRequest};
use lb_core::agent::{ChatRequest, ModelInfo, ProviderKind, StreamEvent};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// A chat stream is cancelled after this long without data.
const STREAM_IDLE_MS: i64 = 60_000;
/// A model list request is cancelled after this long without data.
const MODELS_IDLE_MS: i64 = 30_000;
/// The largest response read.
const MAX_BYTES: usize = 16 << 20;
/// The part of an error response kept for its message.
const MAX_ERROR_BODY: usize = 64 << 10;
/// How long a model list is reused.
const MODELS_TTL: Duration = Duration::from_secs(600);

type OnEvent = Box<dyn FnMut(StreamEvent)>;
type StreamDone = Box<dyn FnOnce(Result<(), String>)>;
type ModelsDone = Box<dyn FnOnce(Result<Vec<ModelInfo>, String>)>;

/// A cached model list: when it came, for which kind and address, and the models.
struct CachedModels {
    at: Instant,
    target: String,
    models: Vec<ModelInfo>,
}

thread_local! {
    /// Model lists by provider id (UI thread only).
    static MODELS: RefCell<HashMap<String, CachedModels>> = RefCell::new(HashMap::new());
}

/// A running chat stream's state, shared by the chunk and completion callbacks.
struct Stream {
    decoder: Decoder,
    /// The HTTP status seen with the chunks (0 = not known yet).
    status: i32,
    /// The start of an error response (status 400 and up), for its message.
    error_body: Vec<u8>,
    /// Taken out while it runs.
    on_event: Option<OnEvent>,
    done: Option<StreamDone>,
    started: Instant,
}

/// Hands `events` to the stream's listener (no borrow is held while it runs: it may cancel).
fn deliver(st: &Rc<RefCell<Stream>>, events: Vec<StreamEvent>) {
    if events.is_empty() {
        return;
    }
    let Some(mut f) = st.borrow_mut().on_event.take() else { return };
    for e in events {
        f(e);
    }
    let mut s = st.borrow_mut();
    if s.on_event.is_none() {
        s.on_event = Some(f);
    }
}

/// A message for the panel or the settings page for a request that did not complete.
fn fetch_error(e: &FetchError, idle_ms: i64) -> String {
    match e {
        FetchError::Cancelled => "Stopped".into(),
        FetchError::Timeout => format!("The provider did not answer for {} seconds", idle_ms / 1000),
        FetchError::Failed => "Could not reach the provider".into(),
        FetchError::TooBig => "The provider's answer was too big".into(),
    }
}

impl Browser {
    /// Where requests for provider `provider_id` go: its kind and base URL, or why nothing can
    /// be sent (AI off, no such provider, a bad address).
    fn provider_target(&self, provider_id: &str) -> Result<(ProviderKind, String), String> {
        let s = self.settings.borrow();
        if !s.ai.enabled {
            return Err("Turn on AI features to contact the provider".into());
        }
        let p = s.ai.provider(provider_id).ok_or("This provider is not set up. Choose one in Settings.")?;
        prov::check_base_url(p.kind, &p.base_url)?;
        Ok((p.kind, prov::base_url(p.kind, &p.base_url)))
    }

    /// Sends `http` with provider `provider_id`'s key in its headers. The key and the headers
    /// holding it live in zeroizing memory for this call only.
    fn provider_send(
        &self,
        provider_id: &str,
        kind: ProviderKind,
        http: &HttpRequest,
        idle_ms: i64,
        on_chunk: impl FnMut(i32, &[u8]) + 'static,
        done: impl FnOnce(Result<Fetched, FetchError>) + 'static,
    ) -> Result<JobId, String> {
        self.with_ai_key(provider_id, move |key| {
            if key.is_empty() && kind.needs_key() {
                return Err("Add an API key for this provider in Settings".to_string());
            }
            let mut headers = Zeroizing::new(http.headers.clone());
            headers.extend(prov::auth_headers(kind, key));
            let req = Req {
                method: http.method,
                url: &http.url,
                headers: &headers,
                body: http.body.as_deref().map(str::as_bytes),
                idle_timeout_ms: idle_ms,
                max_bytes: MAX_BYTES,
            };
            Ok(fetch::request(req, on_chunk, done))
        })?
    }

    /// Streams a chat request to provider `provider_id` (from `settings.ai.providers`).
    /// `on_event` gets the decoded events; `done` runs once: `Ok` at the normal end, `Err` with a
    /// message for the panel (HTTP status and the provider's message, network error, timeout,
    /// cancelled). Nothing is sent while AI is off. Returns the job for `provider_cancel`.
    pub(in crate::browser) fn provider_stream(
        &self,
        provider_id: &str,
        request: &ChatRequest,
        on_event: impl FnMut(StreamEvent) + 'static,
        done: impl FnOnce(Result<(), String>) + 'static,
    ) -> Option<JobId> {
        let (kind, base) = match self.provider_target(provider_id) {
            Ok(t) => t,
            Err(e) => {
                done(Err(e));
                return None;
            }
        };
        let http = prov::chat_request(kind, &base, request);
        let st = Rc::new(RefCell::new(Stream {
            decoder: Decoder::new(kind),
            status: 0,
            error_body: Vec::new(),
            on_event: Some(Box::new(on_event)),
            done: Some(Box::new(done)),
            started: Instant::now(),
        }));
        let chunks = st.clone();
        let on_chunk = move |status: i32, bytes: &[u8]| {
            let events = {
                let mut s = chunks.borrow_mut();
                s.status = status;
                if status >= 400 {
                    let room = MAX_ERROR_BODY.saturating_sub(s.error_body.len());
                    s.error_body.extend_from_slice(&bytes[..bytes.len().min(room)]);
                    return;
                }
                s.decoder.feed(bytes)
            };
            deliver(&chunks, events);
        };
        let ended = st.clone();
        let on_done = move |r: Result<Fetched, FetchError>| {
            let (events, result) = {
                let mut s = ended.borrow_mut();
                let ms = s.started.elapsed().as_millis();
                match r {
                    Ok(f) => {
                        let status = if f.status > 0 { f.status } else { s.status };
                        info!("ai provider: chat HTTP {status}, {ms} ms");
                        if status >= 400 {
                            let body = String::from_utf8_lossy(&s.error_body).into_owned();
                            (Vec::new(), Err(prov::error_message(status as u16, &body)))
                        } else {
                            (s.decoder.finish(), Ok(()))
                        }
                    }
                    Err(e) => {
                        info!("ai provider: chat ended: {e:?}, {ms} ms");
                        (Vec::new(), Err(fetch_error(&e, STREAM_IDLE_MS)))
                    }
                }
            };
            deliver(&ended, events);
            let done = ended.borrow_mut().done.take();
            ended.borrow_mut().on_event = None;
            if let Some(d) = done {
                d(result);
            }
        };
        match self.provider_send(provider_id, kind, &http, STREAM_IDLE_MS, on_chunk, on_done) {
            Ok(job) => Some(job),
            Err(e) => {
                let done = st.borrow_mut().done.take();
                if let Some(d) = done {
                    d(Err(e));
                }
                None
            }
        }
    }

    pub(in crate::browser) fn provider_cancel(&self, job: JobId) {
        fetch::cancel(job);
    }

    /// The provider's model list (cached per provider for 10 minutes unless `refresh`).
    pub(in crate::browser) fn provider_models(
        &self,
        provider_id: &str,
        refresh: bool,
        done: impl FnOnce(Result<Vec<ModelInfo>, String>) + 'static,
    ) {
        let (kind, base) = match self.provider_target(provider_id) {
            Ok(t) => t,
            Err(e) => return done(Err(e)),
        };
        let target = format!("{kind:?} {base}");
        if !refresh {
            let hit = MODELS.with(|m| {
                m.borrow()
                    .get(provider_id)
                    .filter(|c| c.target == target && c.at.elapsed() < MODELS_TTL)
                    .map(|c| c.models.clone())
            });
            if let Some(models) = hit {
                return done(Ok(models));
            }
        }
        let http = prov::models_request(kind, &base);
        let body = Rc::new(RefCell::new(Vec::new()));
        let done: Rc<RefCell<Option<ModelsDone>>> = Rc::new(RefCell::new(Some(Box::new(done))));
        let sink = body.clone();
        let on_chunk = move |_status: i32, bytes: &[u8]| sink.borrow_mut().extend_from_slice(bytes);
        let finish = done.clone();
        let id = provider_id.to_string();
        let started = Instant::now();
        let on_done = move |r: Result<Fetched, FetchError>| {
            let ms = started.elapsed().as_millis();
            let result = match r {
                Ok(f) => {
                    let text = String::from_utf8_lossy(&body.borrow()).into_owned();
                    if f.status >= 400 {
                        info!("ai provider: models HTTP {}, {ms} ms", f.status);
                        Err(prov::error_message(f.status as u16, &text))
                    } else {
                        let parsed = prov::parse_models(kind, &text);
                        match &parsed {
                            Ok(models) => {
                                info!("ai provider: models HTTP {}, {} models, {ms} ms", f.status, models.len());
                                MODELS.with(|m| {
                                    m.borrow_mut()
                                        .insert(id, CachedModels { at: Instant::now(), target, models: models.clone() })
                                });
                            }
                            Err(_) => warning!("ai provider: models HTTP {}, unreadable list", f.status),
                        }
                        parsed
                    }
                }
                Err(e) => {
                    info!("ai provider: models ended: {e:?}, {ms} ms");
                    Err(fetch_error(&e, MODELS_IDLE_MS))
                }
            };
            let d = finish.borrow_mut().take();
            if let Some(d) = d {
                d(result);
            }
        };
        if let Err(e) = self.provider_send(provider_id, kind, &http, MODELS_IDLE_MS, on_chunk, on_done) {
            let d = done.borrow_mut().take();
            if let Some(d) = d {
                d(Err(e));
            }
        }
    }

    /// Drops provider `id`'s cached model list (its key, address or the provider changed).
    pub(in crate::browser) fn provider_models_forget(&self, id: &str) {
        MODELS.with(|m| m.borrow_mut().remove(id));
    }
}
