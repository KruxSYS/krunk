# 0.26.0 hang after start: diagnosis (VM, 2026-11-21, one session)

**Cause: F-block's blocked-host push built an 85 825-entry CEF list in O(n²) on the UI thread.**

- **Reproduced:** release build app260rel, fresh copy of lp-base, real network.
  - `IsHungAppWindow` = true 33 s after start; the address typed at 20 s never opened.
  - krunk.log: "engine initialized" at t+0 s, "preconnect: 85825 blocked host(s) handed to the
    engine" at **t+39 s**. Then the overdue filter-list update starts a second push.
- **cdb (`!runaway`):** the UI thread (thread 0) used **38.5 s** of CPU; every other thread
  < 0.3 s. Sampled again later it was back in the message loop: the work had finished (one long
  burst per push).
- **Bisect** (harness build app260r, `KRUNK_PERF_OFF`, typing an address at 20 s):

  | run | switched off | hung |
  |---|---|---|
  | release | – | yes |
  | a | preload, omnibox-preconnect, preconnect-block | no |
  | b | preconnect-block | **no** |
  | d | preload, omnibox-preconnect | **yes** |
  | c | F-filter / F-writes / F-cosmetic | not run: these fixes and their switches were reverted |

- **Code:** `ffi::cef::set_blocked_hosts` called `list.set_size(n)` and then `set_string(i)` for
  each host.
  - In CEF, `CefListValueImpl::SetInternal` on an existing slot does `RemoveInternal(i)` (a vector
    erase) and then `Insert(i)`: O(n) per host, ~3.7 × 10⁹ element moves per push.
  - The load passes never showed it: the replay keeps lists from updating, and the first push
    fell inside the harness's start-up wait.
- **Fix:** append the hosts in order, no `set_size`: O(1) per host. The push's time is logged
  ("… handed to the engine in N ms").
- **Not done:** symbolized libcef stack (no PDBs on the VM); the CPU split, the log timeline and
  the bisect pin it.
