// The macro runner (0.12.0, WP-F): the async API a macro's code gets. The host starts a macro
// with DevTools `Runtime.evaluate`:
//   window.__krunkRunMacro(async function (api) { const {key, …, log} = api;
//   <source>
//   }, runId)
// so the page's CSP needs no eval. Every API call is a `macroCall` request the host carries out
// (a hidden page's own timers are throttled, so even `wait` is the host's); the end of the
// macro is `macroDone`. A call rejected with "stopped" ends the macro, whatever it catches.
(function () {
  'use strict';
  let current = 0; // the running macro's run id (0: none)
  let stopped = false;

  function call(op, args) {
    if (stopped) return Promise.reject(new Error('stopped'));
    // Where the macro made the call: a failed call reports that line.
    const site = new Error();
    return LB.request('macroCall', { op: op, args: args || {} }).catch(function (e) {
      if (e && e.message === 'stopped') stopped = true;
      if (e) e.site = site.stack;
      throw e;
    });
  }

  // A macrotask break between repeat() rounds: Stop and replies get through even when the
  // body never awaits anything. (A MessageChannel is not throttled like timers.)
  const chan = new MessageChannel();
  const ticks = [];
  chan.port1.onmessage = function () { const r = ticks.shift(); if (r) r(); };
  function tick() {
    return new Promise(function (resolve) { ticks.push(resolve); chan.port2.postMessage(0); });
  }

  function text(v) {
    if (typeof v === 'string') return v;
    if (v instanceof Error) return String(v);
    try {
      const s = JSON.stringify(v);
      return s === undefined ? String(v) : s;
    } catch (e) { return String(v); }
  }

  // mouse.click(x, y, opts) or mouse.click(selector, opts).
  function pointArgs(a, b, c) {
    if (typeof a === 'string') return [{ selector: a }, b || {}];
    return [{ x: a, y: b }, c || {}];
  }

  function makeApi() {
    const key = Object.freeze({
      down: function (k, opts) { return call('key.down', { key: k, repeat: !(opts && opts.repeat === false) }); },
      up: function (k) { return call('key.up', { key: k }); },
      press: function (k, opts) { return call('key.press', { key: k, mods: (opts && opts.mods) || undefined }); },
      hold: function (k, ms) { return call('key.hold', { key: k, ms: ms }); },
    });
    const mouse = Object.freeze({
      click: function (a, b, c) {
        const p = pointArgs(a, b, c);
        return call('mouse.click', Object.assign(p[0], { button: p[1].button, count: p[1].count }));
      },
      move: function (x, y) { return call('mouse.move', { x: x, y: y }); },
      down: function (x, y, opts) { return call('mouse.down', { x: x, y: y, button: opts && opts.button }); },
      up: function (x, y, opts) { return call('mouse.up', { x: x, y: y, button: opts && opts.button }); },
    });
    const page = Object.freeze({
      // A function is sent as its source and called in the page: page.eval(() => document.title).
      eval: function (code) {
        return call('page.eval', { code: typeof code === 'function' ? '(' + String(code) + ')()' : String(code) });
      },
      query: function (selector, limit) { return call('page.query', { selector: selector, limit: limit }); },
    });
    const tabs = Object.freeze({
      open: function (url, opts) { return call('tabs.open', { url: url, background: !!(opts && opts.background) }); },
      switch: function (id) { return call('tabs.switch', { id: id }); },
    });
    async function repeat(n, fn) {
      if (typeof fn !== 'function') throw new TypeError('repeat(n, fn): fn must be a function');
      const forever = n === 'forever' || n === Infinity;
      if (!forever && !(Number.isInteger(n) && n >= 0)) {
        throw new TypeError("repeat(n, fn): n must be a whole number or 'forever'");
      }
      for (let i = 0; forever || i < n; i++) {
        if (stopped) throw new Error('stopped');
        await fn(i);
        await tick();
      }
    }
    return Object.freeze({
      key: key,
      mouse: mouse,
      type: function (t) { return call('type', { text: String(t) }); },
      wait: function (ms) { return call('wait', { ms: ms }); },
      page: page,
      tabs: tabs,
      navigate: function (url) { return call('navigate', { url: String(url) }); },
      repeat: repeat,
      toast: function (message) { return call('toast', { message: text(message) }); },
      log: function () { return call('log', { text: Array.prototype.map.call(arguments, text).join(' ') }); },
    });
  }

  // "message (line N)": the line in the macro's own source (the wrapper adds one line on top).
  function describe(e) {
    let msg = (e && e.message) || String(e);
    if (msg === 'stopped') return msg;
    if (e && e.name && e.name !== 'Error') msg = e.name + ': ' + msg;
    const m = /macro\.js:(\d+):(\d+)/.exec(((e && e.stack) || '') + '\n' + ((e && e.site) || ''));
    if (m && Number(m[1]) > 1) msg += ' (line ' + (Number(m[1]) - 1) + ')';
    return msg.length > 300 ? msg.slice(0, 299) + '…' : msg;
  }

  function finish(runId, ok, error) {
    if (runId !== current) return;
    current = 0;
    LB.post('macroDone', ok ? { ok: true } : { ok: false, error: error });
  }

  window.__krunkRunMacro = function (fn, runId) {
    current = runId;
    stopped = false;
    const api = makeApi();
    Promise.resolve()
      .then(function () { return fn(api); })
      .then(function () { finish(runId, true); }, function (e) { finish(runId, false, describe(e)); });
    return true;
  };

  LB.post('ready');
})();
