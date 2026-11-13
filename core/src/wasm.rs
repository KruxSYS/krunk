//! The WASM memory inspector (0.19.0): the requests krunk://wasm, user scripts (`GM.wasm`) and
//! the AI agent's `wasm_*` tools send to the probe (`ui/inject/wasm-probe.js`), checked here
//! before the host passes them on. The probe checks addresses against the memory itself; this
//! module keeps out what is malformed or larger than the limits, so nothing else reaches a page.

use serde::{Deserialize, Serialize};

/// The probe's limits (the same numbers as `LIMITS` in wasm-probe.js).
pub const MAX_SCAN_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_RESULTS: u32 = 4 * 1024 * 1024;
pub const MAX_READ_BYTES: u32 = 64 * 1024;
pub const MAX_PAGE: u32 = 1000;
pub const MAX_FREEZES: u32 = 256;
pub const MAX_WATCH: usize = 256;
pub const MAX_POINTER_DEPTH: usize = 8;
/// Text of a value (numbers, 64-bit integers as decimal, strings for utf8/utf16).
pub const MAX_VALUE_CHARS: usize = 64 * 1024;

/// A value's type in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Bytes,
    Utf8,
    Utf16,
}

impl ValueType {
    /// A number type (scans work on these only).
    pub fn is_number(self) -> bool {
        !matches!(self, ValueType::Bytes | ValueType::Utf8 | ValueType::Utf16)
    }

    pub fn size(self) -> u32 {
        match self {
            ValueType::I8 | ValueType::U8 => 1,
            ValueType::I16 | ValueType::U16 => 2,
            ValueType::I32 | ValueType::U32 | ValueType::F32 => 4,
            ValueType::I64 | ValueType::U64 | ValueType::F64 => 8,
            ValueType::Bytes | ValueType::Utf8 | ValueType::Utf16 => 1,
        }
    }
}

/// First scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirstScan {
    Exact,
    Range,
    Bigger,
    Smaller,
    /// Unknown initial value: a snapshot, compared by the next scan.
    Unknown,
}

/// Next scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NextScan {
    Exact,
    Range,
    Changed,
    Unchanged,
    Increased,
    Decreased,
    IncreasedBy,
    DecreasedBy,
}

impl NextScan {
    fn needs_value(self) -> bool {
        matches!(self, NextScan::Exact | NextScan::Range | NextScan::IncreasedBy | NextScan::DecreasedBy)
    }
}

/// An address, or a pointer path: `base`, then for each offset a 32-bit pointer read at the
/// address plus the offset (`[[base] + o1] + o2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Where {
    Addr(u32),
    Path { base: u32, offsets: Vec<i32> },
}

/// A watch-list entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchEntry {
    pub mem: u32,
    #[serde(rename = "where")]
    pub at: Where,
    #[serde(rename = "type")]
    pub ty: ValueType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u32>,
}

/// A request to the probe, as the probe reads it (`{"op": …}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum WasmReq {
    /// The memories found (number, size, shared or not).
    List {},
    Read {
        mem: u32,
        #[serde(rename = "where")]
        at: Where,
        #[serde(rename = "type")]
        ty: ValueType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<u32>,
    },
    Write {
        mem: u32,
        #[serde(rename = "where")]
        at: Where,
        #[serde(rename = "type")]
        ty: ValueType,
        value: serde_json::Value,
    },
    /// The hex view: `length` bytes from `addr`.
    Bytes { mem: u32, addr: u32, length: u32 },
    WriteBytes { mem: u32, addr: u32, bytes: Vec<u8> },
    Scan {
        mem: u32,
        #[serde(rename = "type")]
        ty: ValueType,
        kind: FirstScan,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        epsilon: Option<f64>,
    },
    Next {
        scan: u32,
        kind: NextScan,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<serde_json::Value>,
    },
    Results {
        scan: u32,
        #[serde(default)]
        offset: u32,
        #[serde(default = "default_page")]
        limit: u32,
    },
    Progress { scan: u32 },
    Discard { scan: u32 },
    Freeze {
        mem: u32,
        #[serde(rename = "where")]
        at: Where,
        #[serde(rename = "type")]
        ty: ValueType,
        /// None: the value it has now.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
    },
    Unfreeze { freeze: u32 },
    Frozen {},
    Watch { entries: Vec<WatchEntry> },
    /// Ends every scan and freeze of the tab (the inspector page's Reset).
    Reset {},
}

fn default_page() -> u32 {
    100
}

/// A value as the probe takes it: a number, or (64-bit integers, strings, bytes) the
/// right JSON shape, within the size limit.
fn check_value(ty: ValueType, v: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value as V;
    let too_long = |s: &str| s.chars().count() > MAX_VALUE_CHARS;
    match (ty, v) {
        (ValueType::Bytes, V::Array(a)) if a.len() as u32 <= MAX_READ_BYTES => {
            if a.iter().all(|b| b.as_u64().is_some_and(|b| b <= 255)) {
                Ok(())
            } else {
                Err("bytes must be numbers 0..255".into())
            }
        }
        (ValueType::Bytes, _) => Err(format!("bytes must be an array of at most {MAX_READ_BYTES} numbers 0..255")),
        (ValueType::Utf8 | ValueType::Utf16, V::String(s)) if !too_long(s) => Ok(()),
        (ValueType::Utf8 | ValueType::Utf16, _) => Err("a text value must be a string".into()),
        (ValueType::I64 | ValueType::U64, V::String(s)) => {
            let s = s.trim();
            let ok = if ty == ValueType::U64 { s.parse::<u64>().is_ok() } else { s.parse::<i64>().is_ok() };
            if ok {
                Ok(())
            } else {
                Err(format!("not a {} value: {s}", if ty == ValueType::U64 { "u64" } else { "i64" }))
            }
        }
        (_, V::Number(n)) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            let (lo, hi, int) = match ty {
                ValueType::I8 => (i8::MIN as f64, i8::MAX as f64, true),
                ValueType::U8 => (0.0, u8::MAX as f64, true),
                ValueType::I16 => (i16::MIN as f64, i16::MAX as f64, true),
                ValueType::U16 => (0.0, u16::MAX as f64, true),
                ValueType::I32 => (i32::MIN as f64, i32::MAX as f64, true),
                ValueType::U32 => (0.0, u32::MAX as f64, true),
                ValueType::I64 => (-9.007_199_254_740_991e15, 9.007_199_254_740_991e15, true),
                ValueType::U64 => (0.0, 9.007_199_254_740_991e15, true),
                ValueType::F32 => (-f32::MAX as f64, f32::MAX as f64, false),
                _ => (f64::MIN, f64::MAX, false),
            };
            if !f.is_finite() {
                return Err("not a number".into());
            }
            if int && f.fract() != 0.0 {
                return Err(format!("{f} is not a whole number"));
            }
            if f < lo || f > hi {
                return Err(format!("{f} is out of range for {ty:?}").to_lowercase());
            }
            Ok(())
        }
        (_, V::String(s)) if s.trim().parse::<f64>().is_ok() && !too_long(s) => {
            check_value(ty, &serde_json::json!(s.trim().parse::<f64>().unwrap_or(0.0)))
        }
        _ => Err("the value must be a number".into()),
    }
}

fn check_where(w: &Where) -> Result<(), String> {
    match w {
        Where::Addr(_) => Ok(()),
        Where::Path { offsets, .. } if offsets.len() <= MAX_POINTER_DEPTH => Ok(()),
        Where::Path { .. } => Err(format!("a pointer path has at most {MAX_POINTER_DEPTH} offsets")),
    }
}

fn check_length(ty: ValueType, length: Option<u32>) -> Result<(), String> {
    match (ty.is_number(), length) {
        (true, Some(_)) => Err("a length is for bytes, utf8 and utf16 only".into()),
        (false, Some(n)) if n > MAX_READ_BYTES => Err(format!("at most {MAX_READ_BYTES} bytes at once")),
        _ => Ok(()),
    }
}

impl WasmReq {
    /// Parses and checks a request (JSON from a page, a script or the agent).
    pub fn parse(json: &serde_json::Value) -> Result<WasmReq, String> {
        let req: WasmReq = serde_json::from_value(json.clone()).map_err(|e| format!("not a valid request: {e}"))?;
        req.check()?;
        Ok(req)
    }

    pub fn check(&self) -> Result<(), String> {
        match self {
            WasmReq::Read { at, ty, length, .. } => {
                check_where(at)?;
                check_length(*ty, *length)
            }
            WasmReq::Write { at, ty, value, .. } => {
                check_where(at)?;
                check_value(*ty, value)
            }
            WasmReq::Bytes { length, .. } if *length > MAX_READ_BYTES => {
                Err(format!("at most {MAX_READ_BYTES} bytes at once"))
            }
            WasmReq::WriteBytes { bytes, .. } if bytes.len() as u32 > MAX_READ_BYTES => {
                Err(format!("at most {MAX_READ_BYTES} bytes at once"))
            }
            WasmReq::Scan { ty, kind, value, to, align, epsilon, .. } => {
                if !ty.is_number() {
                    return Err("scans are for number types (i8 … f64)".into());
                }
                if let Some(a) = align {
                    if ![1, 2, 4, 8].contains(a) || *a > ty.size() {
                        return Err("alignment must be 1, 2, 4 or 8 and at most the type's size".into());
                    }
                }
                if epsilon.is_some_and(|e| !e.is_finite() || e < 0.0) {
                    return Err("epsilon must be a number ≥ 0".into());
                }
                match kind {
                    FirstScan::Unknown => Ok(()),
                    FirstScan::Range => {
                        check_value(*ty, value.as_ref().ok_or("a range needs value and to")?)?;
                        check_value(*ty, to.as_ref().ok_or("a range needs value and to")?)
                    }
                    _ => check_value(*ty, value.as_ref().ok_or("this scan needs a value")?),
                }
            }
            WasmReq::Next { kind, value, to, .. } => {
                if kind.needs_value() && value.is_none() {
                    return Err("this scan needs a value".into());
                }
                if *kind == NextScan::Range && to.is_none() {
                    return Err("a range needs value and to".into());
                }
                Ok(())
            }
            WasmReq::Results { limit, .. } if *limit > MAX_PAGE => Err(format!("at most {MAX_PAGE} results at once")),
            WasmReq::Freeze { at, ty, value, .. } => {
                check_where(at)?;
                match value {
                    Some(v) => check_value(*ty, v),
                    None => Ok(()),
                }
            }
            WasmReq::Watch { entries } => {
                if entries.len() > MAX_WATCH {
                    return Err(format!("at most {MAX_WATCH} watch entries"));
                }
                for e in entries {
                    check_where(&e.at)?;
                    check_length(e.ty, e.length)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The request as the probe reads it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// The request changes memory (the agent's step log says so).
    pub fn writes(&self) -> bool {
        matches!(self, WasmReq::Write { .. } | WasmReq::WriteBytes { .. } | WasmReq::Freeze { .. })
    }
}

/// The DevTools function the host calls on its probe instance (`this`) with the memories
/// found by `Runtime.queryObjects` and a request.
pub const CALL_FUNCTION: &str = "function (found, req) { return this.call(found, req); }";

/// Context numbers: 0 is the page itself, a same-process frame has its DevTools execution
/// context's number, a dedicated worker `WORKER_CTX_BASE` plus the order it was attached in.
pub const WORKER_CTX_BASE: u32 = 1_000_000;

/// A DevTools call for a scan may take this long (ms); the others the usual ten seconds.
pub const SCAN_TIMEOUT_MS: i64 = 120_000;

/// What the inspector can't reach, as the page, the API and the tools say it.
pub const UNREACHABLE: &str = "Not reachable: frames of other sites (they run in another process), service workers \
     and shared workers, and a page that is paused in the DevTools debugger.";

impl WasmReq {
    /// The request scans a memory (a long call).
    pub fn is_scan(&self) -> bool {
        matches!(self, WasmReq::Scan { .. } | WasmReq::Next { .. })
    }
}

/// May the page at `url` be inspected: a web page or a local file, never one of Krunk's own
/// pages (or an extension's).
pub fn inspectable(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url.trim()) else { return false };
    match u.scheme() {
        "file" => true,
        "http" | "https" => crate::agent::pages::may_act(url).is_ok(),
        _ => false,
    }
}

/// The expression that makes a context's probe instance (the host keeps the object's handle;
/// the page gets no reference). `hook_key`: the property the document-start hook left its
/// instance under, taken (and removed) instead when it is there.
pub fn make_expression(hook_key: Option<&str>) -> String {
    let probe = crate::assets::inject_script("wasm-probe.js").trim().trim_end_matches(';');
    let key = serde_json::to_string(&hook_key).unwrap_or_else(|_| "null".into());
    format!(
        "(function(){{var k={key};try{{var p=k&&globalThis[k];if(p){{delete globalThis[k];return p;}}}}catch(e){{}}\
         return ({probe}\n)(globalThis);}})()"
    )
}

/// The document-start script of the fallback: a probe instance that hooks `WebAssembly`
/// before any page script runs, left under `hook_key` (not enumerable) until the host takes it.
pub fn hook_script(hook_key: &str) -> String {
    let probe = crate::assets::inject_script("wasm-probe.js").trim().trim_end_matches(';');
    let key = serde_json::to_string(hook_key).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(){{try{{var p=({probe}\n)(globalThis);p.hook();\
         Object.defineProperty(globalThis,{key},{{value:p,configurable:true,enumerable:false,writable:false}});\
         }}catch(e){{}}}})();"
    )
}

/// The engine's answer when a probe's handle (or its context) is gone: the page navigated.
pub fn handle_lost(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("could not find object")
        || e.contains("cannot find context")
        || e.contains("execution context was destroyed")
        || e.contains("invalid remote object id")
        || e.contains("object reference chain")
        || e.contains("session with given id not found")
}

/// A thrown error's text for the caller (`exceptionDetails` of a DevTools answer).
pub fn exception_message(details: &serde_json::Value) -> String {
    let text = details
        .pointer("/exception/description")
        .and_then(|d| d.as_str())
        .map(|d| d.lines().next().unwrap_or("").to_string())
        .or_else(|| details.get("text").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_else(|| "the probe failed".into());
    let text = text.strip_prefix("Error: ").unwrap_or(&text);
    text.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(v: serde_json::Value) -> Result<WasmReq, String> {
        WasmReq::parse(&v)
    }

    #[test]
    fn requests_round_trip_in_the_probes_shape() {
        let r = p(json!({"op": "read", "mem": 0, "where": 1024, "type": "i32"})).unwrap();
        assert_eq!(r.to_json(), json!({"op": "read", "mem": 0, "where": 1024, "type": "i32"}));
        let r = p(json!({"op": "read", "mem": 1, "where": {"base": 300, "offsets": [8, -4]}, "type": "f32"})).unwrap();
        assert_eq!(r.to_json()["where"], json!({"base": 300, "offsets": [8, -4]}));
        let r = p(json!({"op": "scan", "mem": 0, "type": "i32", "kind": "exact", "value": 100})).unwrap();
        assert_eq!(r.to_json()["kind"], "exact");
        let r = p(json!({"op": "next", "scan": 3, "kind": "decreasedBy", "value": 10})).unwrap();
        assert_eq!(r.to_json()["kind"], "decreasedBy");
        assert_eq!(p(json!({"op": "results", "scan": 1})).unwrap().to_json()["limit"], 100);
        assert_eq!(p(json!({"op": "list"})).unwrap(), WasmReq::List {});
        assert!(p(json!({"op": "write", "mem": 0, "where": 8, "type": "bytes", "value": [1, 2, 255]})).is_ok());
        assert!(p(json!({"op": "write", "mem": 0, "where": 8, "type": "utf8", "value": "héllo"})).is_ok());
        assert!(p(json!({"op": "freeze", "mem": 0, "where": 8, "type": "i32"})).is_ok(), "freeze at the value it has");
    }

    #[test]
    fn malformed_and_oversized_requests_are_refused() {
        for bad in [
            json!({"op": "eval", "code": "x"}),
            json!({"op": "read", "mem": 0, "where": 1, "type": "i33"}),
            json!({"op": "read", "mem": -1, "where": 1, "type": "i32"}),
            json!({"op": "read", "mem": 0, "where": 1, "type": "i32", "extra": 1}),
            json!({"op": "read", "mem": 0, "where": 1, "type": "i32", "length": 4}),
            json!({"op": "read", "mem": 0, "where": 1, "type": "bytes", "length": 70000}),
            json!({"op": "read", "mem": 0, "where": {"base": 1, "offsets": [0, 0, 0, 0, 0, 0, 0, 0, 0]}, "type": "i32"}),
            json!({"op": "write", "mem": 0, "where": 1, "type": "u8", "value": 256}),
            json!({"op": "write", "mem": 0, "where": 1, "type": "i32", "value": 1.5}),
            json!({"op": "write", "mem": 0, "where": 1, "type": "i32", "value": "abc"}),
            json!({"op": "write", "mem": 0, "where": 1, "type": "u64", "value": "-1"}),
            json!({"op": "write", "mem": 0, "where": 1, "type": "bytes", "value": [256]}),
            json!({"op": "bytes", "mem": 0, "addr": 0, "length": 1_000_000}),
            json!({"op": "scan", "mem": 0, "type": "utf8", "kind": "exact", "value": "x"}),
            json!({"op": "scan", "mem": 0, "type": "i32", "kind": "exact"}),
            json!({"op": "scan", "mem": 0, "type": "i32", "kind": "range", "value": 1}),
            json!({"op": "scan", "mem": 0, "type": "i16", "kind": "unknown", "align": 4}),
            json!({"op": "next", "scan": 1, "kind": "increasedBy"}),
            json!({"op": "results", "scan": 1, "limit": 5000}),
        ] {
            assert!(p(bad.clone()).is_err(), "{bad}");
        }
        let many: Vec<_> = (0..=MAX_WATCH).map(|_| json!({"mem": 0, "where": 0, "type": "i32"})).collect();
        assert!(p(json!({"op": "watch", "entries": many})).is_err());
    }

    #[test]
    fn pages_expressions_and_errors() {
        assert!(inspectable("https://game.example/play") && inspectable("file:///C:/x/index.html"));
        for no in ["https://krunk.invalid/settings.html", "about:blank", "chrome://gpu", "devtools://devtools/x", ""] {
            assert!(!inspectable(no), "{no}");
        }
        let make = make_expression(Some("k1"));
        assert!(make.contains("var k=\"k1\"") && make.contains("maxScanBytes") && make.ends_with("(globalThis);})()"));
        assert!(make_expression(None).contains("var k=null"));
        let hook = hook_script("k1");
        assert!(hook.contains("p.hook()") && hook.contains("enumerable:false"));
        assert!(handle_lost("Could not find object with given id") && handle_lost("Cannot find context with specified id"));
        assert!(!handle_lost("address 5 + 4 is past the end of the memory"));
        let d = json!({"text": "Uncaught (in promise)", "exception": {"description": "Error: no memory 3\n    at mem"}});
        assert_eq!(exception_message(&d), "no memory 3");
        let scan = json!({"op": "scan", "mem": 0, "type": "i32", "kind": "unknown"});
        assert!(p(scan).unwrap().is_scan() && !WasmReq::List {}.is_scan());
    }

    #[test]
    fn numbers_may_come_as_text() {
        // The agent's models send "100" as often as 100.
        assert!(p(json!({"op": "write", "mem": 0, "where": 1, "type": "i32", "value": "100"})).is_ok());
        assert!(p(json!({"op": "write", "mem": 0, "where": 1, "type": "f64", "value": " 2.5 "})).is_ok());
        assert!(p(json!({"op": "write", "mem": 0, "where": 1, "type": "i64", "value": "-9223372036854775808"})).is_ok());
        assert!(WasmReq::Write { mem: 0, at: Where::Addr(0), ty: ValueType::I8, value: json!(0) }.writes());
    }
}
