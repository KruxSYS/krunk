//! An in-memory sync server for the engine's tests: the records semantics of the real one
//! (PROTOCOL §11.7) without a network — per-account seq, conditional writes, purges and the 410
//! horizon, the 423 freeze. Single-threaded; devices share it through `Rc`.

use super::api::{HttpClient, HttpResponse};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Default)]
pub struct Server {
    pub head: i64,
    pub horizon: i64,
    pub epoch: u32,
    pub frozen: bool,
    /// (collection, id) → (seq, envelope)
    pub records: BTreeMap<(String, String), (i64, Value)>,
    pub offline: bool,
    pub calls: usize,
}

#[derive(Clone)]
pub struct Mem(pub Rc<RefCell<Server>>);

impl Mem {
    pub fn new() -> Mem {
        Mem(Rc::new(RefCell::new(Server { epoch: 1, ..Default::default() })))
    }
}

fn ok(v: Value) -> Result<HttpResponse, String> {
    Ok(HttpResponse { status: 200, body: v.to_string().into_bytes(), ..Default::default() })
}

fn err(status: u16, code: &str) -> Result<HttpResponse, String> {
    Ok(HttpResponse { status, body: json!({"error": code}).to_string().into_bytes(), ..Default::default() })
}

impl HttpClient for Mem {
    fn call(
        &self,
        method: &str,
        path: &str,
        _bearer: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, String> {
        let mut s = self.0.borrow_mut();
        s.calls += 1;
        if s.offline {
            return Err("offline".into());
        }
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let q: BTreeMap<&str, &str> = query.split('&').filter_map(|kv| kv.split_once('=')).collect();
        match (method, route) {
            ("GET", "sync/state") => ok(json!({
                "keyset_gen": "g", "current_epoch": s.epoch, "pak_epoch": 1, "rr_gen": 1,
                "rotation": {"ak": s.frozen, "pak": false}, "rekey": {"remaining_old": 0}, "head_seq": s.head,
                "pending_approvals": 0, "quota": {"records": s.records.len(), "bytes": 0, "history": 0, "max_records": 100000, "max_bytes": 50000000, "max_history": 25000},
            })),
            ("GET", "records") => {
                let since: i64 = q.get("since").and_then(|x| x.parse().ok()).unwrap_or(0);
                let limit: usize = q.get("limit").and_then(|x| x.parse().ok()).unwrap_or(500);
                if since > 0 && since < s.horizon {
                    return err(410, "gone");
                }
                let k = q.get("k").copied();
                let mut all: Vec<&(i64, Value)> =
                    s.records.values().filter(|(seq, env)| *seq > since && k.is_none_or(|k| env["k"] == k)).collect();
                all.sort_by_key(|(seq, _)| *seq);
                let page: Vec<Value> =
                    all.iter().take(limit).map(|(seq, env)| json!({"seq": seq, "envelope": env})).collect();
                ok(json!({"records": page, "has_more": all.len() > limit, "head_seq": s.head}))
            }
            ("POST", "records/batch") => {
                if s.frozen {
                    return err(423, "rotation_pending");
                }
                let b: Value = serde_json::from_slice(body.unwrap_or(b"{}")).map_err(|e| e.to_string())?;
                let mut results = Vec::new();
                for w in b["writes"].as_array().cloned().unwrap_or_default() {
                    let key = (
                        w["c"].as_str().unwrap_or_default().to_string(),
                        w["id"].as_str().unwrap_or_default().to_string(),
                    );
                    let cur = s.records.get(&key).cloned();
                    let cur_seq = cur.as_ref().map_or(0, |c| c.0);
                    if w["if_seq"].as_i64().unwrap_or(0) != cur_seq {
                        results.push(json!({"id": key.1, "status": "conflict", "current": cur.map(|(seq, env)| json!({"seq": seq, "envelope": env}))}));
                        continue;
                    }
                    if w["purge"] == true {
                        if let Some((seq, _)) = s.records.remove(&key) {
                            s.horizon = s.horizon.max(seq);
                        }
                        results.push(json!({"id": key.1, "status": "ok", "seq": null}));
                        continue;
                    }
                    if w["envelope"]["e"].as_u64() != Some(u64::from(s.epoch)) {
                        return err(400, "bad_request");
                    }
                    s.head += 1;
                    let seq = s.head;
                    s.records.insert(key.clone(), (seq, w["envelope"].clone()));
                    results.push(json!({"id": key.1, "status": "ok", "seq": seq}));
                }
                ok(json!({"results": results, "head_seq": s.head}))
            }
            _ => err(404, "not_found"),
        }
    }
}
