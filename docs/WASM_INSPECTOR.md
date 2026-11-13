# The WASM memory inspector

Games and apps compiled to WebAssembly keep their state (health, score, money, positions) in a
`WebAssembly.Memory` and in the globals of their instances, not in the page's DOM. The inspector
finds those memories and instances in a tab and reads, searches and changes them, Cheat-Engine
style. It has three surfaces over one implementation:

- the page **`krunk://wasm`** (⋮ → **WASM memory inspector**, for the tab you are on),
- **`GM.wasm`** for user scripts (`@grant GM.wasm`),
- the AI agent's **`wasm_*`** tools.

All three send the same typed requests (`core/src/wasm.rs: WasmReq`), which the host checks and
passes to one probe per page, frame or worker (`ui/inject/wasm-probe.js`) over the tab's DevTools
connection (`app/src/browser/wasm.rs: wasm_call`). There is no second implementation: the page
enters at `dispatch_wasm`, a user script at `userscripts.rs` (`GmCall::Wasm` → `wasm_body`), the
agent at `agent/page.rs: tool_wasm` (→ `core::wasm::agent_request`).

What was tested where, and what was shown on Windows: `docs/WASM_INSPECTOR_STATUS.md`.

## What it reaches

- **Reached:** the page (the main frame); frames of the same process (in practice: same-site
  frames); dedicated workers. Each is a *context* with a number (`ctx`): 0 is the page, a frame
  has its DevTools context number, workers count from 1,000,000. Memory numbers, instance
  numbers, scans and freezes belong to one context.
- **Not reached:** frames of other sites (they run in another process), service workers and
  shared workers, globals an instance does not export, a page that is paused in the DevTools
  debugger (requests time out until it runs again), Krunk's own pages.
- **How:** `Runtime.queryObjects` on `WebAssembly.Memory.prototype` and
  `WebAssembly.Instance.prototype` finds the objects without touching the page's globals. The
  query makes the engine collect garbage, so it runs when memories are listed and when a
  document is new, not on every read.
- **Fallback (the page only):** "Hook WebAssembly and reload the tab" wraps `WebAssembly.Memory`,
  `Instance`, `instantiate` and `instantiateStreaming` at document start, so memories and
  instances are caught as they are made. It is visible to the page.
- **`memory.grow`:** a memory keeps its number. Addresses found before stay valid, addresses
  past the old size become valid, result lists, watch entries and frozen values go on. A scan
  that the memory grows under covers the size it started with.
- **Nothing is hidden from the page.** It can notice the DevTools session, the hook, timing, or
  simply that its values changed. Online games check their state on the server.

## The page: krunk://wasm

- **Memories:** number, context, the instance that exports it, size (`1.1 MiB · 17 pages`).
  **Find memories** looks again.
- **Instances:** exports by kind; choosing an instance chooses its memory. Its exported globals
  are a table of name, type and live value; a mutable one takes a new value with Enter.
- **Scan:** type (`i8 u8 i16 u16 i32 u32 i64 u64 f32 f64`), first scan (exact value, value
  between, bigger than, smaller than, unknown initial value), then next scans (exact, between,
  changed, unchanged, increased, decreased, increased by, decreased by). Results list address,
  value now, value at the scan; **Watch** and **Hex** per row.
- **Watch list:** label, address or pointer path (`0x100040 > 8`: read the 32-bit pointer at
  0x100040, add 8), type (the number types and `utf8`), live value (Enter writes), **Freeze**,
  Remove; Export and Import as JSON.
- **Hex view:** a 256-byte window, go to address, type hex digits to change a byte.

## Values

Little-endian, as WebAssembly stores them. `i8 … u32`, `f32`, `f64` are JSON numbers; `i64` and
`u64` are decimal strings (JSON has no 64-bit integers; numbers are accepted when writing);
`bytes` is an array of 0..255; `utf8` and `utf16` are text. A read of `bytes`, `utf8` or `utf16`
takes a `length` in bytes (default 16; `utf8` without a length stops at the first NUL). An
address is a byte offset into the memory, or a pointer path `{base, offsets}`: for each offset
the 32-bit pointer at the address is read and the offset added (`memory64` is not followed).

## Limits

| Limit | Value |
|---|---|
| Bytes a scan covers | the first 512 MiB of a memory |
| Addresses a scan keeps | 4,194,304 (`truncated: true` when more matched) |
| Scans kept per context | 8 (the oldest ends) |
| Results per request | 1000 (the page shows 100 at a time; the agent's tool gives at most 200) |
| Bytes per read or write (`bytes`, `utf8`, `utf16`, the hex view) | 65,536 |
| Frozen values per context | 256 |
| Freeze interval | 50 ms: a timer of the page or worker itself writes the value back about 20 times a second; slower while the tab is in the background; the program can still see its own value between two writes |
| Watch entries per request | 256 (`wasm_watch`: 32) |
| Pointer path | 8 offsets |
| Instances kept per context | 256 |
| Exports listed per instance | 2000 (memories, globals, tables, tags first; the rest are counted). `wasm_memories` names 40 globals and 40 functions per instance |
| Export name | cut to 200 characters |
| A global's new value | 64 characters |
| Contexts listed | 32 |
| Time: a context's set-up | 3 s, then it is listed as "did not answer in time" |
| Time: a call | 10 s |
| Time: a scan | 120 s from the page and `GM.wasm`; the agent's tools end every call after 10 s. A scan works in slices of 30 ms so the page keeps running |
| An agent tool's result | 8,000 characters (longer results are cut and say so) |

## User scripts: GM.wasm

**Getting access.** A script asks with `// @grant GM.wasm` (or `GM_wasm`) in its metadata block.
There is no prompt per grant: the user allows it by installing the script (krunk://scripts → New
script or Import, where the source with its `@grant` lines is shown and saved by the user) and
can turn the script off or remove it there. In the list a script with this grant carries the tag
**WASM memory**. User scripts run only while Settings → AI and scripts → **Run userscripts** is
on; with it on, the AI agent may also save a script (`save_userscript`, no further prompt): it is
tagged "created by AI" in the list. Without the grant, `GM.wasm` and `GM_wasm` are
`undefined`; a call smuggled past the loader is refused by the host (`GM.wasm needs @grant
GM.wasm`), which reads the grant from the stored script, not from the page. A script only ever
reaches the tab it runs in.

Every function returns a promise. An error rejects it with an `Error` whose message says what
was wrong (`no memory 3 (list the memories first)`, `address 1114128 + 4 is past the end of the
memory (1114112 bytes)`, `not a valid request: …`, `at most 65536 bytes at once`, `the global
VERSION is not mutable`, `The worker did not answer in time.`). Every argument object may carry
`ctx` (default 0, the page); wherever `addr` is taken, `where: {base, offsets}` may be given
instead.

| Function | Arguments | Resolves to |
|---|---|---|
| `memories()` | – | `[{ctx, kind: 'page' \| 'frame' \| 'worker', url, mem, bytes, pages, shared}]` |
| `instances()` | – | `[{ctx, kind, url, inst, counts: {function, memory, global, table, tag, other}, exports: [{index, name, kind, mem?, type?, mutable?, value?, length?}], omitted}]`; `mem` on a memory export is its number in `memories()` |
| `globals(o)` | `{inst}` | `[{index, name, kind: 'global', type, mutable, value}]`, the values now |
| `setGlobal(o)` | `{inst, export, value}` (`export`: the global's `index`) | the value afterwards. Mutable `i32 i64 f32 f64` globals only |
| `read(o)` | `{mem, addr, type, length?}` | the value |
| `write(o)` | `{mem, addr, type, value}` | the number of bytes written |
| `bytes(o)` | `{mem, addr, length?}` (default 256) | `{addr, bytes, size}`: raw bytes, cut at the end of the memory; `size` is the memory's size now |
| `scan(o)` | `{mem, type, kind, value?, to?, align?, epsilon?}`; `kind`: `exact`, `range` (`value`…`to`), `bigger`, `smaller`, `unknown` | `{scan, mem, type, count, truncated, scannedBytes, snapshot}` |
| `next(o)` | `{scan, kind, value?, to?}`; `kind`: `exact`, `range`, `changed`, `unchanged`, `increased`, `decreased`, `increasedBy`, `decreasedBy` | the same. Only addresses whose value fits are kept: `decreased` asked before the value changed leaves none (look at `results` first, as the example does) |
| `results(o)` | `{scan, offset?, limit?}` (default 100, at most 1000) | `{scan, count, offset, rows: [{addr, value, previous}]}`; a scan for an unknown value has rows after its first `next` |
| `discard(n)` | the scan's number, or `{scan, ctx}` | `true` if it was there |
| `freeze(o)` | `{mem, addr, type, value?}` (without `value`: the value it has) | the freeze's number |
| `unfreeze(n)` | the freeze's number, or `{freeze, ctx}` | `true` if it was frozen |
| `frozen(o?)` | `{ctx?}` | `[{freeze, mem, where, type, value, error}]` |
| `watch(entries, ctx?)` | `[{mem, addr, type, length?}]` | per entry `{addr, value}` or `{error}` |

`align` (1, 2, 4, 8; default the type's size) scans unaligned values; `epsilon` is the tolerance
of float comparisons (default 0.001 for `f32`, 1e-9 for `f64`). Scans and freezes end with the
document (a reload or navigation), with the tab, and when the tab's last inspector page closes.

**A worked example.** The test game (`scripts/vm-www/wasm-game/`) starts with health 100. The
script scans for 100, waits until a hit lowers one of the addresses it found, narrows the list
with "decreased", then writes 500 there and holds it:

```js
// ==UserScript==
// @name        Hold the health
// @match       *://*/*wasm-game/*
// @grant       GM.wasm
// @run-at      document-idle
// ==/UserScript==
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  // The game makes its memory a moment after the page loads.
  let mems = [];
  while (!mems.length) { mems = await GM.wasm.memories(); if (!mems.length) await sleep(200); }
  const { ctx, mem } = mems[0];
  // First scan: every aligned i32 that is 100 now.
  const first = await GM.wasm.scan({ ctx, mem, type: 'i32', kind: 'exact', value: 100 });
  // Wait for a hit: one of those addresses then holds less than at the scan.
  for (;;) {
    await sleep(200);
    const { rows } = await GM.wasm.results({ ctx, scan: first.scan, limit: 1000 });
    if (rows.some((r) => r.value < r.previous)) break;
  }
  // Next scan: keep the addresses whose value went down.
  const left = await GM.wasm.next({ ctx, scan: first.scan, kind: 'decreased' });
  if (left.count !== 1) return;
  const { rows } = await GM.wasm.results({ ctx, scan: first.scan });
  const addr = rows[0].addr;
  await GM.wasm.write({ ctx, mem, addr, type: 'i32', value: 500 });
  await GM.wasm.freeze({ ctx, mem, addr, type: 'i32', value: 500 });
})();
```

(`ui/dev/test-wasm-agent.cjs` runs this very block against the game.)

## AI agent tools

Page tools: they act on the agent's target tab and need the same permission as the other page
tools. Arguments: `ctx` (default 0) on every tool; `addr` with optional `offsets` (a pointer
path) wherever an address is taken.

| Tool | Arguments | Result |
|---|---|---|
| `wasm_memories` | – | `{contexts: [{ctx, kind, url, memories: [{mem, bytes, pages, shared}], instances: [{inst, counts, memories: [{name, mem}], globals: [{export, name, type, mutable, value}], functions: [names]}], error?}]}` |
| `wasm_read` | `mem, addr, offsets?, type, length?` (all thirteen types; `bytes` is the hex read) | `{addr, value}` |
| `wasm_write` | `mem, addr, offsets?, type, value` | `{addr, written}` |
| `wasm_scan` | `mode: "first"`: `mem, type, kind` (`exact`, `range`, `bigger`, `smaller`, `unknown`), `value?, to?`. `mode: "next"`: `scan, kind` (`exact`, `range`, `changed`, `unchanged`, `increased`, `decreased`, `increasedBy`, `decreasedBy`), `value?, to?` | `{scan, mem, type, count, truncated, scannedBytes, snapshot}` |
| `wasm_results` | `scan, offset?, limit?` (default 50, at most 200) | `{scan, count, offset, rows: [{addr, value, previous}]}` |
| `wasm_freeze` | `mem, addr, offsets?, type, value?` | `{freeze, addr, value}` |
| `wasm_unfreeze` | `freeze` | `{unfrozen}` |
| `wasm_watch` | `entries: [{mem, addr, offsets?, type, length?}]` (1 to 32) | per entry `{addr, value}` or `{error}` |
| `wasm_global` | `inst`; to write also `export` and `value` | without `value`: the instance's globals `[{index, name, type, mutable, value}]`; with: `{inst, export, type, value}` |

**Refused:** on Krunk's own pages and on pages the agent may not act on (the same rule as every
page tool); while a password or card field of the page holds a value (as `eval_js`: what the
tools read goes to the model's provider); when the request is malformed or over a limit (the
error says which). A scan must finish within the tool time limit of 10 seconds. Results are
scrubbed of the page's secrets and capped at 8,000 characters like every page tool's. Freezes
the agent sets stay until the tab navigates or closes, `wasm_unfreeze`, or until an inspector
page for the tab is closed.

The agent can also write a user script that uses `GM.wasm` (`save_userscript`, offered while
"Run userscripts" is on); such a script keeps working on later visits without the agent.

## Security notes

- **Which rule applies where.**

  | Rule | krunk://wasm | GM.wasm | wasm_* tools |
  |---|---|---|---|
  | Never on Krunk's own pages, extension pages, empty pages (`core::wasm::inspectable`) | yes | yes | yes |
  | Only the tab it belongs to | the tab it was opened from (bound by the host; requests name no tab) | the tab the script runs in | the agent's target tab |
  | Who may send requests | only the `krunk://wasm` page kind | only a stored script with `@grant GM.wasm` (checked by the host per call) | only an agent run with page permission |
  | Requests typed and checked in `core::wasm` (closed set of operations, sizes within the limits, no code, no property names) | yes | yes | yes |
  | Refused while a password or card field holds a value | no | no | yes |
  | Result scrubbed and capped | no (shown with `textContent`) | no | yes |

  The password rule is deliberately the agent's alone. It exists because what an agent tool
  reads leaves the computer (it goes to the model's provider). The page shows memory to the
  user who is sitting at it, and a user script already runs inside the page and could read the
  field itself: refusing `GM.wasm` there would protect nothing.
- **The probe** lives in the inspected page's main world as an object only the host's DevTools
  session has a handle to; a page can't call it. A hostile page can still lie to it (replace
  `WebAssembly.Memory.prototype`'s `buffer` getter, say): everything it returns is
  page-controlled data.
- **`GM.wasm` answers** come back through a function on `window` that settles a promise only
  for the tab's nonce, which page code never sees.
- **Global types** are found by instantiating 18-byte modules (one imported global, no code) in
  the page's context; nothing is written to a global for that.
- Sessions are in memory only; nothing of a memory is stored or sent anywhere by the inspector
  itself.
