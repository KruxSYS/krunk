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
