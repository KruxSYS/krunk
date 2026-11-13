// Krunk 0.19.0: krunk://wasm (the WASM memory inspector) and its ⋮ menu row. The mock host
// answers `wasmCall` with the real probe (ui/inject/wasm-probe.js) over a WebAssembly.Memory
// made in the test page, so the page's scan, watch, freeze and hex paths run for real.
// Hooked into test-ui.cjs (ONLY=WasmPage,WasmMenu).
'use strict';
const fs = require('fs');
const path = require('path');

module.exports = function ({ openPage, check, lastSent, emit, shot }) {
  const PROBE = fs.readFileSync(path.join(__dirname, '..', 'inject', 'wasm-probe.js'), 'utf8').trim().replace(/;$/, '');

  function setup(arg) {
    const memory = new WebAssembly.Memory({ initial: 1 });
    const probe = (0, eval)('(' + arg.probe + ')')(globalThis);
    window.__mem = memory;
    window.__dv = () => new DataView(memory.buffer);
    window.__probe = probe;
    const dv = window.__dv();
    dv.setInt32(0x400, 100, true);
    dv.setInt32(0x800, 100, true);
    dv.setUint32(0x500, 0x600, true); // a pointer
    dv.setInt32(0x608, 42, true);
    const h = window.__host.handlers;
    window.__info = { open: true, title: 'A game', url: 'https://game.example/play', private: false, inspectable: true, hook: false, unreachable: 'Not reachable: other sites\' frames.' };
    h.wasmInfo = () => { if (window.__unbound) throw new Error('Open this page from the ⋮ menu of the tab to inspect'); return window.__info; };
    h.wasmHook = (m) => { window.__info.hook = m.on; return { hook: m.on }; };
    h.wasmCall = (m) => {
      if (m.req.op === 'list') {
        return probe.call([memory], { op: 'list' }).then((mems) => ({ contexts: [
          { ctx: 0, kind: 'page', url: 'https://game.example/play', memories: mems },
          { ctx: 1000001, kind: 'worker', url: 'https://game.example/worker.js', memories: [], error: 'The page did not answer in time' },
        ], unreachable: 'x' }));
      }
      return probe.call(null, m.req);
    };
  }

  async function testWasmPage(browser) {
    console.log('0.19.0: WASM memory inspector page');
    const { page, errors } = await openPage(browser, 'https://krunk.invalid/wasm.html', { width: 1000, height: 900 }, 1, setup, { probe: PROBE });
    await page.waitForSelector('.mem-row');
    check((await page.textContent('#tab-title')) === 'A game' && (await page.textContent('#tab-url')) === 'https://game.example/play', 'shows the tab it is bound to');
    check((await page.textContent('.mem-row .name')) === 'Memory 0' && (await page.textContent('.mem-row .size')).startsWith('64 KiB'), 'lists the memory with its size');
    check((await page.locator('.mem-row.selected').count()) === 1, 'the first memory is chosen');
    check((await page.textContent('.ctx-note')).includes('worker.js') && (await page.textContent('.ctx-note')).includes('did not answer'), 'a context that failed is named');
    check(await page.locator('#problem').isHidden(), 'no problem line for a web page');

    // First scan: exact 100 (i32) finds both.
    await page.fill('#scan-value', '100');
    await page.click('#scan-first');
    await page.waitForSelector('#results tr[data-addr]');
    check((await page.textContent('#scan-status')).startsWith('2 addresses found'), 'first scan: 2 addresses (' + (await page.textContent('#scan-status')) + ')');
    check((await page.locator('#results tr[data-addr]').count()) === 2 && (await page.textContent('#results tr .addr')) === '0x400', 'the results list addresses in hex');
    check(await page.locator('#scan-first').isDisabled() && await page.locator('#scan-next').isEnabled(), 'after a first scan: Next scan and New scan');

    // The game takes a hit; next scan: decreased.
    await page.evaluate(() => window.__dv().setInt32(0x400, 90, true));
    await page.selectOption('#scan-kind', 'decreased');
    check(await page.locator('#scan-value').isHidden(), '"Decreased" needs no value');
    await page.click('#scan-next');
    await page.waitForFunction(() => document.querySelectorAll('#results tr[data-addr]').length === 1);
    check((await page.textContent('#scan-status')).startsWith('1 address found'), 'next scan narrows to 1 address');
    check((await page.textContent('#results tr .value')) === '90' && (await page.locator('#results td.changed').count()) === 0, 'the row shows the value now and before');

    // Watch it, write 999, freeze it.
    await page.click('#results .to-watch');
    await page.waitForFunction(() => { const i = document.querySelector('#watch input.value'); return i && i.value === '90'; });
    check((await page.textContent('#watch .where')) === '0x400', 'the result is on the watch list');
    await page.fill('#watch input.label', 'health');
    await page.fill('#watch input.value', '999');
    await page.press('#watch input.value', 'Enter');
    await page.waitForFunction(() => window.__dv().getInt32(0x400, true) === 999);
    check(true, 'typing a value and Enter writes it to the memory');
    await page.check('#watch input.freeze');
    await page.waitForFunction(() => window.__probe.call(null, { op: 'frozen' }).then((f) => f.length === 1));
    await page.evaluate(() => window.__dv().setInt32(0x400, 5, true));
    await page.waitForFunction(() => window.__dv().getInt32(0x400, true) === 999);
    check(true, 'a frozen value is written back');

    // Export, remove, import.
    await page.click('#watch-export');
    const exported = await page.inputValue('#io-text');
    const doc = JSON.parse(exported);
    check(doc.krunkWasmWatch === 1 && doc.entries.length === 1 && doc.entries[0].label === 'health' && doc.entries[0].frozen === true && doc.entries[0].value === 999
      && doc.entries[0].where === 0x400 && doc.entries[0].type === 'i32', 'export: the list as JSON (label, address, type, frozen value)');
    check(await page.locator('#io-ok').isHidden() && await page.locator('#io-copy').isVisible(), 'the export dialog offers Copy and Save, not Import');
    await shot(page, 'wasm-export.png');
    await page.click('#io-cancel');
    await page.click('#watch .remove');
    await page.waitForFunction(() => window.__probe.call(null, { op: 'frozen' }).then((f) => f.length === 0));
    check((await page.locator('#watch tr[data-i]').count()) === 0, 'Remove ends the freeze and drops the row');
    await page.click('#watch-import');
    await page.fill('#io-text', '{"entries": 1}');
    await page.click('#io-ok');
    check((await page.textContent('#io-error')).includes('not a watch list'), 'import refuses other JSON');
    await page.fill('#io-text', exported);
    await page.click('#io-ok');
    await page.waitForFunction(() => window.__probe.call(null, { op: 'frozen' }).then((f) => f.length === 1 && f[0].value === 999));
    await page.waitForFunction(() => document.querySelector('#watch input.freeze').checked);
    check((await page.inputValue('#watch input.label')) === 'health' && await page.locator('#watch input.freeze').isChecked(), 'import restores the entry, frozen at its value');

    // A pointer path.
    await page.fill('#add-where', '0x500 > 8');
    await page.click('#add-watch');
    await page.waitForFunction(() => { const i = document.querySelectorAll('#watch input.value')[1]; return i && i.value === '42'; });
    check((await page.locator('#watch .where').nth(1).textContent()) === '0x500 > 0x8 = 0x608', 'a pointer path is followed ([0x500] + 8) and shows where it ends');
    await page.fill('#add-where', 'zz');
    await page.click('#add-watch');
    check((await page.locator('#watch tr[data-i]').count()) === 2, 'a malformed address adds nothing');

    // The hex view: go to an address, change a byte in place.
    await page.fill('#hex-goto', '0x800');
    await page.click('#hex-go');
    await page.waitForFunction(() => { const o = document.querySelector('#hex .hx-row .o'); return o && o.textContent === '00000800'; });
    check((await page.textContent('#hex .b.here')) === '64', 'go to 0x800: the window starts there, the byte is selected (0x64 = 100)');
    check((await page.textContent('#hex-status')).includes('i32 100'), 'the status line reads the value at the caret');
    await page.keyboard.type('7F');
    await page.waitForFunction(() => new Uint8Array(window.__mem.buffer)[0x800] === 0x7f);
    check((await page.textContent('#hex .b[data-a="2048"]')) === '7F', 'typing two hex digits writes the byte');
    check((await page.getAttribute('#hex .b.here', 'data-a')) === '2049', 'the caret moves on');
    await page.click('#hex-next');
    await page.waitForFunction(() => document.querySelector('#hex .hx-row .o').textContent === '00000900');
    check(true, 'the next page of the window');
    await shot(page, 'wasm.png');

    // New scan; unknown initial value.
    await page.click('#scan-new');
    check(await page.locator('#results-card').isHidden() && await page.locator('#scan-first').isEnabled(), 'New scan clears the results');
    await page.selectOption('#scan-kind', 'unknown');
    await page.click('#scan-first');
    await page.waitForFunction(() => document.getElementById('scan-status').textContent.startsWith('Snapshot'));
    check(true, 'an unknown-value scan takes a snapshot');

    // The fallback.
    await page.click('#hook');
    await page.waitForFunction(() => window.__host.sent.some((m) => m.cmd === 'wasmHook'));
    check((await lastSent(page, 'wasmHook')).on === true, 'the hook button asks the host for the document-start hook');

    // Narrow.
    await page.setViewportSize({ width: 420, height: 800 });
    await page.waitForTimeout(100);
    check(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'at 420 px nothing sticks out of the page');
    await shot(page, 'wasm-narrow.png');

    // Not bound, closed tab, own page.
    await page.evaluate(() => { window.__unbound = true; });
    await page.setViewportSize({ width: 1000, height: 900 });
    await page.click('#reload');
    await page.waitForFunction(() => !document.getElementById('problem').hidden);
    check((await page.textContent('#problem')).includes('⋮ menu') && await page.locator('#scan-first').isDisabled(), 'opened by address: says how to open it, controls off');
    await page.evaluate(() => { window.__unbound = false; window.__info.inspectable = false; });
    await page.click('#reload');
    await page.waitForFunction(() => !document.getElementById('problem').textContent.includes('⋮'));
    check((await page.textContent('#problem')).includes('Only web pages'), 'a tab on one of Krunk\'s own pages: says so');
    check(errors.length === 0, 'no errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  async function testWasmMenu(browser) {
    console.log('0.19.0: the ⋮ menu row');
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 280, height: 730 }, 1);
    await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1, canReopen: true, hasVideo: false }, seq: 3 });
    await page.waitForSelector('.item');
    const labels = await page.locator('.item .label').allTextContents();
    const at = labels.indexOf('WASM memory inspector');
    check(at > 0 && labels[at - 1] === 'Developer tools', 'the menu has "WASM memory inspector" under Developer tools');
    await page.locator('.item', { hasText: 'WASM memory inspector' }).click();
    check(!!(await lastSent(page, 'openWasm')), 'it asks the host to open the inspector for the active tab');
    check(errors.length === 0, 'no errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  return { testWasmPage, testWasmMenu };
};
