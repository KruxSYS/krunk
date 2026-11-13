// krunk://wasm: the WASM memory inspector for the tab this page was opened from (⋮ menu →
// WASM memory inspector). The host bound the page to that tab; requests name no tab. Every
// request goes to the probe next to the memories (ui/inject/wasm-probe.js) as
// {op, …}: list, scan, next, results, read, write, freeze, watch, bytes, writeBytes.
(function () {
  'use strict';
  const el = LB.el;
  const $ = function (id) { return document.getElementById(id); };
  const NUM_TYPES = ['i8', 'u8', 'i16', 'u16', 'i32', 'u32', 'i64', 'u64', 'f32', 'f64'];
  const WATCH_TYPES = NUM_TYPES.concat(['utf8']);
  const FIRST = [['exact', 'Exact value'], ['range', 'Value between'], ['bigger', 'Bigger than'], ['smaller', 'Smaller than'], ['unknown', 'Unknown initial value']];
  const NEXT = [['exact', 'Exact value'], ['range', 'Value between'], ['changed', 'Changed'], ['unchanged', 'Unchanged'], ['increased', 'Increased'],
    ['decreased', 'Decreased'], ['increasedBy', 'Increased by'], ['decreasedBy', 'Decreased by']];
  const NEEDS_VALUE = { exact: 1, range: 1, bigger: 1, smaller: 1, increasedBy: 1, decreasedBy: 1 };
  const PAGE = 100, MAX_SHOWN = 1000, HEX_BYTES = 256, MAX_WATCH = 256, TEXT_LENGTH = 32;
  const KIND = { page: 'Page', frame: 'Frame', worker: 'Worker' };

  const state = {
    ok: false, // bound to an open web page
    contexts: [],
    sel: null, // {ctx, mem, bytes}
    scan: null, // {ctx, mem, id, type, count, snapshot}
    scanning: false,
    shown: PAGE,
    watch: [], // {label, ctx, mem, where, type, freeze, value, error}
    hex: { addr: 0, bytes: [], size: 0, caret: -1, lo: false, before: null },
  };

  // ---------- small things ----------
  function hex(n) { return '0x' + Number(n).toString(16).toUpperCase(); }
  function hex2(b) { return (b < 16 ? '0' : '') + b.toString(16).toUpperCase(); }
  function fail(e) { Page.toast(e && e.message ? e.message : String(e)); }
  function call(ctx, req) { return LB.request('wasmCall', { ctx: ctx, req: req }); }
  function sizeText(bytes) {
    if (bytes >= 1048576) return (bytes / 1048576).toFixed(bytes % 1048576 ? 1 : 0) + ' MiB';
    return Math.round(bytes / 1024) + ' KiB';
  }
  function parseInt2(text) {
    const t = String(text).trim();
    const m = /^(-?)(0x[0-9a-f]+|\d+)$/i.exec(t);
    if (!m) return null;
    const n = Number(m[2]);
    return m[1] ? -n : n;
  }
  function parseAddr(text) {
    const n = parseInt2(text);
    return n === null || n < 0 || n > 0xffffffff ? null : n;
  }
  // "0x400" or "0x400 > 8 > 0": an address, or a base and the offsets of a pointer path.
  function parseWhere(text) {
    const parts = String(text).split(/\s*-?>\s*/);
    const base = parseAddr(parts[0]);
    if (base === null) return null;
    if (parts.length === 1) return base;
    const offsets = parts.slice(1).map(parseInt2);
    if (offsets.length > 8 || offsets.some(function (o) { return o === null; })) return null;
    return { base: base, offsets: offsets };
  }
  function whereText(w) {
    if (typeof w === 'number') return hex(w);
    return [hex(w.base)].concat(w.offsets.map(function (o) { return o < 0 ? '-' + hex(-o) : hex(o); })).join(' > ');
  }
  // What the user typed, as the probe takes it.
  function parseValue(type, text) {
    const t = String(text).trim();
    if (type === 'utf8') return { value: String(text) };
    if (t === '') return { error: 'Type a value' };
    if (type === 'i64' || type === 'u64') return /^-?\d+$/.test(t) ? { value: t } : { error: 'A whole number' };
    if (type === 'f32' || type === 'f64') { const f = Number(t); return isFinite(f) ? { value: f } : { error: 'A number' }; }
    const n = parseInt2(t);
    return n === null ? { error: 'A whole number (decimal or 0x…)' } : { value: n };
  }
  function show(v) { return v === null || v === undefined ? '–' : String(v); }
  function options(select, list, keep) {
    const was = keep ? select.value : null;
    select.textContent = '';
    list.forEach(function (o) {
      const pair = Array.isArray(o) ? o : [o, o];
      select.appendChild(el('option', { value: pair[0], text: pair[1] }));
    });
    if (was && list.some(function (o) { return (Array.isArray(o) ? o[0] : o) === was; })) select.value = was;
  }
  function memName(s) {
    const c = state.contexts.find(function (x) { return x.ctx === s.ctx; });
    return 'Memory ' + s.mem + (c && c.kind !== 'page' ? ' (' + KIND[c.kind].toLowerCase() + ')' : '');
  }

  // ---------- the tab and its memories ----------
  function problem(text) {
    $('problem').textContent = text || '';
    $('problem').hidden = !text;
    state.ok = !text;
    ['hook', 'scan-first', 'add-watch', 'hex-go', 'hex-prev', 'hex-next', 'watch-import'].forEach(function (id) { $(id).disabled = !state.ok; });
    if (!state.ok) { $('scan-next').disabled = true; $('scan-new').disabled = true; }
  }
  function loadInfo() {
    return LB.request('wasmInfo').then(function (r) {
      $('unreachable').textContent = r.unreachable || '';
      $('tab-title').textContent = r.open ? (r.title || r.url) : '';
      $('tab-url').textContent = r.open ? r.url : '';
      $('private').hidden = !r.private;
      $('hook').hidden = !!r.hook;
      $('hook-on').hidden = !r.hook;
      if (r.open) document.title = 'WASM – ' + (r.title || r.url);
      if (!r.open) problem('The tab this page was opened from has closed.');
      else if (!r.inspectable) problem('Only web pages can be inspected: the tab shows one of Krunk\'s own pages or an empty page.');
      else problem('');
      return r;
    }, function (e) { problem(e.message); return null; });
  }
  function find() {
    if (!state.ok) return Promise.resolve();
    return call(0, { op: 'list' }).then(function (r) {
      state.contexts = r.contexts || [];
      const all = [];
      state.contexts.forEach(function (c) { (c.memories || []).forEach(function (m) { all.push({ ctx: c.ctx, mem: m.mem, bytes: m.bytes }); }); });
      const cur = state.sel && all.find(function (m) { return m.ctx === state.sel.ctx && m.mem === state.sel.mem; });
      state.sel = cur || all[0] || null;
      renderMemories();
      selected();
    }).catch(function (e) {
      $('memories').textContent = '';
      $('memories').appendChild(el('div', { class: 'empty', text: e.message }));
    });
  }
  function renderMemories() {
    const box = $('memories');
    box.textContent = '';
    let any = false;
    state.contexts.forEach(function (c) {
      (c.memories || []).forEach(function (m) {
        any = true;
        const on = state.sel && state.sel.ctx === c.ctx && state.sel.mem === m.mem;
        box.appendChild(el('div', {
          class: 'mem-row' + (on ? ' selected' : ''), role: 'button', tabindex: '0', 'data-ctx': String(c.ctx), 'data-mem': String(m.mem),
          'aria-pressed': on ? 'true' : 'false',
          onclick: function () { state.sel = { ctx: c.ctx, mem: m.mem, bytes: m.bytes }; renderMemories(); selected(); },
          onkeydown: function (e) { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); e.currentTarget.click(); } },
        }, [
          el('span', { class: 'name', text: 'Memory ' + m.mem }),
          el('span', { class: 'where', text: KIND[c.kind] + (c.kind === 'page' ? '' : ' ' + c.url), title: c.url }),
          el('span', { class: 'size', text: sizeText(m.bytes) + (m.shared ? ' · shared' : '') }),
        ]));
      });
      if (c.error) box.appendChild(el('div', { class: 'ctx-note', text: KIND[c.kind] + ' ' + c.url + ': ' + c.error }));
    });
    if (!any) box.insertBefore(el('div', { class: 'empty', text: 'No WebAssembly memory found in this tab (yet). Start the game or app, then choose Find memories.' }), box.firstChild);
  }
  function selected() {
    const s = state.sel;
    $('scan-memory').textContent = s ? memName(s) : '';
    $('hex-memory').textContent = s ? memName(s) + ', ' + sizeText(s.bytes) : '';
    state.hex.before = null;
    if (s) hexLoad(Math.min(state.hex.addr, Math.max(0, s.bytes - 16)));
    else { state.hex.bytes = []; hexRender(); }
  }

  // ---------- scans ----------
  function scanControls() {
    const sc = state.scan;
    options($('scan-kind'), sc ? NEXT : FIRST, true);
    $('scan-type').disabled = !!sc;
    $('scan-first').disabled = !state.ok || !!sc || state.scanning;
    $('scan-next').disabled = !sc || state.scanning;
    $('scan-new').disabled = !sc || state.scanning;
    kindChanged();
  }
  function kindChanged() {
    const k = $('scan-kind').value;
    $('scan-value').hidden = !NEEDS_VALUE[k];
    $('scan-to').hidden = k !== 'range';
  }
  function scanValues(req, type) {
    const k = req.kind;
    if (!NEEDS_VALUE[k]) return true;
    const v = parseValue(type, $('scan-value').value);
    if (v.error) { $('scan-status').textContent = v.error + '.'; return false; }
    req.value = v.value;
    if (k === 'range') {
      const to = parseValue(type, $('scan-to').value);
      if (to.error) { $('scan-status').textContent = 'The upper end: ' + to.error.toLowerCase() + '.'; return false; }
      req.to = to.value;
    }
    return true;
  }
  function scanStatus(r) {
    let t = r.snapshot ? 'Snapshot of ' + r.count.toLocaleString() + ' values taken. Change the value in the page, then scan for Changed, Increased or Decreased.'
      : r.count.toLocaleString() + (r.count === 1 ? ' address' : ' addresses') + ' found.';
    if (r.truncated) t += ' More matched than can be kept: narrow it down with another scan.';
    $('scan-status').textContent = t;
  }
  function runScan(ctx, req, type, mem) {
    state.scanning = true;
    scanControls();
    $('scan-status').textContent = 'Scanning…';
    return call(ctx, req).then(function (r) {
      state.scan = { ctx: ctx, mem: mem, id: r.scan, type: type, count: r.count, snapshot: !!r.snapshot };
      state.shown = PAGE;
      scanStatus(r);
    }, function (e) {
      $('scan-status').textContent = e.message;
      // A scan the page no longer has (it navigated): start over.
      if (/no scan|unknown scan/i.test(e.message)) state.scan = null;
    }).then(function () {
      state.scanning = false;
      scanControls();
      return loadResults();
    });
  }
  function firstScan() {
    const s = state.sel;
    if (!s) { $('scan-status').textContent = 'Choose a memory first.'; return; }
    const type = $('scan-type').value;
    const req = { op: 'scan', mem: s.mem, type: type, kind: $('scan-kind').value };
    if (scanValues(req, type)) runScan(s.ctx, req, type, s.mem);
  }
  function nextScan() {
    const sc = state.scan;
    if (!sc) return;
    const req = { op: 'next', scan: sc.id, kind: $('scan-kind').value };
    if (scanValues(req, sc.type)) runScan(sc.ctx, req, sc.type, sc.mem);
  }
  function newScan() {
    const sc = state.scan;
    state.scan = null;
    if (sc) call(sc.ctx, { op: 'discard', scan: sc.id }).catch(function () {});
    $('scan-status').textContent = 'Choose a memory, then scan for a value the page shows.';
    scanControls();
    loadResults();
  }
  function loadResults() {
    const sc = state.scan;
    const card = $('results-card');
    if (!sc || sc.snapshot) { card.hidden = true; return Promise.resolve(); }
    return call(sc.ctx, { op: 'results', scan: sc.id, offset: 0, limit: state.shown }).then(function (r) {
      if (state.scan !== sc) return;
      card.hidden = false;
      const body = $('results');
      body.textContent = '';
      if (!r.rows.length) body.appendChild(el('tr', {}, [el('td', { colspan: '4', class: 'none', text: 'No address left. Start a new scan.' })]));
      r.rows.forEach(function (row) {
        body.appendChild(el('tr', { 'data-addr': String(row.addr) }, [
          el('td', { class: 'mono addr', text: hex(row.addr) }),
          el('td', { class: 'mono value' + (String(row.value) !== String(row.previous) ? ' changed' : ''), text: show(row.value) }),
          el('td', { class: 'mono', text: show(row.previous) }),
          el('td', {}, [el('button', { class: 'link-btn to-watch', text: 'Watch', onclick: function () {
            addWatch({ label: '', ctx: sc.ctx, mem: sc.mem, where: row.addr, type: sc.type });
          } }), document.createTextNode(' '), el('button', { class: 'link-btn to-hex', text: 'Hex', onclick: function () { hexGoto(sc, row.addr); } })]),
        ]));
      });
      $('results-more').hidden = !(r.count > r.rows.length && state.shown < MAX_SHOWN);
    }).catch(function () { /* the next tick tries again */ });
  }

  // ---------- the watch list ----------
  function probeEntry(w) {
    const e = { mem: w.mem, where: w.where, type: w.type };
    if (w.type === 'utf8') e.length = TEXT_LENGTH;
    return e;
  }
  function addWatch(w) {
    if (state.watch.length >= MAX_WATCH) { Page.toast('The watch list holds at most ' + MAX_WATCH + ' entries.'); return null; }
    const entry = { label: w.label || '', ctx: w.ctx, mem: w.mem, where: w.where, type: w.type, freeze: null, value: undefined, error: '' };
    state.watch.push(entry);
    renderWatch();
    refreshWatch();
    return entry;
  }
  function setFrozen(w, on, value) {
    const off = w.freeze === null ? Promise.resolve() : call(w.ctx, { op: 'unfreeze', freeze: w.freeze }).catch(function () {});
    w.freeze = null;
    return off.then(function () {
      if (!on) return null;
      const req = { op: 'freeze', mem: w.mem, where: w.where, type: w.type };
      if (value !== undefined) req.value = value;
      return call(w.ctx, req).then(function (r) { w.freeze = r.freeze; w.value = r.value; w.frozenAt = r.value; });
    });
  }
  function writeWatch(w, text, input) {
    const v = parseValue(w.type, text);
    if (v.error) { Page.toast(v.error); return; }
    const done = w.freeze !== null ? setFrozen(w, true, v.value)
      : call(w.ctx, { op: 'write', mem: w.mem, where: w.where, type: w.type, value: v.value });
    done.then(function () { input.blur(); refreshWatch(); }).catch(fail);
  }
  function renderWatch() {
    const body = $('watch');
    body.textContent = '';
    if (!state.watch.length) {
      body.appendChild(el('tr', {}, [el('td', { colspan: '6', class: 'none', text: 'Nothing watched. Add an address below, or choose Watch on a scan result.' })]));
      return;
    }
    state.watch.forEach(function (w, i) {
      const value = el('input', { type: 'text', class: 'value', spellcheck: 'false', 'aria-label': 'Value', title: 'Type a value and press Enter to write it' });
      value.addEventListener('keydown', function (e) {
        if (e.key === 'Enter') { e.preventDefault(); writeWatch(w, value.value, value); }
        if (e.key === 'Escape') { value.blur(); }
      });
      const label = el('input', { type: 'text', class: 'label', placeholder: 'Label', 'aria-label': 'Label' });
      label.value = w.label;
      label.addEventListener('input', function () { w.label = label.value.slice(0, 80); });
      const freeze = el('input', { type: 'checkbox', class: 'freeze', 'aria-label': 'Freeze this value' });
      freeze.checked = w.freeze !== null;
      freeze.addEventListener('change', function () {
        setFrozen(w, freeze.checked).catch(function (e) { freeze.checked = false; fail(e); }).then(refreshWatch);
      });
      body.appendChild(el('tr', { 'data-i': String(i) }, [
        el('td', {}, [label]),
        el('td', { class: 'mono where', text: whereText(w.where), title: memName(w) }),
        el('td', { class: 'mono', text: w.type }),
        el('td', {}, [value]),
        el('td', {}, [freeze]),
        el('td', {}, [el('button', { class: 'link-btn danger remove', text: 'Remove', onclick: function () {
          setFrozen(w, false);
          state.watch.splice(state.watch.indexOf(w), 1);
          renderWatch();
          refreshWatch();
        } })]),
      ]));
    });
    paintWatch();
  }
  // The values, without touching a field the user is typing in.
  function paintWatch() {
    const rows = $('watch').querySelectorAll('tr[data-i]');
    state.watch.forEach(function (w, i) {
      const row = rows[i];
      if (!row) return;
      const input = row.querySelector('input.value');
      if (document.activeElement !== input) input.value = w.error ? '' : (w.value === undefined ? '' : String(w.value));
      input.placeholder = w.error || '';
      input.title = w.error || 'Type a value and press Enter to write it';
      const where = row.querySelector('.where');
      where.textContent = whereText(w.where) + (typeof w.where !== 'number' && w.addr !== undefined && !w.error ? ' = ' + hex(w.addr) : '');
      row.querySelector('input.freeze').checked = w.freeze !== null;
    });
  }
  function refreshWatch() {
    const byCtx = {};
    state.watch.forEach(function (w) { (byCtx[w.ctx] = byCtx[w.ctx] || []).push(w); });
    return Promise.all(Object.keys(byCtx).map(function (ctx) {
      const list = byCtx[ctx];
      return call(Number(ctx), { op: 'watch', entries: list.map(probeEntry) }).then(function (r) {
        list.forEach(function (w, i) {
          const x = r[i] || {};
          w.error = x.error || '';
          w.value = x.value;
          w.addr = x.addr;
        });
      }, function (e) { list.forEach(function (w) { w.error = e.message; }); });
    })).then(paintWatch);
  }
  function addFromForm() {
    const s = state.sel;
    if (!s) { Page.toast('Choose a memory first.'); return; }
    const where = parseWhere($('add-where').value);
    if (where === null) { Page.toast('An address like 0x1F40, or a pointer path like 0x400 > 8 > 0.'); return; }
    if (addWatch({ label: '', ctx: s.ctx, mem: s.mem, where: where, type: $('add-type').value })) $('add-where').value = '';
  }

  // Export and import: JSON through this page (a dialog; copy, or a file).
  function exportText() {
    return JSON.stringify({ krunkWasmWatch: 1, entries: state.watch.map(function (w) {
      const e = { label: w.label, ctx: w.ctx, mem: w.mem, where: w.where, type: w.type, frozen: w.freeze !== null };
      // (The value it is held at, not the one last read: the page may have just written.)
      if (w.freeze !== null && w.frozenAt !== undefined) e.value = w.frozenAt;
      return e;
    }) }, null, 2);
  }
  function importText(text) {
    let doc;
    try { doc = JSON.parse(text); } catch (e) { return 'This is not JSON.'; }
    const list = doc && doc.krunkWasmWatch === 1 && Array.isArray(doc.entries) ? doc.entries : null;
    if (!list) return 'This is not a watch list exported from this page.';
    const ok = [];
    for (let i = 0; i < list.length; i++) {
      const e = list[i] || {};
      const where = typeof e.where === 'number' ? parseAddr(String(e.where))
        : (e.where && typeof e.where.base === 'number' && Array.isArray(e.where.offsets) && e.where.offsets.length <= 8
          && e.where.offsets.every(function (o) { return Number.isInteger(o); }) ? { base: e.where.base, offsets: e.where.offsets } : null);
      if (where === null || WATCH_TYPES.indexOf(e.type) < 0 || !Number.isInteger(e.mem) || e.mem < 0) return 'Entry ' + (i + 1) + ' is not a watch entry.';
      ok.push({ label: String(e.label || '').slice(0, 80), ctx: Number.isInteger(e.ctx) && e.ctx >= 0 ? e.ctx : 0, mem: e.mem, where: where, type: e.type,
        frozen: e.frozen === true, value: e.value });
    }
    if (state.watch.length + ok.length > MAX_WATCH) return 'The watch list holds at most ' + MAX_WATCH + ' entries.';
    ok.forEach(function (e) {
      const w = addWatch(e);
      if (w && e.frozen) setFrozen(w, true, e.value === undefined ? undefined : e.value).catch(function () {}).then(refreshWatch);
    });
    return '';
  }
  function openIo(importing) {
    $('io-title').textContent = importing ? 'Import a watch list' : 'Export the watch list';
    $('io-text').value = importing ? '' : exportText();
    $('io-text').readOnly = !importing;
    $('io-error').textContent = '';
    $('io-ok').hidden = !importing;
    $('io-file-label').hidden = !importing;
    $('io-save').hidden = importing;
    $('io-copy').hidden = importing;
    $('io').showModal();
    if (!importing) $('io-text').select();
  }

  // ---------- the hex view: a window of the memory ----------
  function hexLoad(addr) {
    const s = state.sel;
    if (!s) return Promise.resolve();
    const h = state.hex;
    const at = Math.max(0, Math.floor(addr / 16) * 16);
    return call(s.ctx, { op: 'bytes', mem: s.mem, addr: at, length: HEX_BYTES }).then(function (r) {
      if (state.sel !== s) return;
      h.before = h.addr === at ? h.bytes : null;
      h.addr = at;
      h.bytes = r.bytes || [];
      h.size = r.size || 0;
      s.bytes = h.size || s.bytes;
      hexRender();
    }).catch(function (e) { h.bytes = []; hexRender(); $('hex-status').textContent = e.message; });
  }
  function hexRender() {
    const h = state.hex;
    const box = $('hex');
    box.textContent = '';
    for (let o = 0; o < h.bytes.length; o += 16) {
      const hx = el('span', { class: 'hx' });
      const tx = el('span', { class: 'tx' });
      for (let i = o; i < Math.min(o + 16, h.bytes.length); i++) {
        const b = h.bytes[i];
        const here = h.addr + i === h.caret;
        const changed = h.before && h.before[i] !== undefined && h.before[i] !== b;
        hx.appendChild(el('span', { class: 'b' + (here ? ' here' + (h.lo ? ' lo' : '') : '') + (changed ? ' changed' : ''), 'data-a': String(h.addr + i), text: hex2(b) }));
        tx.appendChild(el('span', { class: 'c' + (here ? ' here' : ''), 'data-a': String(h.addr + i), text: b >= 32 && b < 127 ? String.fromCharCode(b) : '.' }));
      }
      box.appendChild(el('div', { class: 'hx-row' }, [
        el('span', { class: 'o', text: (h.addr + o).toString(16).toUpperCase().padStart(8, '0') }), hx, tx,
      ]));
    }
    hexStatus();
  }
  function hexStatus() {
    const h = state.hex;
    if (!state.sel) { $('hex-status').textContent = 'No memory chosen.'; return; }
    const i = h.caret - h.addr;
    if (h.caret < 0 || i < 0 || i >= h.bytes.length) {
      $('hex-status').textContent = hex(h.addr) + ' – ' + hex(h.addr + Math.max(0, h.bytes.length - 1)) + ' of ' + sizeText(h.size) + '. Click a byte, then type hex digits to change it.';
      return;
    }
    let t = 'Address ' + hex(h.caret) + ' · u8 ' + h.bytes[i];
    if (i + 4 <= h.bytes.length) {
      const dv = new DataView(Uint8Array.from(h.bytes.slice(i, i + 4)).buffer);
      t += ' · i32 ' + dv.getInt32(0, true) + ' · f32 ' + Number(dv.getFloat32(0, true).toPrecision(7));
    }
    $('hex-status').textContent = t;
  }
  function hexMove(to) {
    const h = state.hex;
    if (to < 0 || to >= h.size) return;
    h.caret = to;
    h.lo = false;
    if (to < h.addr || to >= h.addr + h.bytes.length) hexLoad(to < h.addr ? to - (HEX_BYTES - 16) : to);
    else hexRender();
  }
  function hexType(digit) {
    const h = state.hex, s = state.sel;
    const i = h.caret - h.addr;
    if (!s || i < 0 || i >= h.bytes.length) return;
    const old = h.bytes[i];
    const b = h.lo ? (old & 0xf0) | digit : (digit << 4) | (old & 0x0f);
    const addr = h.caret, wasLo = h.lo;
    h.bytes[i] = b;
    h.lo = !wasLo;
    hexRender();
    call(s.ctx, { op: 'writeBytes', mem: s.mem, addr: addr, bytes: [b] }).then(function () {
      if (wasLo) hexMove(addr + 1);
    }, function (e) { h.bytes[i] = old; hexRender(); fail(e); });
  }
  function hexGoto(target, addr) {
    if (target && (!state.sel || state.sel.ctx !== target.ctx || state.sel.mem !== target.mem)) {
      state.sel = { ctx: target.ctx, mem: target.mem, bytes: 0 };
      renderMemories();
      $('scan-memory').textContent = memName(state.sel);
      $('hex-memory').textContent = memName(state.sel);
    }
    state.hex.caret = addr;
    state.hex.lo = false;
    state.hex.before = null;
    hexLoad(addr).then(function () { $('hex').focus(); });
  }
  function hexKey(e) {
    const h = state.hex;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const moves = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -16, ArrowDown: 16, PageUp: -HEX_BYTES, PageDown: HEX_BYTES };
    if (moves[e.key] !== undefined) {
      e.preventDefault();
      if (h.caret < 0) hexMove(h.addr);
      else hexMove(Math.max(0, Math.min(h.size - 1, h.caret + moves[e.key])));
      return;
    }
    if (/^[0-9a-fA-F]$/.test(e.key) && h.caret >= 0) { e.preventDefault(); hexType(parseInt(e.key, 16)); }
  }

  // ---------- start ----------
  function tick() {
    if (document.hidden || !state.ok) return;
    if (state.watch.length) refreshWatch();
    if (state.scan && !state.scanning) loadResults();
    if ($('hex-live').checked && state.sel && !state.hex.lo) hexLoad(state.hex.addr);
  }
  function start() {
    options($('scan-type'), NUM_TYPES);
    $('scan-type').value = 'i32';
    options($('add-type'), WATCH_TYPES);
    $('add-type').value = 'i32';
    scanControls();
    renderWatch();
    $('reload').addEventListener('click', function () { loadInfo().then(find); });
    $('hook').addEventListener('click', function () { hook(true); });
    $('hook-off').addEventListener('click', function () { hook(false); });
    $('scan-kind').addEventListener('change', kindChanged);
    $('scan-first').addEventListener('click', firstScan);
    $('scan-next').addEventListener('click', nextScan);
    $('scan-new').addEventListener('click', newScan);
    ['scan-value', 'scan-to'].forEach(function (id) {
      $(id).addEventListener('keydown', function (e) { if (e.key === 'Enter') { if (state.scan) nextScan(); else firstScan(); } });
    });
    $('results-more').addEventListener('click', function () { state.shown = Math.min(MAX_SHOWN, state.shown + PAGE * 3); loadResults(); });
    $('add-watch').addEventListener('click', addFromForm);
    $('add-where').addEventListener('keydown', function (e) { if (e.key === 'Enter') addFromForm(); });
    $('watch-export').addEventListener('click', function () { openIo(false); });
    $('watch-import').addEventListener('click', function () { openIo(true); });
    $('io-cancel').addEventListener('click', function () { $('io').close(); });
    $('io-ok').addEventListener('click', function () {
      const problemText = importText($('io-text').value);
      $('io-error').textContent = problemText;
      if (!problemText) $('io').close();
    });
    $('io-copy').addEventListener('click', function () {
      $('io-text').select();
      (navigator.clipboard ? navigator.clipboard.writeText($('io-text').value) : Promise.reject()).catch(function () { document.execCommand('copy'); });
      Page.toast('Copied');
    });
    $('io-save').addEventListener('click', function () {
      const a = el('a', { href: URL.createObjectURL(new Blob([$('io-text').value], { type: 'application/json' })), download: 'wasm-watch-list.json' });
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(function () { URL.revokeObjectURL(a.href); }, 10000);
    });
    $('io-file').addEventListener('change', function () {
      const f = $('io-file').files[0];
      if (!f) return;
      if (f.size > 1048576) { $('io-error').textContent = 'That file is too large for a watch list.'; return; }
      f.text().then(function (t) { $('io-text').value = t; });
      $('io-file').value = '';
    });
    $('hex-go').addEventListener('click', function () {
      const a = parseAddr($('hex-goto').value);
      if (a === null) { Page.toast('An address like 0x1F40.'); return; }
      hexGoto(null, a);
    });
    $('hex-goto').addEventListener('keydown', function (e) { if (e.key === 'Enter') $('hex-go').click(); });
    $('hex-prev').addEventListener('click', function () { state.hex.before = null; hexLoad(Math.max(0, state.hex.addr - HEX_BYTES)); });
    $('hex-next').addEventListener('click', function () {
      state.hex.before = null;
      if (state.hex.addr + HEX_BYTES < state.hex.size) hexLoad(state.hex.addr + HEX_BYTES);
    });
    $('hex').addEventListener('mousedown', function (e) {
      const a = e.target && e.target.getAttribute && e.target.getAttribute('data-a');
      if (a !== null && a !== undefined) hexMove(Number(a));
    });
    $('hex').addEventListener('keydown', hexKey);
    loadInfo().then(find);
    setInterval(tick, 1000);
  }
  function hook(on) {
    LB.request('wasmHook', { on: on }).then(function () {
      state.scan = null;
      scanControls();
      $('results-card').hidden = true;
      Page.toast(on ? 'The tab is reloading with the hook.' : 'The tab is reloading without the hook.');
      // The page needs a moment to make its memories again.
      setTimeout(function () { loadInfo().then(find); }, 1500);
    }).catch(fail);
  }
  start();
})();
