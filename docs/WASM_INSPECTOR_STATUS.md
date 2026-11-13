# WASM memory inspector: what the code has, point by point

Checked against the code of this branch (not against earlier reports). Files:
`ui/inject/wasm-probe.js` (the probe, runs next to the memories), `core/src/wasm.rs` (the typed
requests), `app/src/browser/wasm.rs` (the host: DevTools sessions), `ui/pages/wasm.{html,js,css}`
(`krunk://wasm`), `scripts/vm-www/wasm-game/` (the test app).

Tests, and the short names used in the table:
- **core**: unit tests in `core/src/wasm.rs` (`cargo test -p core --lib wasm`).
- **probe**: `ui/dev/test-wasm-probe.cjs`, the probe in Node with real `WebAssembly` objects
  (`node ui/dev/test-wasm-probe.cjs`, or `ONLY=WasmProbe` in `test-ui.cjs`); its fixture module
  is `ui/dev/wasm-fixture.cjs`.
- **page**: `ui/dev/test-0190-wasm.cjs`, `krunk://wasm` in Playwright with a mock host that
  answers with the real probe (`ONLY=WasmPage,WasmMenu`).
- **e2e**: `ui/dev/test-wasm-e2e.cjs`, the DevTools calls the host makes, in Playwright's
  Chromium against the test app (page, hook, worker session).

What no automated test covers, for every row: **the Rust host itself**
(`app/src/browser/wasm.rs`, `app/src/ffi/cef/`) is compiled and linted for Windows but only
runs inside Krunk. Its DevTools calls are the ones the e2e check makes by hand; that the host
makes them correctly is proven only by a run on Windows (the checklist at the end; results in
`.pipeline/evidence-0.19.0/vm/`).

Found by that run (2026-11-13) and fixed: calls into a **worker's session never came back**.
CEF parses a DevTools answer only in the form `{"id":N,"result":{…}}`; the answer of a call
made in an attached target's (flat) session ends in `,"sessionId":"…"}` and CEF drops it
(`libcef/browser/devtools/devtools_util.cc: ParseResult`). The observer now takes such answers
from the raw message (`on_dev_tools_message` → `core::wasm::session_answer`, core test
`answers_of_a_worker_session_are_read_from_the_raw_message`). Also since then: a context that
does not answer the two set-up calls is reported after 3 seconds (`SETUP_TIMEOUT_MS`), the page
runs one search at a time, and its header follows the inspected tab's title. All contexts are
asked at the same time. A worker of a page the tab keeps for Back (frozen in the back/forward
cache) stays attached and answers DevTools, so its memory showed up under the next page: the
host's survey of a worker now waits for a task of the worker's own, which a frozen worker never
runs, and leaves such a worker out. Result rows are updated in place (a click on Watch during
the once-a-second refresh was lost).

Windows run of 2026-11-13 (screenshots `y01`–`y13` in `.pipeline/evidence-0.19.0/vm/`): the
checklist's steps 1–7 and 9–13 passed, the worker variant included (list, scan → Hit →
Decreased, Grow to 18 pages, global edit), and `index.html?stuck` (a worker that spins) is
reported as `The worker did not answer in time.` about four seconds after the page opens.
Not run on Windows: step 8 (a next scan after the growth), a same-process frame, a shared
memory, the hook fallback, a back navigation to a page with a worker.

| Sub-point | Code | Tests | Verdict |
|---|---|---|---|
| Enumerate the memories of a tab | probe `adoptMem`/`list`; host `wasm_list` → `wasm_query` → `wasm_find` (`Runtime.queryObjects` on `WebAssembly.Memory.prototype`) per context: page, same-process frames, dedicated workers | probe "list: one memory…", "shared memory listed as shared", "a shared memory is listed … no SharedArrayBuffer global"; e2e "queryObjects finds the game's memory", "queryObjects in the worker's session finds its memory and its instance"; page "lists the memory with its size" | done. Not tested: a same-process **frame** (the host passes the frame's `contextId`; no check does) |
| Enumerate the instances of a tab | probe `adoptInst`/`instances`/`describeExport` (name, kind, place; `mem` = which listed memory an exported memory is), op `survey` = memories + instances; hook `keep` catches instances from `new Instance`, `instantiate`, `instantiateStreaming`; `WasmReq::Instances`; host `wasm_query` (second query on `WebAssembly.Instance.prototype`), `wasm_list_next` (`instances` per context); page `renderInstances`/`renderInstance`/`ownersOf` | core `instance_requests_round_trip_and_are_checked`, `the_probe_has_the_same_limits…`; probe "survey: 3 memories and 2 instances", "an exported memory names the listed memory it is", "a memory no instance exports stays listed", "the exports are counted by kind", "at most 2000 exports…", "the hook finds instances…"; e2e "queryObjects finds the game's instance", "its export "memory" is the listed memory", "hook: the instance is caught too", worker; page "instances: …" (5 checks) | done |
| Globals of an instance | probe `describeGlobal` (exact type and mutability by linking one-import modules; nothing written), `globalValue`, `globals`, `setGlobal` (mutable `i32 i64 f32 f64`); `WasmReq::Globals`/`SetGlobal`, `check_global_value`; page globals table, `refreshGlobals`, `writeGlobal` | core as above; probe "mutable globals: exact type and value…", "an immutable global…", "finding the types wrote nothing", "setGlobal …" (6 checks), "without compiling: i64 or "number"…"; e2e "__stack_pointer: a mutable i32 global", "VERSION: an immutable i32 global…", "setGlobal: the page reads the new value…", "setGlobal on the immutable one is refused", worker "its mutable global is written"; page "globals: …" (6 checks) | done for **exported** globals. Not possible: globals that are not exported (JavaScript can't reach them). `v128` and reference globals are listed without a value; reference types beyond `funcref`/`externref` show as `other` |
| Surviving `memory.grow` | every operation calls `view(m)` (takes `m.buffer` again); `firstScan`/`nextScan` take the buffer again after each pause (a grow during a paused scan detached the old one: fixed here); page `grew` (list and hex view follow the size) | probe `runGrow` twice (not shared, shared; 15 checks each: list, read, write, pointer path, hex bytes, text, old result list, next scan on it, snapshot → changed, freeze from before, freeze past the old size, second growth, watch, new scan), `runGrowDuringScan` (aligned, byte-wise, snapshot; both kinds); e2e "memory.grow (page)" (12 checks, grown by the game's own button), "shared memory after grow…", worker "after memory.grow…"; page "grow: …" (8 checks) | done. By design: a scan that the memory grows under covers the size it started with; a next scan after "unknown initial value" covers what the snapshot covered |
| Typed read/write | probe `readAt`/`writeAt`; `WasmReq::Read`/`Write`, `check_value` | core `requests_round_trip…`, `malformed_and_oversized…`, `numbers_may_come_as_text`; probe "read i32/i64/utf8/f32", "write u64/utf16", refusals; page "typing a value and Enter writes it"; e2e "writing 999 shows in the game" | done |
| Hex view | probe ops `bytes`/`writeBytes`; page `hexLoad`/`hexRender`/`hexType` | page "go to 0x800…", "typing two hex digits writes the byte", "the next page of the window", "grow: the hex view reaches the last row…"; probe/e2e hex window after grow | done |
| First and next scans, result lists | probe `firstScan`/`nextScan`/`results`; `WasmReq::Scan`/`Next`/`Results`; page `firstScan`/`nextScan`/`loadResults` | core as above; probe "first scan exact…", "next scan decreased", "decreasedBy", "unknown scan…", "f32 range scan", "u8 scan", "64 MiB scan"; page "first scan: 2 addresses", "next scan narrows to 1 address"; e2e scan → next (page and worker) | done |
| Freeze | probe `freeze`/`tick`/`unfreeze`/`frozen` (a 50 ms timer of the page or worker) | probe "freeze: the value is written back", "unfreeze…", grow checks; page "a frozen value is written back"; e2e "frozen at 500 although hit twice", worker "a frozen value holds" | done |
| Watch list | probe `watch`; page `addWatch`/`refreshWatch`, export/import | probe "watch reads entries and reports bad ones"; page "the result is on the watch list", export/import checks | done |
| Pointer/offset entries | probe `resolve` (32-bit pointers, ≤ 8 offsets); `Where::Path`; page `parseWhere` | probe "pointer path [300]+8", "a pointer path into the new pages resolves"; page "a pointer path is followed"; e2e "gold read through the pointer path" | done (32-bit only: memory64 is not followed) |
| The user UI | `ui/pages/wasm.*`: Memories, Instances (+ globals table), Scan, results, Watch list, Hex view; ⋮ menu row (`open_wasm`) | page test (all of the above), "the ⋮ menu row", "at 420 px nothing sticks out" | done |
| Rust→WASM test app | `scripts/vm-www/wasm-game/`: `src/lib.rs`, `build.rs`, `Cargo.toml`, `Cargo.lock`, `build.sh` (`cargo build --release --target wasm32-unknown-unknown`; `./build.sh --check` builds from scratch into a temporary directory and compares with the committed `game.wasm`: identical), `index.html`, `game.js`, `worker.js` | the e2e check runs against it | done: health on a button (Hit/Heal), score (f32), gold behind a pointer, `memory.grow` on a button (a marker 4242 in the new page), a mutable exported global (`__stack_pointer`), an immutable one (`VERSION`), worker variant (`index.html?worker`). Note: stable Rust can't declare a mutable WebAssembly global; the stack pointer, exported by a linker flag in `build.rs`, is the one a Rust module has |
| Security rules and caps | `wasm::inspectable` (never Krunk's own pages; `wasm_call`, `wasm_hook`); only `Page::Wasm` may send the requests (`protocol.rs`); caps in `LIMITS` (probe) and the constants of `core::wasm` | core `pages_expressions_and_errors`, `the_probe_has_the_same_limits…`, protocol tests; page "a tab on one of Krunk's own pages: says so", "krunk://wasm itself can't compile WebAssembly" | kept. New caps: 256 instances per context, 2000 exports listed per instance, names cut to 200 characters, a global's value text ≤ 64 characters, export place < 1,000,000. "Refused while a password or card field holds a value" is, as before, a rule of the **agent's `wasm_*` tools only** (`app/src/browser/agent/page.rs: tool_wasm`); the page and `GM.wasm` never had it |

Not part of this round (the next milestone): `GM.wasm` and the agent's `wasm_*` tools know
nothing of instances and globals. They compile and pass as before; the agent's `wasm_memories`
answer is kept in its old shape (the host's `instances` are taken out for it).

## Checklist for a run in Krunk on the Windows VM

Page: `http://<host serving scripts/vm-www>/wasm-game/index.html` (the title starts
`WASM health=100 score=0 gold=250 pages=17 marker=0 sp=1048576`).

1. On the game tab: ⋮ → **WASM memory inspector**. Must show: Memories: `Memory 0`, `Page`,
   `export "memory" of instance 0`, `1.1 MiB · 17 pages`. Instances: `Instance 0`,
   `14 functions · 1 memory · 2 globals · Memory 0`. Globals table: `__stack_pointer`, `i32`,
   an input with `1048576`; `VERSION`, `i32 (constant)`, `1048576`.
2. Scan: type `i32`, Exact value, `100`, **First scan** → `1 address found.`, row `0x100004`,
   value `100`.
3. Game tab: **Hit (−10)** (health 90). Inspector: Find `Decreased`, **Next scan** →
   `1 address found.`, `0x100004`, value `90`, previous `90`.
4. **Watch** on the row; in the watch row type `999`, Enter → the game shows Health 999. Tick
   **Freeze**; in the game press Hit twice → Health stays 999.
5. Watch list: add `0x110010`, type `i32`, **Add** → the row's value field holds no value; it
   shows, greyed, `address 1114128 + 4 is past the end of the memory (1114112 bytes)`.
6. Game tab: **Grow memory (+1 page)** → the game shows `18` pages, marker `4242 at 0x110010`.
   Inspector, within about two seconds and with nothing clicked: the `0x110010` row reads
   `4242`; the memory row and the hex view's title say `1.1 MiB · 18 pages`; the results row
   `0x100004` still shows `999`; after another Hit the game's health is still 999.
7. In the `0x110010` watch row type `7`, Enter → the game shows marker `7 at 0x110010`.
8. Find `Increased`, **Next scan** → still `1 address found.` (`0x100004`, value `999`).
9. Hex view: Go to `0x11FFF0` → one row, `0011FFF0` (the last 16 bytes of the grown memory).
10. Globals: in `__stack_pointer` type `1048560`, Enter → the game shows
    `Global __stack_pointer 1048560`; the game's buttons still work.
11. Watch list: add `0x100040 > 8` as `i32` → `0x100040 > 0x8 = 0x100028`, value `250` (gold,
    through the pointer to the player); **Spend 20 gold** in the game → `230`.
12. Worker variant: open `…/wasm-game/index.html?worker`, ⋮ → WASM memory inspector. Must show
    `Memory 0`, `Worker http://…/wasm-game/worker.js`, and `Instance 0` with the same two
    globals; repeat steps 2–3, 6 and 10 there (the game page shows what the worker reports).
13. On `krunk://settings`: ⋮ → WASM memory inspector → `Only web pages can be inspected…`,
    controls off.
