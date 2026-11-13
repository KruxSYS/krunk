# Stuck page loads: how one request holds up others

Krunk 0.18.0. The report: a page hangs while loading, and other tabs that go to the same site
(or the same address) hang too until the first one is closed. This file says what was measured,
what causes it, what Krunk does about it and how to look at it on a running Krunk.

## What was measured, and where

Measured with `scripts/vm-www/hang-measure.cjs` against `scripts/vm-www/hangserve.py`
(a server whose answers hang on purpose, port 8120), in **Chromium 155.0.8059.12 (Playwright's
build) on Linux, 2026-11-13**, each scenario in a fresh profile, HTTP/1.1 over plain TCP.

**Not measured: Krunk itself on Windows.** Both mechanisms below live in Chromium's network
stack (`net/`), which Krunk's engine (CEF 156 / Chromium 156.0.8078.80) shares, and the two
constants were read in Krunk's own engine sources (see "Mechanisms"); but the numbers are
Chromium's on Linux, and Krunk's mitigations (below) have not run against them. "To do on
Windows" at the end lists the re-measurement.

Times are seconds after tab A started. "at the server" is the moment the server's access log
shows the request, so it tells waiting inside the browser from waiting for the server.

| # | Scenario | Result |
|---|---|---|
| 1 | Tab A navigates to a URL that never answers. Tab B navigates to the **same URL** at 1.0. | B's request reaches the server at **21.0**: it waited 20 s inside the browser. |
| 2 | As 1, tab C navigates to **another URL on the same host** at 1.0. | Done at 1.0: not held up. |
| 3 | As 1, as a subresource: two tabs `fetch()` the same never-answering URL, 1 s apart. | The second fetch reaches the server at **21.1**. |
| 4 | A's URL answers after 8 s, cacheable (`max-age=600`). B, same URL, at 1.0. | Both done at 8.0, **one** request at the server: B is served from A's cache entry. |
| 5 | A's URL answers after 8 s, `no-store`. B, same URL, at 1.0. | A done at 8.0. B's request reaches the server only at **8.1** and is done at **16.1**: the tabs are served one after the other. |
| 6 | A's URL sends headers at once, the body's end after 8 s, cacheable. B at 1.0. | Both done at 8.0, one request: B reads along with A. |
| 7 | As 6 with no cache headers, or `no-store`. | B's own request reaches the server at once (1.0 after A's); B done 8 s later. Not held up. |
| 8 | Tab A's page starts **six** requests to one host that never answer, then a seventh. | The seventh never reaches the server (watched 12 s). |
| 9 | As 8; tab B **navigates** to that host at 2.0. | Never reaches the server (watched until 12.0). When tab A is closed at 12.1, B is done at 12.1. |
| 10 | As 8; tab C navigates to **another host** (`localhost` instead of `127.0.0.1`) at 2.0. | Done at 2.0. |
| 11 | Tab A holds **six event streams** (`/sse`, nothing broken about them) to one host; tab B navigates to that host at 2.0. | Never reaches the server (watched until 10.0); done the moment tab A is closed. |
| 12 | Three hung requests in tab A, three in tab B; tab C navigates to that host at 2.0. | Never reaches the server (watched until 10.0): the six are counted across tabs. |
| 13 | As 1, but tab B's request carries `Cache-Control: no-cache` (what a hard reload sends). | B's request reaches the server at **1.0**: no wait. |
| 14 | As 5, but tab B's request carries `Cache-Control: no-cache`. | B's request reaches the server at 1.0 and is done at 9.0 (instead of 16.1). |

Run it again with:

```
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node scripts/vm-www/hang-measure.cjs
```

(It starts the server itself and prints the table's raw rows. A scenario's name as the argument
runs only that one; `HANG_LOG=file` keeps the server's access log.)

## Mechanisms

### 1. The HTTP cache lets one request at a time fetch a URL (rows 1, 3, 5, 13, 14)

Chromium's HTTP cache keeps one entry per URL (per top-level site and frame site: the cache is
partitioned), and while a request is fetching that entry, every other request for it queues
behind it **until the first one's response headers arrive**. Then either the answer can be
cached and the others share it (rows 4 and 6), or it cannot and each fetches for itself
(row 7). If the headers never come, a queued request gives up waiting after **20 seconds**
and goes to the network without the cache
(`net/http/http_cache_transaction.cc`, `AddCacheLockTimeoutHandler`:
`int timeout_milliseconds = 20 * 1000;`). That is row 1's 21.0.

What it looks like: the same page opened in a second tab (or the same script, image or API
call needed by two tabs) sits on "waiting" for 20 s although the server would answer the
second request. With a server that answers slowly and does not allow caching, the tabs are
served strictly one after the other (row 5), each waiting for all the earlier ones.

It is not tied to the tab: the queue is in the network process, per profile. Private tabs
have their own cache, so a normal tab and a private one do not hold each other up. Another
URL on the same host is not affected (row 2).

### 2. Six connections per host (rows 8 to 12)

Chromium opens at most **six** connections to one host (scheme, host and port; over HTTP/1.1,
where a connection carries one request at a time). The limit is per profile, not per tab
(`net/socket/client_socket_pool_manager.cc`, `g_max_sockets_per_group`: `6, // kNormal`).
Six requests that never finish take all six, and everything else for that host waits without a
time limit, in every tab, navigations included. Nothing has to be broken for this: six open
event streams (`EventSource`), long polls or slow downloads from one host do the same (row 11).
Closing the tab that holds them frees the connections at once (rows 9 and 11).

Not affected: other hosts (row 10); HTTP/2 and HTTP/3 hosts, where all requests share one
connection (not measured here: `hangserve.py` speaks HTTP/1.1 only); WebSockets, which have
their own limit of 255.

### What these two do not explain

A single tab whose own page hangs with nothing else open is neither of these: that is the
server, the connection, or something in the page. `krunk://tasks` and `krunk.log` (below) say
which host the tab is waiting for.

## Krunk's own hooks

Read for paths that never resolve (0.18.0); nothing was changed in them:

- **The request hook** (`app/src/ffi/cef/net.rs`, `on_before_resource_load`) decides at once
  on the IO thread (block, stand-in, or continue with headers); it never returns "continue
  later", so it cannot leave a request pending.
- **HTTP sign-in** (`app/src/browser/http_auth.rs`): a challenge in a background tab waits
  until the tab is shown (by design, the tab strip shows the key). Every way out answers the
  engine: answer, cancel, tab closed or navigated, too many, duplicate, and dropping the
  handle cancels too (`AuthResponder`'s `Drop`). While it waits, that request holds one of
  its host's six connections and its URL's cache entry, as in Chrome.
- **Client hints off** (`app/src/browser/hints.rs`): workers start paused and are resumed in
  `hints_target_attached`. The early returns there are "the tab is gone" (its DevTools session
  goes with it, which resumes the worker) and "the event has no session id" (nothing to
  resume). Frames are never paused.
- **The first navigation of a new tab** waits for the user-agent override's acknowledgement
  (`settings_host.rs`, `apply_user_agent_to_then`). DevTools calls with a callback time out
  after 10 s (`DEVTOOLS_TIMEOUT_MS`, `app/src/ffi/cef/view.rs`) and the navigation then
  starts anyway, with a warning in the log ("the user-agent override was not applied"). So a
  new tab can start up to 10 s late if the engine does not answer, but not never.
- **Downloads in parts** (`segmented.rs`) use Krunk's own HTTP client (WinHTTP), not the
  engine's connections, with timeouts (8 s for the probe, 30 s per read). Open question, not
  verified: while the probe runs, and after Krunk gives the engine's own download up
  (`abandon_download`), the engine's request for that URL stays open until its next progress
  update cancels it. If no update comes, it would hold one connection to that host and the
  URL's cache entry. Worth a look on Windows with a download that stalls right after its
  headers (`/slow-body?s=600` served as an attachment).

## What Krunk does (0.18.0)

### Around the cache lock

Krunk keeps a list of the requests under way (`core/src/inflight.rs`; filled by the request
hook: start in `on_before_resource_load`, headers in `on_resource_response`, end in
`on_resource_load_complete`). When a GET request starts for an address that **another tab**
of the same kind (normal or private) has been waiting on for at least one second without
headers, Krunk adds `Cache-Control: no-cache` and `Pragma: no-cache` to it, exactly what a hard
reload sends. The cache then starts a new entry for it instead of queueing (rows 13 and 14:
1.0 instead of 21.0, 9.0 instead of 16.1).

- Within the first second the engine's sharing is left alone (two tabs loading the same thing
  at the same moment still fetch it once, row 4).
- Once the first request has its headers, nothing changes either (rows 6 and 7 are fine as
  they are).
- Cost: such a request is always fetched from the network, even if a fresh copy is cached,
  and the server sees `Cache-Control: no-cache`. The first request's answer is then not kept
  in the cache (its entry was replaced); the second one's is.
- It is a header, not `CefRequest::SetFlags`: when the flags are changed in that hook, CEF
  replaces all of the request's load flags and its credentials mode
  (`libcef/common/request_impl.cc`).
- `krunk.log` says so the first twenty times: `tab 12: a script request goes around the cache
  (another tab waits for an answer for the same address)`.

### Sixteen connections per host (engine patch, not built yet)

`scripts/cef-build/patches/chromium-sockets-per-host.patch` raises the limit from 6 to 16. It
**only mitigates**: sixteen hung requests block a host just as six do, and a page that holds
six streams open simply has ten connections left for everything else. Low-priority requests
(images and the like) keep their own limit of six per host in the network service's scheduler
(`services/network/resource_scheduler`), so ordinary page loads open about as many connections
as before; the extra ten are there for navigations, scripts, styles and fetches. Known
trade-off: a few servers limit connections per client address and may refuse the eleventh;
Firefox and Chrome both use 6. The limit through one proxy stays 128.

The patch takes effect with the next engine build (`scripts/cef-build/README.md`); until then
Krunk's engine has Chromium's six.

### Seeing it: krunk://tasks and krunk.log

- **krunk://tasks** has a **Pending** column: the number of requests the task's tabs wait
  for, and the age of the oldest once it is over five seconds (`6 (45 s)`), in red from 30 s.
  An open stream or a long download counts as pending for as long as it runs. A tab that shows
  six or more old ones to one host is the tab to close.
- **krunk.log** gets a line when requests have been pending for over 30 s:
  `tab 7: 6 fetch requests to api.example.com pending for over 30 s (oldest 31 s, no answer
  yet)`. It names the tab, the **host only** (never the address or its query), the kind of
  request and how many, and whether an answer has started ("no answer yet", "answer still
  arriving"). For a private tab it names no host. At most four lines every ten seconds, and
  the same tab and host once in ten minutes.
- The list is bounded (4096 requests) and forgets a tab's requests when the tab closes.

## To do on Windows (not done here)

1. Copy `scripts/vm-www/hangserve.py` to the VM and start it (`python hangserve.py`); use
   `127.0.0.1:8120` and `localhost:8120` as the two hosts.
2. Rows 1, 5: open `http://127.0.0.1:8120/hang?x=1` in one tab and again in a second tab a
   few seconds later; the server's log should show the second request at once (with the
   mitigation) instead of after 20 s, and `krunk.log` the "goes around the cache" line. Same
   with `/slow-headers?s=8&nostore=1`.
3. Rows 8, 9: open `http://127.0.0.1:8120/page?n=6&u=%2Fhang`, then `http://127.0.0.1:8120/ok`
   in a second tab. With the current engine it hangs until the first tab is closed; with the
   patched engine it loads, and `/page?n=16&u=%2Fhang` blocks it again.
4. `krunk://tasks` should show `6` and a growing age for the first tab; `krunk.log` the 30 s
   line with `127.0.0.1` and no path.
5. The open question under "Downloads in parts".
