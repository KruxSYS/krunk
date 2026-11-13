// Krunk 0.19.0: krunk://wasm (the WASM memory inspector) and its ⋮ menu row. The mock host
// answers `wasmCall` with the real probe (ui/inject/wasm-probe.js) over a WebAssembly.Memory
// made in the test page, so the page's scan, watch, freeze and hex paths run for real; and over
// an instance of the fixture module (wasm-fixture.cjs) for the Instances list and its globals.
// Hooked into test-ui.cjs (ONLY=WasmPage,WasmMenu).
'use strict';
const fs = require('fs');
const path = require('path');

module.exports = function ({ openPage, check, lastSent, emit, shot, UI }) {
  const PROBE = fs.readFileSync(path.join(__dirname, '..', 'inject', 'wasm-probe.js'), 'utf8').trim().replace(/;$/, '');
  const FIXTURE = require('./wasm-fixture.cjs').fixtureBytes();

  function setup(arg) {
    const memory = new WebAssembly.Memory({ initial: 1 });
    const probe = (0, eval)('(' + arg.probe + ')')(globalThis);
    // (Compiling needs 'wasm-unsafe-eval': the first load, under the page's own policy, has no instance.)
    let instance = null;
    try { instance = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(arg.fixture))); } catch (_) { /* the page's policy */ }
    window.__inst = instance;
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
        // As the host does: the memories and the instances a query found, then the probe's survey.
        return probe.call(instance ? [memory, instance.exports.memory] : [memory], { op: 'survey' }, instance ? [instance] : null).then((sv) => ({ contexts: [
          { ctx: 0, kind: 'page', url: 'https://game.example/play', memories: sv.memories, instances: sv.instances },
          { ctx: 1000001, kind: 'worker', url: 'https://game.example/worker.js', memories: [], instances: [], error: 'The page did not answer in time' },
        ], unreachable: 'x' }));
      }
      return probe.call(null, m.req);
    };
  }

  async function testWasmPage(browser) {
    console.log('0.19.0: WASM memory inspector page');
    const { page, errors } = await openPage(browser, 'https://krunk.invalid/wasm.html', { width: 1000, height: 900 }, 1, setup, { probe: PROBE, fixture: FIXTURE });
    await page.waitForSelector('.mem-row');
    // The mock's instance lives in the inspector page itself (in Krunk it lives in the inspected
    // tab), and Krunk's pages may not compile WebAssembly: this one document is served again
    // with 'wasm-unsafe-eval' added to the policy it came with.
    check(await page.evaluate(() => window.__inst === null), 'krunk://wasm itself can\'t compile WebAssembly (its content security policy)');
    const policy = (await page.reload()).headers()['content-security-policy'];
    await page.context().route('https://krunk.invalid/wasm.html', (r) => r.fulfill({ status: 200,
      headers: { 'content-type': 'text/html', 'content-security-policy': policy.replace("script-src 'self'", "script-src 'self' 'wasm-unsafe-eval'") },
      body: fs.readFileSync(path.join(UI, 'pages', 'wasm.html')) }));
    await page.reload();
    await page.waitForSelector('.inst-row');
    check((await page.textContent('#tab-title')) === 'A game' && (await page.textContent('#tab-url')) === 'https://game.example/play', 'shows the tab it is bound to');
    check((await page.textContent('.mem-row .name')) === 'Memory 0' && (await page.textContent('.mem-row .size')) === '64 KiB · 1 page', 'lists the memory with its size');
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

    // The list refreshes once a second: the row stays the same element (a click on it is not lost).
    await page.evaluate(() => { window.__row = document.querySelector('#results tr[data-addr]'); window.__dv().setInt32(0x400, 91, true); });
    await page.waitForFunction(() => document.querySelector('#results tr .value').textContent === '91');
    check(await page.evaluate(() => window.__row === document.querySelector('#results tr[data-addr]')) && (await page.locator('#results td.changed').count()) === 1, 'a refresh changes the values in place (the row is not rebuilt), a changed value is marked');
    await page.evaluate(() => window.__dv().setInt32(0x400, 90, true));
    await page.waitForFunction(() => document.querySelector('#results tr .value').textContent === '90');

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
    await page.waitForFunction(() => { const h = document.querySelector('#hex .b.here'); return h && h.getAttribute('data-a') === '2049'; });
    check(true, 'the caret moves on');
    await page.click('#hex-next');
    await page.waitForFunction(() => document.querySelector('#hex .hx-row .o').textContent === '00000900');
    check(true, 'the next page of the window');
    await shot(page, 'wasm.png');

    // memory.grow: the memory keeps its number, the scan's results, the watch list and the freeze.
    await page.fill('#add-where', '0x10010');
    await page.click('#add-watch');
    await page.waitForFunction(() => { const i = document.querySelectorAll('#watch input.value')[2]; return i && /past the end/.test(i.placeholder); });
    check(true, 'grow: before it, an address past the end of the memory says so');
    await page.evaluate(() => { window.__mem.grow(1); window.__dv().setInt32(0x10010, 4242, true); });
    await page.waitForFunction(() => { const i = document.querySelectorAll('#watch input.value')[2]; return i && i.value === '4242'; });
    check(true, 'grow: the watch entry past the old size now reads the value there');
    await page.waitForFunction(() => document.querySelector('.mem-row .size').textContent === '128 KiB · 2 pages' && document.getElementById('hex-memory').textContent === 'Memory 0, 128 KiB · 2 pages');
    check((await page.locator('.mem-row.selected .name').textContent()) === 'Memory 0', 'grow: the memory list and the hex view show the new size (128 KiB), the same memory stays chosen');
    check((await page.textContent('#results tr .addr')) === '0x400' && (await page.textContent('#results tr .value')) === '999', 'grow: the result list from before is still there, with the value of now');
    await page.evaluate(() => window.__dv().setInt32(0x400, 5, true));
    await page.waitForFunction(() => window.__dv().getInt32(0x400, true) === 999);
    check(true, 'grow: the frozen value is still written back, in the new buffer');
    await page.selectOption('#scan-kind', 'increased');
    await page.click('#scan-next');
    await page.waitForFunction(() => document.getElementById('scan-status').textContent.startsWith('1 address found'));
    check((await page.locator('#results tr[data-addr]').count()) === 1, 'grow: a next scan on the old results works (increased: 90 before, 999 now)');
    const far = page.locator('#watch input.value').nth(2);
    await far.fill('7');
    await far.press('Enter');
    await page.waitForFunction(() => window.__dv().getInt32(0x10010, true) === 7);
    check(true, 'grow: writing past the old size');
    await page.fill('#hex-goto', '0x1FFF0');
    await page.click('#hex-go');
    await page.waitForFunction(() => { const o = document.querySelector('#hex .hx-row .o'); return o && o.textContent === '0001FFF0'; });
    check((await page.locator('#hex .hx-row').count()) === 1, 'grow: the hex view reaches the last row of the grown memory');
    await page.locator('#watch .remove').nth(2).click();

    // The header follows the tab (a game writes its state into its title).
    await page.evaluate(() => { window.__info.title = 'A game, health 90'; });
    await page.waitForFunction(() => document.getElementById('tab-title').textContent === 'A game, health 90' && document.title === 'WASM – A game, health 90');
    check(await page.locator('#scan-first').isDisabled() && await page.locator('#scan-next').isEnabled(), 'the header follows the tab\'s title; the controls stay as they were');

    // Instances: exports, which memory is whose, globals.
    const rows = page.locator('#instances .inst-row');
    check((await rows.count()) === 1 && (await rows.locator('.name').textContent()) === 'Instance 0', 'instances: the list has the instance');
    check((await rows.locator('.size').textContent()) === '1 function · 1 memory · 5 globals · 1 table · Memory 1', 'instances: its exports by kind, and its memory (' + (await rows.locator('.size').textContent()) + ')');
    const owners = await page.locator('.mem-row .owner').allTextContents();
    check(owners.length === 2 && owners[0] === '' && owners[1] === 'export "memory" of instance 0', 'instances: memory 1 is instance 0\'s export "memory"; memory 0 belongs to no instance and stays listed');
    await rows.click();
    await page.waitForFunction(() => document.getElementById('scan-memory').textContent === 'Memory 1');
    check((await page.locator('#memories .mem-row.selected .name').textContent()) === 'Memory 1' && (await page.textContent('#hex-memory')).startsWith('Memory 1, 64 KiB'), 'instances: choosing the instance chooses its memory');
    check((await page.textContent('#instance-memories .mem-link')) === 'export "memory" = Memory 1', 'instances: the instance names its memory');
    const grow = (n) => page.locator('#globals tr[data-export="' + n + '"]');
    check((await page.locator('#globals tr[data-export]').count()) === 5 && (await grow(1).locator('.gname').textContent()) === 'g_i32' && (await grow(1).locator('.gtype').textContent()) === 'i32'
      && (await grow(1).locator('input.gvalue').inputValue()) === '7', 'globals: the table lists them with type and value (g_i32: i32, 7)');
    check((await grow(2).locator('input.gvalue').inputValue()) === '-5' && (await grow(3).locator('.gtype').textContent()) === 'f32' && (await grow(4).locator('input.gvalue').inputValue()) === '2.25', 'globals: i64, f32 and f64');
    check((await grow(5).locator('.gtype').textContent()) === 'i32 (constant)' && (await grow(5).locator('.gconst').textContent()) === '1024' && (await grow(5).locator('input').count()) === 0, 'globals: an immutable one is shown, not editable');
    await grow(1).locator('input.gvalue').fill('42');
    await grow(1).locator('input.gvalue').press('Enter');
    await page.waitForFunction(() => window.__inst.exports.g_i32.value === 42);
    check((await lastSent(page, 'wasmCall')).req.op !== undefined && (await page.evaluate(() => window.__host.sent.filter((m) => m.cmd === 'wasmCall' && m.req.op === 'setGlobal').map((m) => JSON.stringify(m.req))[0])) === '{"op":"setGlobal","inst":0,"export":1,"value":"42"}',
      'globals: Enter writes a mutable global (by the instance\'s number and the export\'s place)');
    await grow(2).locator('input.gvalue').fill('9007199254740993');
    await grow(2).locator('input.gvalue').press('Enter');
    await page.waitForFunction(() => window.__inst.exports.g_i64.value === 9007199254740993n);
    check(true, 'globals: a 64-bit value keeps every digit');
    await page.evaluate(() => { window.__inst.exports.g_f64.value = 9.5; });
    await page.waitForFunction(() => document.querySelector('#globals tr[data-export="4"] input.gvalue').value === '9.5');
    check(true, 'globals: a value the page changes shows up');
    await grow(1).locator('input.gvalue').fill('1.5');
    await grow(1).locator('input.gvalue').press('Enter');
    await page.waitForFunction(() => /whole number/.test(document.body.textContent));
    check(await page.evaluate(() => window.__inst.exports.g_i32.value === 42), 'globals: a value that does not fit the type is refused with the reason');
    await grow(1).locator('input.gvalue').press('Escape');
    check((await page.textContent('#instance-others')) === '1 function: f. 1 table: tbl (1).', 'instances: the other exports by name (' + (await page.textContent('#instance-others')) + ')');
    await shot(page, 'wasm-instances.png');
    await page.locator('#memories .mem-row').first().click();
    await page.waitForFunction(() => document.getElementById('scan-memory').textContent === 'Memory 0');

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
    check((await page.textContent('#memories')) === 'Nothing to inspect.' && (await page.textContent('#instances')) === 'Nothing to inspect.', 'and the lists do not claim to be looking for anything');
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
