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

## The three surfaces, operation by operation

One path for all three: a typed request (`core/src/wasm.rs: WasmReq`, checked by `WasmReq::check`)
→ the host's session (`app/src/browser/wasm.rs: wasm_call`) → the probe
(`ui/inject/wasm-probe.js`). Where each surface enters it:
- **UI** (`krunk://wasm`): `ui/pages/wasm.js: call` → `Request::WasmCall` (`protocol.rs`, only
  from `Page::Wasm`) → `dispatch_wasm` → `wasm_call`.
- **Script API** (`GM.wasm`): `ui/inject/userscript-loader.js: wasmApi` builds `{ctx, req}` →
  the script binding → `app/src/browser/userscripts.rs` (`GmCall::Wasm`: the grant is read from
  the stored script) → `core::userscripts::wasm_body` (→ `WasmReq::parse`) → `wasm_call`; the
  answer returns through `core::userscripts::gm_reply`.
- **AI tool** (`wasm_*`): `core/src/agent/tools.rs` (schemas) → `app/src/browser/agent/page.rs:
  tool_wasm` (the password/card refusal) → `core::wasm::agent_request` (→ `WasmReq::parse`) →
  `wasm_call`; `agent_trim_instances` shortens the instance lists of `wasm_memories`.

Tests named below: **page** = `ui/dev/test-0190-wasm.cjs`; **probe** / **loader** =
`ui/dev/test-wasm-probe.cjs` (its `loader:` checks run the real loader and compare every body
sent with `ui/dev/wasm-loader-cases.json`, which the core test
`userscripts::gm_wasm_bodies_from_the_shared_file` passes through `wasm_body`); **agent** =
`ui/dev/test-wasm-agent.cjs` (the mock provider's scenarios played against the real probe and the
test game; user scripts run through the real loader; its translation of tool calls is checked
against `ui/dev/wasm-agent-cases.json`, which the core test
`wasm::agent_cases_from_the_shared_file` runs through `agent_request` and the tools' schemas);
**e2e** = `ui/dev/test-wasm-e2e.cjs`.

| Operation | UI | Script API | AI tool |
|---|---|---|---|
| Enumerate memories (page, frames, workers) | Memories list, **Find memories**. page "lists the memory with its size"; e2e (page, worker) | `memories()`. loader "memories() is a flat list"; agent "GM.wasm: memories() and instances()" | `wasm_memories`. agent "wasm_memories: the memory (17 pages) and the instance…" |
| Enumerate instances and their exports | Instances list. page "instances: …" | `instances()` (added). loader "instances() asks for the list…" (a worker's instance); agent as above | `wasm_memories` (added: instances in a short form). core `the_agent_gets_a_short_form_of_the_instances`; agent as above |
| Read/write `i8 u8 i16 u16 i32 u32 i64 u64 f32 f64` | watch list (type select, Enter writes); scans in all ten. page "typing a value and Enter writes it"; probe "read i32/i64/f32", "write u64" | `read(o)`, `write(o)`. loader "write u8 / i64 / f64"; agent "GM.wasm: write and read of all thirteen types" | `wasm_read`, `wasm_write`. core cases (every type); agent "wasm_write and reads of i8 … utf16" |
| Read/write bytes | hex view (type hex digits). page "typing two hex digits writes the byte" | `read/write` type `bytes`, `bytes(o)` (added). loader "write bytes", "bytes({mem, addr})…"; agent "GM.wasm: bytes() (hex read)" | `wasm_read` / `wasm_write` type `bytes` (write added). agent "wasm_read type bytes: a 64-byte hex window" |
| Read/write UTF-8 | watch list type `utf8` (32 bytes). page "text entries: …"; probe "read utf8 up to the NUL" | `read/write` type `utf8`. loader "write utf8"; agent thirteen types | `wasm_read` / `wasm_write` type `utf8`. agent thirteen types |
| Read/write UTF-16 | watch list type `utf16` (added; 32 bytes). page "text entries: utf8 reads "hi", utf16 reads "hé"", "text entries: Enter writes UTF-16" | `read/write` type `utf16`. loader "write utf16", "read utf16 with a length"; agent thirteen types | `wasm_read` / `wasm_write` type `utf16`. agent thirteen types |
| Hex read | Hex view, go to address, Live. page "go to 0x800…", "grow: the hex view reaches the last row" | `bytes(o)`; `read` type `bytes` | `wasm_read` type `bytes` with `length` |
| First scan: exact | Find "Exact value". page "first scan: 2 addresses" | `scan({kind: 'exact'})`. agent "GM.wasm: scan exact, next …" | `wasm_scan` mode first. agent "wasm_scan first exact → next decreased → wasm_results" |
| First scan: range | "Value between". probe "f32 range scan" | `scan({kind: 'range', value, to})`. loader "scan (range)"; agent "GM.wasm: scan range and unknown, discard" | `wasm_scan` kind range. agent "wasm_scan first: range, and unknown initial value…" |
| First scan: unknown initial value | "Unknown initial value". page "an unknown-value scan takes a snapshot" | `scan({kind: 'unknown'})`. loader "scan (unknown initial value)"; agent as above | `wasm_scan` kind unknown. agent as above |
| Next scan: changed / unchanged / increased / decreased / exact | Find select after a first scan. page "next scan narrows to 1 address" (decreased), "grow: a next scan…" (increased); probe (changed, increased, decreasedBy) | `next({scan, kind})`. loader "next (changed / unchanged / increased / decreased)", "next (exact, in a worker)"; agent "GM.wasm: scan exact, next decreased / exact / unchanged / increased / changed" | `wasm_scan` mode next. core cases (all five); agent "wasm_scan next: exact, unchanged, increased, changed each keep the address" |
| Result lists | results table (100 at a time, Show more to 1000). page "the results list addresses in hex", "a refresh changes the values in place" | `results({scan, offset, limit})`. loader "results" | `wasm_results` (≤ 200). core cases; agent |
| Freeze / unfreeze | Freeze checkbox. page "a frozen value is written back", "Remove ends the freeze" | `freeze(o)`, `unfreeze(n)`, `frozen()` (added). loader "freeze through a pointer path…", "unfreeze takes the freeze's number", "frozen()"; agent "GM.wasm: pointer path, write, read, freeze (holds through a hit), frozen, unfreeze" | `wasm_freeze`, `wasm_unfreeze`. agent "wasm_write, wasm_read, wasm_freeze", "wasm_unfreeze", health scenario |
| Watch list | Watch list with labels, Export/Import. page export/import checks | `watch(entries, ctx)` (added: many values in one call; the list itself is the script's). loader "watch(entries, ctx)…"; agent "GM.wasm: watch (address, pointer path, and an error per entry)" | `wasm_watch` (added, ≤ 32 entries). core cases; agent "wasm_watch: the frozen health after a hit (999), the gold through the pointer, raw bytes" |
| Pointer/offset entries | `0x500 > 8` in the watch list. page "a pointer path is followed" | `where: {base, offsets}` wherever `addr` is taken. loader "ctx and a pointer path pass through" | `offsets` next to `addr`. core cases; agent "wasm_read through a pointer path (offsets [8]) reads the gold" |
| Across `memory.grow` | sizes follow, results/watch/freeze go on. page "grow: …" (8 checks) | nothing to call: the same numbers stay valid. agent "GM.wasm across memory.grow: the freeze, the new page, the old results, a next scan, 18 pages" | the same. agent "memory.grow: an address past the end is refused before, read after (4242); the freeze holds through it", "…the result list from before and a next scan on it" |
| Globals: read | globals table (live). page "globals: the table lists them…" | `globals({inst})`, `instances()` (added). loader "globals({inst, ctx})"; agent "GM.wasm: globals(), setGlobal()…" | `wasm_global` without `value` (added), `wasm_memories`. core cases; agent "wasm_global: reads the globals, writes the mutable one" |
| Globals: write | Enter in the table. page "globals: Enter writes a mutable global…" | `setGlobal({inst, export, value})` (added). loader "setGlobal(…) gives the value afterwards (a name is not sent)"; agent as above (the constant one rejects) | `wasm_global` with `export` and `value` (added). core cases; agent as above |

Verdict: every operation of the list is on all three surfaces and tested there. Scans are for
the ten number types on every surface (text is not scanned for).

**Granting a user script.** `// @grant GM.wasm` (or `GM_wasm`) in the script's metadata. There is
no per-grant prompt: the user grants by installing the script in `krunk://scripts` (the source
is in front of them) and sees the tag **WASM memory** on such a script in the list (page test
"a script with the grant has the "WASM memory" tag…"). Without the grant the script's `GM.wasm`
and `GM_wasm` are `undefined` (loader "a script without the grant gets neither"; agent "a script
without @grant GM.wasm has no GM.wasm and no GM_wasm"), and the host refuses a call anyway (it
reads the grant from the stored script: `userscripts.rs`, core `gm_wasm_grant_call_and_reply`).

**Security rules per surface** (unchanged by this round; decided and written down in
`docs/WASM_INSPECTOR.md`, "Security notes"): never on Krunk's own pages (all three); typed,
checked requests (all three); the refusal while a password or card field holds a value stays
with the agent's tools alone, because only there does what is read leave the computer (to the
model's provider); a user script already runs inside the page, and the page is the user looking
at their own tab. Results are scrubbed and capped (8,000 characters) for the agent only.

**Limits:** `docs/WASM_INSPECTOR.md`, "Limits" (scan 512 MiB and 4,194,304 addresses; results
1000 per request, 200 for the agent; freeze interval 50 ms; 3 s set-up, 10 s per call, 120 s per
scan from the page and scripts, 10 s for an agent tool; `wasm_watch` 32 entries). Core test
`the_probe_has_the_same_limits_and_takes_what_the_host_sends` keeps the numbers of the probe and
of core the same.

## Shown on Windows (2026-11-13, second round: scripts, agent, the open points)

Evidence in `.pipeline/evidence-0.19.0/vm/` (screenshots `z…png`; `z03-krunk-log.txt`,
`z05-z09-krunk-log.txt`, `z10-z14-krunk-log.txt` from krunk.log; `z-mock.jsonl`, the mock
provider's full log, and `z-mock-requests.txt`, one line per request). The AI provider was the
mock (`ui/dev/mock-provider.cjs` behind `ssh -R`, profile set up by `scripts/vm-0190.ps1`).
Points (a), (b) and the next scan of (c) ran on the build of commit "krunk://wasm: UTF-16 watch
entries"; Back in (c) and all of (d) on the build one fix later (the workers asked for anew).

| Point | What was done | Evidence |
|---|---|---|
| (a) a user script with `GM.wasm` | `scripts/vm-www/wasm-game/hold-the-health.user.js` (the example of `docs/WASM_INSPECTOR.md`) imported in `krunk://scripts` with **Import .user.js…**; the list shows it with the tag **WASM memory**. On the game: health 100; after the first Hit the script's next scan finds the address, writes 500 and freezes it; three more hits: still 500 | `z01-scripts-page`, `z02-script-imported`, `z03a-script-game-before`, `z03b-script-game-after-first-hit`, `z03-script-game-after-hits`, `z03-krunk-log.txt` |
| (b) the agent's tools | "find the health value in the wasm game and set it to 999, then freeze it": `wasm_memories`, `wasm_scan` first, click Hit, `wasm_scan` next, `wasm_results`, `wasm_write`, `wasm_freeze`; two hits by hand afterwards: 999 | `z05-agent-asked`, `z06-agent-frozen-after-hits` |
| (b) every tool | "wasm tools tour": 50 tool calls (the step question after 30 answered with Continue): all nine tools, every scan kind, a pointer path, `wasm_watch`, an address past the end refused then read after Grow, the thirteen types written and read back, `wasm_global` read and write (the game shows `__stack_pointer` 1048560) | `z07a-agent-tour-30-steps`, `z07-agent-tour`, `z05-z09-krunk-log.txt` (`agent step … wasm_… ok`) |
| (b) the agent writes a `GM.wasm` script | "write a userscript that keeps the gold": `save_userscript`; the panel shows "Created userscript: Keep the gold"; after a reload of the game and two clicks on Spend: gold 9999 | `z08-agent-saved-script`, `z09-agent-script-after-reload` |
| (c) a next scan after Grow (page) | scan 100, Hit, Decreased (1 address), Grow (18 pages), Unchanged: `1 address found`, `0x100004`, 90 | `z10a-decreased-before-grow`, `z10-next-scan-after-grow` |
| (c) Back to a page with a worker | inspector open on `index.html?worker` (worker listed); the tab goes to `index.html?c2`: only the page's memory, no stale worker; Back: the worker's memory and instance are listed again | `z11a-on-worker-page`, `z11b-next-page-no-stale-worker`, `z11c-back-on-worker-page`, `z11-back-to-worker-page`, `z10-z14-krunk-log.txt` |
| (d) same-process frame | `wasm-game/frame.html` (the game in a same-origin iframe): Memory 0 and Instance 0 listed as `Frame …/index.html?framed` | `z12a-frame-page`, `z12-frame` |
| (d) shared memory | `wasm-game/shared.html`: `64 KiB · 1 page · shared`; scan i32 2024 → `0x40` | `z13-shared` |
| (d) hook-and-reload | "Hook WebAssembly and reload the tab" on the game: the tab reloads, "The hook is on", the memory and the instance are listed (krunk.log: `WASM inspector hook installed, reloading`). The screen does not tell which of the two ways found them: both feed the list | `z14-hook`, `z10-z14-krunk-log.txt` |

Found by this round and fixed: after Back to a page with a worker the inspector listed nothing
(the engine names a worker once; the host now asks for the tab's workers anew when memories are
listed and nothing is held in a worker); the mock provider's tour kept its results itself (Krunk
shortens older tool results).

Not run on Windows: `GM.wasm` and the tools in a worker or a frame (`ctx` other than 0); growth
of a shared memory; the refusal of the tools while a password field holds a value (core and
earlier agent tests only); `GM.wasm` beyond what the two scripts call (`memories`, `scan`,
`next`, `results`, `write`, `freeze`): the other functions are tested in Node only
(`test-wasm-agent.cjs`, `test-wasm-probe.cjs`); a real model provider (the mock only).

Known cost: while a page with a worker sits in the tab's back/forward cache, the first listing
after each navigation waits 3 seconds for that sleeping worker.

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
