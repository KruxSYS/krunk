# WASM memory inspector: what the code has, point by point

Checked against the code of this branch (not against earlier reports). Files:
`ui/inject/wasm-probe.js` (the probe, runs next to the memories), `core/src/wasm.rs` (the typed
requests), `app/src/browser/wasm.rs` (the host: DevTools sessions), `ui/pages/wasm.{html,js,css}`
(`krunk://wasm`), `scripts/vm-www/wasm-game/` (the test app). Tests: `core/src/wasm.rs` unit
tests, `ui/dev/test-wasm-probe.cjs` (the probe in Node, `ONLY=WasmProbe`),
`ui/dev/test-0190-wasm.cjs` (the page in Playwright against the real probe,
`ONLY=WasmPage,WasmMenu`), `ui/dev/test-wasm-e2e.cjs` (the host's DevTools calls in Chromium
against the test app).

State before this round of work (commit 93a2af7):

| Sub-point | Code | Test | Verdict |
|---|---|---|---|
| Enumerate the memories of a tab | probe `adopt`/`list`; host `wasm_list` → `wasm_query` (`Runtime.queryObjects` on `WebAssembly.Memory.prototype`) per context: page, same-process frames, dedicated workers | probe "list: one memory"; e2e "queryObjects finds the game's memory"; page "lists the memory with its size" | done. Frames and workers: the host code is there, but no automated test runs `queryObjects` in a frame or a worker session (the e2e only runs the probe inside a worker) |
| Enumerate the **instances** of a tab | none: the hook looks at an instance's exports only to pick out memories (`hook` → `keep`) | none | **not done** |
| Surviving `memory.grow` | every operation calls `view(m)` (takes `m.buffer` again) | probe "grow: list shows 3 pages", "grow: a scan sees the new page" | partly: only list and a first scan are tested, non-shared only. Gap in the code: a scan that yields (30 ms slices) keeps the buffer it started with; a grow in between detaches it (the rest of the scan finds nothing or throws) |
| Typed read/write | probe `readAt`/`writeAt`; `WasmReq::Read`/`Write`, `check_value` | core `requests_round_trip…`, `malformed_and_oversized…`, `numbers_may_come_as_text`; probe "read i32/i64/utf8/f32", "write u64/utf16"; page "typing a value and Enter writes it" | done |
| Hex view | probe ops `bytes`/`writeBytes`; page `hexLoad`/`hexRender`/`hexType` | page "go to 0x800…", "typing two hex digits writes the byte", "the next page of the window" | done |
| First and next scans, result lists | probe `firstScan`/`nextScan`/`results`; `WasmReq::Scan`/`Next`/`Results`; page `firstScan`/`nextScan`/`loadResults` | core as above; probe "first scan exact…", "next scan decreased", "decreasedBy", "unknown scan…", "f32 range scan", "u8 scan", "64 MiB scan"; page "first scan: 2 addresses", "next scan narrows to 1 address"; e2e scan → next | done |
| Freeze | probe `freeze`/`tick`/`unfreeze`/`frozen` (50 ms timer of the page) | probe "freeze: the value is written back", "unfreeze…"; page "a frozen value is written back"; e2e "frozen at 500 although hit twice" | done |
| Watch list | probe `watch`; page `addWatch`/`refreshWatch`, export/import | probe "watch reads entries and reports bad ones"; page "the result is on the watch list", export/import checks | done |
| Pointer/offset entries | probe `resolve` (32-bit pointers, ≤ 8 offsets); `Where::Path`; page `parseWhere` | probe "pointer path [300]+8"; page "a pointer path is followed"; e2e "gold read through the pointer path" | done (32-bit only; memory64 is not followed) |
| Globals of an instance | none | none | **not done** |
| The user UI | `ui/pages/wasm.*`, ⋮ menu row (`open_wasm`) | page test, "the ⋮ menu row" | done for memories; nothing for instances |
| Rust→WASM test app | `scripts/vm-www/wasm-game/` (`src/lib.rs`, `Cargo.toml`, `build.sh`, `game.wasm`, page, worker) | e2e runs against it | partly: health, score, gold behind a pointer, worker variant are there; **no mutable exported global, no `memory.grow`** |
| Security rules and caps | `wasm::inspectable` (never Krunk's own pages, `wasm_call`/`wasm_hook`); only `Page::Wasm` may send the requests (`protocol.rs`); caps in `LIMITS` (probe) and `core::wasm` constants | core `pages_expressions_and_errors`, protocol tests | done. Note: "refused while a password or card field holds a value" is a rule of the **agent's `wasm_*` tools only** (`app/src/browser/agent/page.rs: tool_wasm`); the page and `GM.wasm` have no such rule |

The table at the end of this file is rewritten when the gaps above are closed.
