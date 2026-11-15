//! An in-memory sync server for the engine's tests: the records semantics of the real one
//! (PROTOCOL §11.7) without a network — per-account seq, conditional writes, purges and the 410
//! horizon, the 423 freeze — and, for the rotation tests (S9), the keys side: directories by
//! CAS with the epoch rules, the escrow, the claim lease, devices and their removal, `rekey/batch`
//! and `epoch_lt` pulls. Single-threaded; devices share it through `Rc`. The bearer token is the
//! calling device's id.

use super::api::{HttpClient, HttpResponse};
use super::dir;
use super::envelope::{b64, unb64};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Debug, Clone, Default)]
pub struct MemDevice {
    pub id: String,
    pub short: u16,
    pub revoked: bool,
    pub had_keys: bool,
}

#[derive(Default)]
pub struct Server {
    pub head: i64,
    pub horizon: i64,
    /// The AK epoch (`current_epoch`).
    pub epoch: u32,
    pub pak_epoch: u32,
    /// Engine tests without keys: every write is refused with 423.
    pub frozen: bool,
    /// (collection, id) → (seq, envelope)
    pub records: BTreeMap<(String, String), (i64, Value)>,
    pub offline: bool,
    pub calls: usize,
    // ---- keys (S6–S9) ----
    pub keyset: Option<String>,
    /// Directory versions as wire blobs (`{seq, bytes, mac, chain?, escrow_mac?}`), oldest first.
    pub ak_dirs: Vec<Value>,
    pub pak_dirs: Vec<Value>,
    pub escrow: Option<Value>,
    pub devices: Vec<MemDevice>,
    pub rotation_ak: bool,
    pub rotation_pak: bool,
    pub lease_ak: Option<String>,
    pub lease_pak: Option<String>,
    /// Commits refused for a lost CAS (the race tests look at it).
    pub cas_lost: usize,
    /// The lease is advisory: with this, every claim succeeds (to force a CAS race).
    pub ignore_leases: bool,
}

#[derive(Clone)]
pub struct Mem(pub Rc<RefCell<Server>>);

impl Default for Mem {
    fn default() -> Self {
        Mem::new()
    }
}

impl Mem {
    pub fn new() -> Mem {
        Mem(Rc::new(RefCell::new(Server { epoch: 1, pak_epoch: 1, ..Default::default() })))
    }

    /// Links a device (as the website would).
    pub fn link(&self, id: &str) -> u16 {
        let mut s = self.0.borrow_mut();
        let short = s.devices.len() as u16 + 1;
        s.devices.push(MemDevice { id: id.into(), short, ..Default::default() });
        short
    }

    /// Removes a device (the website's Remove): its tokens stop working; a device that held keys
    /// freezes the writes until both keys are rotated.
    pub fn revoke(&self, id: &str) {
        let mut s = self.0.borrow_mut();
        let mut had = false;
        for d in s.devices.iter_mut().filter(|d| d.id == id) {
            d.revoked = true;
            had = d.had_keys;
        }
        if had && s.keyset.is_some() {
            s.rotation_ak = true;
            s.rotation_pak = true;
        }
    }
}

fn ok(v: Value) -> Result<HttpResponse, String> {
    Ok(HttpResponse { status: 200, body: v.to_string().into_bytes(), ..Default::default() })
}

fn err(status: u16, code: &str) -> Result<HttpResponse, String> {
    Ok(HttpResponse { status, body: json!({"error": code}).to_string().into_bytes(), ..Default::default() })
}

fn blob_hash(b: &Value) -> [u8; 32] {
    dir::hash(&unb64(b["bytes"].as_str().unwrap_or_default()).unwrap_or_default())
}

/// The directory fields the server checks (it can't verify tags).
fn fields(b: &Value) -> Value {
    let bytes = unb64(b["bytes"].as_str().unwrap_or_default()).unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn listed(dirs: &[Value]) -> Vec<String> {
    dirs.last()
        .map(fields)
        .and_then(|f| f["devices"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_string))
        .collect()
}

impl Server {
    fn active(&self, id: &str) -> bool {
        self.devices.iter().any(|d| d.id == id && !d.revoked)
    }

    fn keys_state(&self) -> Value {
        let remaining = self
            .records
            .values()
            .filter(|(_, e)| {
                let ep = e["e"].as_u64().unwrap_or(0) as u32;
                (e["k"] == "ak" && ep < self.epoch) || (e["k"] == "pak" && ep < self.pak_epoch)
            })
            .count();
        // Engine tests without keys see a keyset "g" and never a rotation.
        let keyset = self.keyset.clone().or_else(|| self.ak_dirs.is_empty().then(|| "g".to_string()));
        json!({
            "keyset_gen": keyset,
            "current_epoch": self.epoch, "pak_epoch": self.pak_epoch, "rr_gen": 1,
            "rotation": {"ak": self.rotation_ak, "pak": self.rotation_pak},
            "rekey": {"remaining_old": remaining}, "head_seq": self.head,
            "pending_approvals": 0,
            "quota": {"records": self.records.len(), "bytes": 0, "history": 0, "max_records": 100000,
                "max_bytes": 50000000, "max_history": 25000},
        })
    }

    fn commit(&mut self, b: &Value) -> Result<HttpResponse, String> {
        let first = b["if_absent"] == true;
        if first != self.ak_dirs.is_empty() {
            self.cas_lost += 1;
            return err(409, "conflict");
        }
        if !first {
            let cas = |want: &Value, dirs: &Vec<Value>| {
                dirs.last().is_some_and(|h| want.as_str() == Some(b64(&blob_hash(h)).as_str()))
            };
            if (b.get("ak_dir").is_some() && !cas(&b["if_ak_prev_hash"], &self.ak_dirs))
                || (b.get("pak_dir").is_some() && !cas(&b["if_pak_prev_hash"], &self.pak_dirs))
            {
                self.cas_lost += 1;
                return err(409, "conflict");
            }
        }
        let mut new_epoch = None;
        let mut new_pak_epoch = None;
        if let Some(a) = b.get("ak_dir") {
            let f = fields(a);
            let epoch = f["epoch"].as_u64().unwrap_or(0) as u32;
            let seq = f["seq"].as_u64().unwrap_or(0);
            let head = self.ak_dirs.last().map(fields);
            let (head_seq, head_epoch) = head
                .as_ref()
                .map_or((0, 0), |h| (h["seq"].as_u64().unwrap_or(0), h["epoch"].as_u64().unwrap_or(0) as u32));
            if seq != head_seq + 1 || (head.is_some() && epoch != head_epoch && epoch != head_epoch + 1) {
                return err(400, "bad_request");
            }
            if head.is_some() && self.rotation_ak && epoch == head_epoch {
                return err(423, "rotation_pending");
            }
            let members: Vec<String> = f["devices"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d["id"].as_str().map(str::to_string))
                .collect();
            if members.iter().any(|m| !self.active(m)) {
                return err(400, "bad_request");
            }
            if let Some(h) = &head {
                let revoked: Vec<&Value> = f["revoked"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
                if h["revoked"].as_array().into_iter().flatten().any(|r| !revoked.contains(&r)) {
                    return err(400, "bad_request");
                }
            }
            if head.is_some() && epoch == head_epoch + 1 {
                new_epoch = Some(epoch);
            }
        }
        if let Some(p) = b.get("pak_dir") {
            let f = fields(p);
            let epoch = f["pak_epoch"].as_u64().unwrap_or(0) as u32;
            let seq = f["seq"].as_u64().unwrap_or(0);
            let head = self.pak_dirs.last().map(fields);
            let (head_seq, head_epoch) = head
                .as_ref()
                .map_or((0, 0), |h| (h["seq"].as_u64().unwrap_or(0), h["pak_epoch"].as_u64().unwrap_or(0) as u32));
            if seq != head_seq + 1 || (head.is_some() && epoch != head_epoch && epoch != head_epoch + 1) {
                return err(400, "bad_request");
            }
            if head.is_some() && self.rotation_pak && epoch == head_epoch {
                return err(423, "rotation_pending");
            }
            if f["vault_keys"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| !self.active(v["device"].as_str().unwrap_or_default()))
            {
                return err(400, "bad_request");
            }
            if head.is_some() && epoch == head_epoch + 1 {
                new_pak_epoch = Some(epoch);
            }
        }
        let mut seqs = json!({});
        for (k, out) in [("ak_dir", "ak_seq"), ("pak_dir", "pak_seq")] {
            if let Some(blob) = b.get(k) {
                let mut w = blob.clone();
                let seq = fields(blob)["seq"].clone();
                w["seq"] = seq.clone();
                if k == "ak_dir" {
                    self.ak_dirs.push(w);
                } else {
                    self.pak_dirs.push(w);
                }
                seqs[out] = seq;
            }
        }
        if let Some(e) = b.get("escrow") {
            self.escrow = Some(e.clone());
        }
        if first {
            self.keyset = Some("gen-1".into());
            self.epoch = 1;
            self.pak_epoch = 1;
        }
        if let Some(e) = new_epoch {
            self.epoch = e;
            self.rotation_ak = false;
            self.lease_ak = None;
        }
        if let Some(e) = new_pak_epoch {
            self.pak_epoch = e;
            self.rotation_pak = false;
            self.lease_pak = None;
        }
        // Devices listed in ak_dir hold keys (their later removal rotates).
        let now_listed = listed(&self.ak_dirs);
        for d in self.devices.iter_mut().filter(|d| now_listed.contains(&d.id)) {
            d.had_keys = true;
        }
        ok(seqs)
    }
}

impl HttpClient for Mem {
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, String> {
        let mut s = self.0.borrow_mut();
        s.calls += 1;
        if s.offline {
            return Err("offline".into());
        }
        let caller = bearer.unwrap_or_default().to_string();
        if s.devices.iter().any(|d| d.id == caller && d.revoked) {
            return err(401, "unauthorized");
        }
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let q: BTreeMap<&str, &str> = query.split('&').filter_map(|kv| kv.split_once('=')).collect();
        let body: Value = body.and_then(|b| serde_json::from_slice(b).ok()).unwrap_or(Value::Null);
        match (method, route) {
            ("GET", "sync/state") => ok(s.keys_state()),
            ("GET", "keys/head") => match (s.ak_dirs.last(), s.pak_dirs.last()) {
                (Some(a), Some(p)) => ok(json!({"ak_dir": a, "pak_dir": p})),
                _ => err(404, "no_keyset"),
            },
            ("GET", "keys/chain") => {
                let from = q.get("from").copied().unwrap_or("0,0");
                let pick = |list: &Vec<Value>, after: Option<u64>| -> Vec<Value> {
                    match after {
                        Some(a) => list.iter().filter(|b| b["seq"].as_u64().unwrap_or(0) > a).cloned().collect(),
                        None => {
                            let anchor = list
                                .iter()
                                .rposition(|b| b.get("escrow_mac").is_some_and(|m| !m.is_null()))
                                .unwrap_or(0);
                            list[anchor..].to_vec()
                        }
                    }
                };
                if from == "anchor" {
                    ok(json!({"ak": pick(&s.ak_dirs, None), "pak": pick(&s.pak_dirs, None)}))
                } else {
                    let (a, p) = from.split_once(',').unwrap_or((from, "0"));
                    ok(json!({"ak": pick(&s.ak_dirs, a.parse().ok()), "pak": pick(&s.pak_dirs, p.parse().ok())}))
                }
            }
            ("GET", "keys/escrow") => match &s.escrow {
                Some(e) => ok(e.clone()),
                None => err(404, "no_keyset"),
            },
            ("POST", "keys/commit") => s.commit(&body),
            ("POST", "keys/rotation/claim") => {
                let kind = body["kind"].as_str().unwrap_or_default().to_string();
                let ignore = s.ignore_leases;
                let lease = if kind == "ak" { &mut s.lease_ak } else { &mut s.lease_pak };
                match lease {
                    Some(holder) if *holder != caller && !ignore => err(409, "conflict"),
                    _ => {
                        *lease = Some(caller);
                        ok(json!({"lease_until": "later"}))
                    }
                }
            }
            ("GET", "devices") => {
                let now_listed = listed(&s.ak_dirs);
                ok(Value::Array(
                    s.devices
                        .iter()
                        .map(|d| {
                            json!({"id": d.id, "short": d.short, "platform": "linux", "label": format!("Device {}", d.id),
                                "client_version": "test", "has_keys": now_listed.contains(&d.id),
                                "revoked_at": d.revoked.then_some("then"), "self": d.id == caller})
                        })
                        .collect(),
                ))
            }
            (m, r) if m == "POST" && r.starts_with("devices/") && r.ends_with("/revoke") => {
                let id = r["devices/".len()..r.len() - "/revoke".len()].to_string();
                drop(s);
                self.revoke(&id);
                ok(json!({}))
            }
            ("GET", "records") => {
                let since: i64 = q.get("since").and_then(|x| x.parse().ok()).unwrap_or(0);
                let limit: usize = q.get("limit").and_then(|x| x.parse().ok()).unwrap_or(500);
                if since > 0 && since < s.horizon {
                    return err(410, "gone");
                }
                let epoch_lt: Option<u64> = q.get("epoch_lt").and_then(|x| x.parse().ok());
                let k = match (q.get("k").copied(), epoch_lt) {
                    (None, Some(_)) => Some("ak"),
                    (k, _) => k,
                };
                let mut all: Vec<&(i64, Value)> = s
                    .records
                    .values()
                    .filter(|(seq, env)| {
                        *seq > since
                            && k.is_none_or(|k| env["k"] == k)
                            && epoch_lt.is_none_or(|e| env["e"].as_u64().unwrap_or(0) < e)
                    })
                    .collect();
                all.sort_by_key(|(seq, _)| *seq);
                let page: Vec<Value> =
                    all.iter().take(limit).map(|(seq, env)| json!({"seq": seq, "envelope": env})).collect();
                ok(json!({"records": page, "has_more": all.len() > limit, "head_seq": s.head}))
            }
            ("POST", "records/batch") => {
                if s.frozen {
                    return err(423, "rotation_pending");
                }
                let writes = body["writes"].as_array().cloned().unwrap_or_default();
                for w in &writes {
                    let k = &w["envelope"]["k"];
                    if (k == "ak" && s.rotation_ak) || (k == "pak" && s.rotation_pak) {
                        return err(423, "rotation_pending");
                    }
                }
                let mut results = Vec::new();
                for w in writes {
                    let key = (
                        w["c"].as_str().unwrap_or_default().to_string(),
                        w["id"].as_str().unwrap_or_default().to_string(),
                    );
                    let cur = s.records.get(&key).cloned();
                    let cur_seq = cur.as_ref().map_or(0, |c| c.0);
                    if w["if_seq"].as_i64().unwrap_or(0) != cur_seq {
                        results.push(json!({"id": key.1, "status": "conflict",
                            "current": cur.map(|(seq, env)| json!({"seq": seq, "envelope": env}))}));
                        continue;
                    }
                    if w["purge"] == true {
                        if let Some((seq, _)) = s.records.remove(&key) {
                            s.horizon = s.horizon.max(seq);
                        }
                        results.push(json!({"id": key.1, "status": "ok", "seq": null}));
                        continue;
                    }
                    let want = if w["envelope"]["k"] == "pak" { s.pak_epoch } else { s.epoch };
                    if w["envelope"]["e"].as_u64() != Some(u64::from(want)) {
                        return err(400, "bad_request");
                    }
                    s.head += 1;
                    let seq = s.head;
                    s.records.insert(key.clone(), (seq, w["envelope"].clone()));
                    results.push(json!({"id": key.1, "status": "ok", "seq": seq}));
                }
                ok(json!({"results": results, "head_seq": s.head}))
            }
            ("POST", "rekey/batch") => {
                let epoch = body["epoch"].as_u64().unwrap_or(0) as u32;
                let mut done = 0;
                for r in body["replaces"].as_array().cloned().unwrap_or_default() {
                    let env = r["envelope"].clone();
                    let pak = env["k"] == "pak";
                    let current = if pak { s.pak_epoch } else { s.epoch };
                    if epoch != current || env["e"].as_u64() != Some(u64::from(current)) {
                        return err(409, "conflict");
                    }
                    if (pak && s.rotation_pak) || (!pak && s.rotation_ak) {
                        return err(423, "rotation_pending");
                    }
                    let c = r["c"].as_str().unwrap_or_default().to_string();
                    let old = (c.clone(), r["old_id"].as_str().unwrap_or_default().to_string());
                    let gone =
                        s.records.get(&old).is_some_and(|(_, e)| (e["e"].as_u64().unwrap_or(0) as u32) < current);
                    if !gone {
                        continue;
                    }
                    s.records.remove(&old);
                    s.head += 1;
                    let seq = s.head;
                    s.records.insert((c, env["id"].as_str().unwrap_or_default().to_string()), (seq, env));
                    done += 1;
                }
                let remaining = s.keys_state()["rekey"]["remaining_old"].clone();
                ok(json!({"done": done, "remaining_old": remaining}))
            }
            _ => err(404, "not_found"),
        }
    }
}
