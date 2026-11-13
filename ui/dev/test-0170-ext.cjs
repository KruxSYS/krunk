// Krunk 0.17.0: the extension runtime's scripts in a real engine. Hooked into test-ui.cjs
// (ONLY=ExtWorker,ExtModuleWorker,ExtContentShim,ExtErrorsList).
//
// The fixture extension (scripts/vm-ext/krunk-fixture) is served on its Krunk origin with the
// extension pages' CSP, exactly as the host serves it: the relay page, the worker bootstrap
// (ui/inject/ext-worker.js with ext-page.js filled in, as core::ext_runtime::worker_bootstrap
// does) and the files. A small stand-in for the host routes the `ext` requests between the
// background and a page: enough to prove that the service worker really runs in a worker with
// the globals it expects, that importScripts is the engine's own, and that the shims speak
// the port and message protocol. The host's side of that protocol (who may talk to whom, the
// port table, senders) is Rust: core::ext_runtime's unit tests and the fixture's self-test on
// Windows (its options page, #selftest).
'use strict';
const fs = require('fs');
const path = require('path');

module.exports = function ({ openPage, check }) {
  const ROOT = path.resolve(__dirname, '../..');
  const FIXTURE = path.join(ROOT, 'scripts/vm-ext/krunk-fixture');
  const ID = 'abcdefghijklmnopabcdefghijklmnop';
  const ORIGIN = 'https://' + ID + '.ext.krunk.invalid';
  const SHIM = fs.readFileSync(path.join(ROOT, 'ui/inject/ext-page.js'), 'utf8');
  const WORKER = fs.readFileSync(path.join(ROOT, 'ui/inject/ext-worker.js'), 'utf8');
  const CONTENT = fs.readFileSync(path.join(ROOT, 'ui/inject/ext-content.js'), 'utf8');
  // The CSP of extension pages, from the source (core::ext::PAGE_CSP).
  const CSP = (() => {
    const src = fs.readFileSync(path.join(ROOT, 'core/src/ext.rs'), 'utf8');
    const body = src.split('pub const PAGE_CSP: &str =')[1].split(';\n')[0];
    return body.match(/"((?:[^"\\]|\\\n|\\.)*)"/s)[1].replace(/\\\n\s*/g, '');
  })();
  const MIME = { html: 'text/html', js: 'text/javascript', mjs: 'text/javascript', json: 'application/json', css: 'text/css', png: 'image/png' };
  // As core::ext_runtime::worker_bootstrap.
  const bootstrap = (script, module) => WORKER.replace('/*KRUNK_SW*/null', JSON.stringify({ script: './' + script, module: module })).replace('/*KRUNK_SHIM*/', () => SHIM.trim());

  // An extension on its origin with a stand-in host. `files`: extra or replaced files;
  // `background`: { script, module, install }.
  async function start(browser, background, files) {
    const context = await browser.newContext({ viewport: { width: 900, height: 700 } });
    const log = { ops: [], errors: [], unsupported: [], blocked: [] };
    await context.route('**/*', (r) => { log.blocked.push(r.request().url()); return r.abort(); });
    await context.route(ORIGIN + '/**', (r) => {
      const p = new URL(r.request().url()).pathname;
      const send = (type, body) => r.fulfill({ status: 200, headers: { 'content-type': type, 'content-security-policy': CSP, 'access-control-allow-origin': '*' }, body: body });
      if (p === '/_krunk/background.html') return send('text/html', '<!doctype html><html><head><meta charset="utf-8"><title>background</title></head><body></body></html>');
      if (p === '/_krunk/background.json') return send('application/json', JSON.stringify({ boot: '/__krunk_sw__.js', module: !!background.module, install: !!background.install }));
      if (p === '/__krunk_sw__.js') return send('text/javascript', bootstrap(background.script, !!background.module));
      if (p === '/_krunk/messages.json') return send('application/json', '{}');
      if (p === '/_krunk/env.json') return send('application/json', JSON.stringify({ log: !!background.log }));
      const rel = p.replace(/^\//, '');
      if (files && files[rel] !== undefined) return send(MIME[rel.split('.').pop()] || 'application/octet-stream', files[rel]);
      const file = path.join(FIXTURE, rel);
      if (!file.startsWith(FIXTURE) || !fs.existsSync(file) || !fs.statSync(file).isFile()) return r.fulfill({ status: 404, body: 'not found' });
      return send(MIME[rel.split('.').pop()] || 'application/octet-stream', fs.readFileSync(file));
    });
    // The stand-in host. Ends: 'bg' (the relay page of the worker) and pages by number.
    const ends = new Map(); // key -> { page, ready, queue }
    const ports = new Map(); // id -> { opener, receivers, connected }
    const pending = new Map(); // message id -> { from, reqId, waiting, listened }
    let nextMessage = 1;
    const deliver = (key, obj) => { const e = ends.get(key); if (e && !e.page.isClosed()) e.page.evaluate((o) => window.__fromHost(o), obj).catch(() => {}); };
    const event = (key, kind, data) => {
      const e = ends.get(key); if (!e) return;
      const obj = { event: 'ext', ext: ID, kind: kind, data: data };
      if (key === 'bg' && !e.ready) e.queue.push(obj); else deliver(key, obj);
    };
    const reply = (key, reqId, ok, data) => { if (reqId !== undefined && reqId !== null) deliver(key, ok ? { event: 'reply', reqId: reqId, ok: true, data: data } : { event: 'reply', reqId: reqId, ok: false, error: data }); };
    const sender = (key) => key === 'bg' ? { id: ID, url: 'chrome-extension://' + ID + '/' + background.script, origin: 'chrome-extension://' + ID }
      : { id: ID, url: ends.get(key).page.url().replace(ORIGIN, 'chrome-extension://' + ID), origin: 'chrome-extension://' + ID, tab: { id: 7 }, frameId: 0 };
    const NO_RECEIVER = 'Could not establish connection. Receiving end does not exist.';
    function request(key, m) {
      const d = m.data || {};
      log.ops.push({ from: key, op: m.op, data: d });
      switch (m.op) {
        case 'bgReady': { const e = ends.get('bg'); if (e && key === 'bg') { e.ready = true; e.queue.splice(0).forEach((o) => deliver('bg', o)); } return; }
        case 'log': if (d.level === 'error') log.errors.push({ from: key, text: d.text, file: d.file, line: d.line }); return;
        case 'unsupported': log.unsupported.push(d.api); return;
        case 'runtime.sendMessage': {
          const to = Array.from(ends.keys()).filter((k) => k !== key);
          if (!to.length) return reply(key, m.reqId, false, NO_RECEIVER);
          const id = nextMessage++;
          pending.set(id, { from: key, reqId: m.reqId, waiting: to.length, listened: false });
          to.forEach((k) => event(k, 'message', { id: id, data: d.message, sender: sender(key) }));
          return;
        }
        case 'respond': {
          const p = pending.get(d.id); if (!p) return;
          if (!d.none && !d.empty) { pending.delete(d.id); return reply(p.from, p.reqId, true, d.data); }
          p.waiting--; p.listened = p.listened || !!d.empty;
          if (p.waiting <= 0) { pending.delete(d.id); if (p.listened) reply(p.from, p.reqId, true, null); else reply(p.from, p.reqId, false, NO_RECEIVER); }
          return;
        }
        case 'port.open': {
          const to = d.tabId !== null && d.tabId !== undefined ? [] : Array.from(ends.keys()).filter((k) => k !== key);
          if (!to.length) return event(key, 'portDisconnect', { port: d.id, error: NO_RECEIVER });
          ports.set(d.id, { opener: key, receivers: to, connected: false });
          to.forEach((k) => event(k, 'portConnect', { port: d.id, name: d.name, sender: sender(key) }));
          return;
        }
        case 'port.accept': { const p = ports.get(d.id); if (p) p.connected = true; return; }
        case 'port.post': {
          const p = ports.get(d.id); if (!p) return;
          (key === p.opener ? p.receivers : [p.opener]).forEach((k) => event(k, 'portMessage', { port: d.id, data: d.data }));
          return;
        }
        case 'port.close': {
          const p = ports.get(d.id); if (!p) return;
          if (key === p.opener) { ports.delete(d.id); p.receivers.forEach((k) => event(k, 'portDisconnect', { port: d.id, error: null })); return; }
          p.receivers = p.receivers.filter((k) => k !== key);
          if (!p.receivers.length) { ports.delete(d.id); event(p.opener, 'portDisconnect', { port: d.id, error: p.connected ? null : NO_RECEIVER }); }
          return;
        }
        case 'storage.get': return reply(key, m.reqId, true, {});
        case 'tabs.query': return reply(key, m.reqId, true, []);
        default: return reply(key, m.reqId, true, null);
      }
    }
    await context.exposeBinding('__toHost', (source, key, json) => { try { request(key, JSON.parse(json)); } catch (e) { log.errors.push({ from: 'test', text: String(e && e.stack || e) }); } });
    // In every page of the extension: the bridge the renderer gives extension pages, then
    // the shim, before the page's own scripts (as app/src/ffi/cef/renderer.rs does).
    const inject = '(function () { if (!/\\.ext\\.krunk\\.invalid$/.test(location.hostname) || window.top !== window) return; var key = (/#end=(\\w+)/.exec(location.hash) || [])[1] || "bg"; var ls = [];'
      + 'window.__fromHost = function (o) { ls.forEach(function (f) { f({ data: o }); }); };'
      + 'var __lbHost = { postMessage: function (m) { window.__toHost(key, JSON.stringify(m)); }, addEventListener: function (t, f) { ls.push(f); } };'
      + '(function (__lbHost) {\n' + SHIM + '\n})(__lbHost); })();';
    await context.addInitScript(inject);
    const open = async (key, file) => {
      const page = await context.newPage();
      page.on('pageerror', (e) => log.errors.push({ from: key + ' (pageerror)', text: String(e) }));
      // The fragment tells the bridge which end the page is (the real host knows the view).
      ends.set(key, { page: page, ready: key !== 'bg', queue: [] });
      await page.goto(ORIGIN + '/' + file + (key === 'bg' ? '' : '#end=' + key));
      return page;
    };
    return { context, log, open, event, ends };
  }
  const waitFor = async (fn, ms) => { const end = Date.now() + (ms || 5000); while (Date.now() < end) { if (fn()) return true; await new Promise((r) => setTimeout(r, 25)); } return false; };

  async function testExtWorker(browser) {
    console.log('0.17.0: the background is a worker; ports and messages between it and a page');
    const x = await start(browser, { script: 'bg.js', module: false, install: true });
    const bg = await x.open('bg', '_krunk/background.html');
    // Events sent before the worker's script ran wait for it (the host queues them).
    x.event('bg', 'installed', { reason: 'install' });
    check(await waitFor(() => x.log.ops.some((o) => o.from === 'bg' && o.op === 'bgReady')), 'the worker ran the service worker and said so (bgReady)');
    check(await waitFor(() => x.log.ops.some((o) => o.op === 'storage.set' && o.data.items && o.data.items.installedReason === 'install')), 'onInstalled, queued before the worker was ready, reached its listener');
    check(await bg.evaluate(() => typeof window.chrome === 'undefined' || !window.chrome.runtime || !window.chrome.runtime.id), 'the hidden background page itself has no chrome.* (it only relays)');
    check(x.log.ops.some((o) => o.from === 'bg' && o.op === 'alarms.create'), 'calls made while the script ran reached the host');
    check(x.log.unsupported.indexOf('webRequest') >= 0 && !x.log.unsupported.some((a) => /^__/.test(a)), 'chrome.webRequest listeners are accepted and reported once (' + x.log.unsupported.join(', ') + ')');

    const page = await x.open('p1', 'options.html');
    const env = await page.evaluate(() => chrome.runtime.sendMessage({ kind: 'env' }));
    check(env && env.noWindow && env.noDocument, 'the service worker has no window and no document (' + JSON.stringify(env && { w: env.noWindow, d: env.noDocument }) + ')');
    check(env && env.swState === 'activated' && env.clients && env.registration && env.skipWaiting, 'self.serviceWorker.state is "activated"; clients, registration and skipWaiting exist');
    check(env && env.imported === 'imported', 'importScripts("lib/sw-helper.js") is the engine\'s own (no eval under the extension CSP)');
    check(env && env.globalChrome, 'chrome is a global of the worker (globalThis.chrome)');
    check(env && env.sawInstall === true, 'a fresh installation gets the install event');
    check(env && env.sender && env.sender.origin === 'chrome-extension://' + ID && env.sender.tab && env.sender.tab.id === 7, 'onMessage gets the sender the host gives');

    // A port from the page to the background and back.
    const echo = await page.evaluate(() => new Promise((resolve) => {
      const port = chrome.runtime.connect({ name: 'echo' });
      window.__port = port;
      port.onMessage.addListener((m, p) => resolve({ m: m, same: p === port, name: port.name }));
      port.postMessage({ n: 1 });
    }));
    check(echo.m && echo.m.echo && echo.m.echo.n === 1 && echo.m.name === 'echo' && echo.same, 'runtime.connect: a message goes to the background\'s onConnect port and its answer comes back');
    check(echo.m.sender && echo.m.sender.url.indexOf('chrome-extension://' + ID + '/options.html') === 0 && echo.m.sender.origin === 'chrome-extension://' + ID, 'port.sender is what the host says (Chrome\'s form of the page address)');
    check(echo.m.left_out === undefined && Array.isArray(echo.m.kept) && echo.m.kept[0] === null && echo.m.kept[1] === 1, 'a message travels as JSON, as in Chrome: a function in it is left out and does not make postMessage throw (' + JSON.stringify(echo.m.kept) + ')');
    await page.evaluate(() => window.__port.disconnect());
    const threw = await page.evaluate(() => { try { window.__port.postMessage(1); return false; } catch (e) { return /disconnected port/.test(e.message); } });
    check(threw, 'postMessage on a disconnected port throws');
    await new Promise((r) => setTimeout(r, 150));
    const env2 = await page.evaluate(() => chrome.runtime.sendMessage({ kind: 'env' }));
    check(env2.echoDisconnects === env.echoDisconnects + 1, 'disconnect() fires onDisconnect at the other end');
    // A port nobody takes: onDisconnect with runtime.lastError.
    const nobody = await page.evaluate(() => new Promise((resolve) => {
      const port = chrome.tabs.connect(999, { name: 'x' });
      port.onDisconnect.addListener(() => resolve({ error: chrome.runtime.lastError && chrome.runtime.lastError.message, after: 0 }));
    }));
    check(/Receiving end does not exist/.test(nobody.error || ''), 'a port without a receiver disconnects with runtime.lastError (' + nobody.error + ')');
    check(await page.evaluate(() => chrome.runtime.lastError === undefined), 'runtime.lastError is only set inside the callback');
    await page.close();
    x.ends.delete('p1');

    // Messages: a page without listeners does not answer for the background, and the answer
    // of a listener that returns a promise is used.
    const p2 = await x.open('p2', 'popup.html');
    const p3 = await x.open('p3', 'window.html');
    await p3.evaluate(() => { chrome.runtime.onMessage.addListener((m) => (m && m.kind === 'who' ? Promise.resolve('p3 via a promise') : undefined)); });
    const tabs = await p2.evaluate(() => chrome.runtime.sendMessage({ kind: 'env' }).then((e) => e && e.swState));
    check(tabs === 'activated', 'sendMessage: the background answers although other pages got the message too');
    const who = await p2.evaluate(() => chrome.runtime.sendMessage({ kind: 'who' }));
    check(who === 'p3 via a promise', 'sendMessage: a listener that returns a promise answers with its value (' + JSON.stringify(who) + ')');
    const none = await p2.evaluate(() => chrome.runtime.sendMessage({ kind: 'nobody-knows-this' }).then((v) => ({ v: v === null || v === undefined }), (e) => ({ e: e.message })));
    check(none.v === true, 'sendMessage: listeners that do not answer give undefined, not an error (' + JSON.stringify(none) + ')');
    // The window page connected a port named "window" (window.js) and closes when told to.
    check(await waitFor(() => x.log.ops.some((o) => o.from === 'p3' && o.op === 'port.open' && o.data.name === 'window')), 'a page connects as soon as it loads');

    // Errors of the contexts reach the host: console.error, uncaught, unhandled rejection.
    const before = x.log.errors.length;
    await p2.evaluate(() => chrome.runtime.sendMessage({ kind: 'errors' }));
    await p2.evaluate(() => console.error('fixture: console.error from a page', { a: 1 }));
    await waitFor(() => x.log.errors.length >= before + 4, 3000);
    const texts = x.log.errors.slice(before).map((e) => e.from + ': ' + e.text);
    check(texts.some((t) => /^bg: fixture: console\.error from the background/.test(t)), 'console.error in the worker is reported');
    check(texts.some((t) => /^bg: .*uncaught error in the background/.test(t)), 'an uncaught error in the worker is reported');
    check(texts.some((t) => /^bg: .*unhandled rejection in the background/.test(t)), 'an unhandled rejection in the worker is reported');
    check(texts.filter((t) => /^bg: .*uncaught error in the background/.test(t)).length === 1, 'each once (the relay page does not report a running worker\'s errors again)');
    check(texts.some((t) => /^p2: fixture: console\.error from a page \{"a":1\}/.test(t)), 'console.error in a page is reported (' + texts.join(' | ').slice(0, 300) + ')');
    const uncaught = x.log.errors.slice(before).find((e) => /uncaught error/.test(e.text));
    check(uncaught && /bg\.js/.test(uncaught.file || uncaught.text), 'with the file it happened in (' + (uncaught && (uncaught.file + ':' + uncaught.line)) + ')');
    check(x.log.blocked.length === 0, 'nothing was fetched from outside the extension (' + x.log.blocked.slice(0, 3).join(', ') + ')');
    // (The worker's own uncaught errors also surface on the page that started it.)
    const own = x.log.errors.filter((e) => /pageerror|test/.test(e.from) && !/fixture:/.test(e.text));
    check(own.length === 0, 'no script errors in the shims (' + own.map((e) => e.text).join('; ').slice(0, 300) + ')');
    await x.context.close();
  }

  async function testExtModuleWorker(browser) {
    console.log('0.17.0: a module service worker, and a worker that fails to load');
    const files = {
      'mod-sw.js': 'import { helper } from "./lib/mod-helper.js";\nchrome.runtime.onMessage.addListener((m, s, send) => { send({ helper: helper(), noWindow: typeof window === "undefined", state: self.serviceWorker.state, install: globalThis.__sawInstall === true }); });\nself.addEventListener("install", () => { globalThis.__sawInstall = true; });\n',
      'lib/mod-helper.js': 'export function helper() { return "module helper"; }\n',
    };
    const x = await start(browser, { script: 'mod-sw.js', module: true, install: false }, files);
    await x.open('bg', '_krunk/background.html');
    check(await waitFor(() => x.log.ops.some((o) => o.from === 'bg' && o.op === 'bgReady')), 'a module service worker starts (static imports resolve next to it)');
    const page = await x.open('p1', 'window.html');
    const r = await page.evaluate(() => chrome.runtime.sendMessage({ kind: 'x' }));
    check(r && r.helper === 'module helper' && r.noWindow && r.state === 'activated', 'it answers from a worker (' + JSON.stringify(r) + ')');
    check(r && r.install === false, 'no install event on an ordinary start');
    await x.context.close();

    const y = await start(browser, { script: 'broken.js', module: false, install: false }, { 'broken.js': 'chrome.runtime.onMessage.addListener(function () {});\nthis is not javascript' });
    await y.open('bg', '_krunk/background.html');
    check(await waitFor(() => y.log.ops.some((o) => o.from === 'bg' && o.op === 'bgReady')), 'a service worker that does not parse still lets queued events go (bgReady)');
    check(y.log.errors.some((e) => e.from === 'bg' && /SyntaxError|Unexpected/.test(e.text)), 'and its error is reported (' + y.log.errors.map((e) => e.text).join(' | ').slice(0, 200) + ')');
    await y.context.close();

    const z = await start(browser, { script: 'throws.js', module: false, install: false }, { 'throws.js': 'importScripts("lib/sw-helper.js");\nnull.x;' });
    await z.open('bg', '_krunk/background.html');
    check(await waitFor(() => z.log.ops.some((o) => o.from === 'bg' && o.op === 'bgReady')), 'a service worker that throws at start still says it is done');
    check(z.log.errors.some((e) => e.from === 'bg' && /Cannot read properties of null/.test(e.text)), 'and the TypeError is reported');
    await z.context.close();
  }

  // ext-content.js in a page, with the DevTools binding and the host's deliveries played by
  // the test (the isolated world is the host's business: any world works for the shim).
  async function testExtContentShim(browser) {
    console.log('0.17.0: the content-script shim: global chrome, ports');
    const context = await browser.newContext();
    await context.route('**/*', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: '<!doctype html><title>page</title><body>web page</body>' }));
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.goto('https://example.test/a?b=1');
    const r = await page.evaluate(({ shim, id }) => {
      const sent = [];
      window['__krunkExt_' + id] = (json) => { sent.push(JSON.parse(json)); };
      const ext = (0, eval)('(' + shim.replace(/^\/\/.*\n/gm, '') + ')')(id, { name: 'x', manifest_version: 3 }, {});
      const out = { global: window.chrome && window.chrome.runtime && window.chrome.runtime.id === id, browser: window.browser === window.chrome, run: typeof ext.run };
      // A port to the background.
      const port = chrome.runtime.connect({ name: 'cs' });
      const got = [];
      port.onMessage.addListener((m) => got.push(m));
      port.onDisconnect.addListener(() => got.push('closed:' + (chrome.runtime.lastError ? chrome.runtime.lastError.message : '')));
      port.postMessage({ hi: 1 });
      const open = sent.find((m) => m.cmd === 'port.open');
      out.open = open && open.data;
      out.post = sent.find((m) => m.cmd === 'port.post');
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'portMessage', port: open.data.id, data: { echo: 1 } }));
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'portDisconnect', port: open.data.id, error: 'gone' }));
      out.got = got;
      // tabs.connect from the background: nobody listens, then somebody does.
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'portConnect', port: 'pabc_1', name: 'n', sender: { id: id } }));
      out.refused = sent.some((m) => m.cmd === 'port.close' && m.data.id === 'pabc_1');
      chrome.runtime.onConnect.addListener((p) => { p.onMessage.addListener((m) => p.postMessage({ pong: m, from: p.sender.id === id })); });
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'portConnect', port: 'pabc_2', name: 'n', sender: { id: id } }));
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'portMessage', port: 'pabc_2', data: 5 }));
      out.accepted = sent.some((m) => m.cmd === 'port.accept' && m.data.id === 'pabc_2');
      out.pong = sent.find((m) => m.cmd === 'port.post' && m.data.id === 'pabc_2');
      // A message nobody listens for, then one that is answered.
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'message', id: 3, data: 1, sender: { id: id } }));
      out.none = sent.find((m) => m.cmd === 'respond' && m.data.id === 3);
      chrome.runtime.onMessage.addListener((m, s, send) => { if (m === 2) send('two'); });
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'message', id: 4, data: 2, sender: { id: id } }));
      window.__krunkExtDeliver(JSON.stringify({ ext: id, event: 'message', id: 5, data: 9, sender: { id: id } }));
      out.answered = sent.find((m) => m.cmd === 'respond' && m.data.id === 4);
      out.empty = sent.find((m) => m.cmd === 'respond' && m.data.id === 5);
      chrome.runtime.sendMessage({ a: 1 });
      out.message = sent.find((m) => m.cmd === 'runtime.sendMessage');
      console.error('content boom');
      out.logged = sent.find((m) => m.cmd === 'log' && /content boom/.test(m.data.text));
      return out;
    }, { shim: CONTENT, id: ID });
    check(r.global && r.browser && r.run === 'function', 'chrome and browser are globals of the script\'s world');
    check(r.open && /^p[a-z0-9]+_1$/.test(r.open.id) && r.open.name === 'cs' && r.open.src && r.open.src.top === true && r.open.src.url === 'https://example.test/a?b=1', 'runtime.connect opens a port and says which frame it is (' + JSON.stringify(r.open) + ')');
    check(r.post && r.post.data.data.hi === 1, 'port.postMessage reaches the host');
    check(r.got.length === 2 && r.got[0].echo === 1 && r.got[1] === 'closed:gone', 'onMessage and onDisconnect (with runtime.lastError) fire (' + JSON.stringify(r.got) + ')');
    check(r.refused, 'a port from the background is closed at once when nothing listens for onConnect');
    check(r.accepted && r.pong && r.pong.data.data.pong === 5 && r.pong.data.data.from === true, 'with a listener it is accepted and answers');
    check(r.none && r.none.data.none === true, 'a message without listeners is answered as "no listener"');
    check(r.answered && r.answered.data.data === 'two' && r.empty && r.empty.data.empty === true, 'sendResponse answers; listeners that do not answer say so');
    check(r.message && r.message.data.src && r.message.data.src.top === true, 'sendMessage says which frame it is from');
    check(!!r.logged, 'console.error of the world is reported');
    // fetch() of the extension's own file goes to the host (the page's request would need a
    // web-accessible resource); any other fetch is the page's as before.
    const f = await page.evaluate(async ({ id }) => {
      const reads = [];
      window['__krunkExt_' + id] = (json) => {
        const m = JSON.parse(json);
        if (m.cmd !== 'resource.read') return;
        reads.push(m.data.url);
        window.__krunkExtDeliver(JSON.stringify({ ext: id, reply: m.id, data: { mime: 'application/json', body: btoa('{"ok":1}') } }));
      };
      const own = await fetch(chrome.runtime.getURL('data/x.json'));
      const other = await (await fetch('/other')).text();
      const posted = await (await fetch(chrome.runtime.getURL('data/x.json'), { method: 'POST' })).text();
      return { reads: reads, json: await own.json(), type: own.headers.get('content-type'), url: own.url, other: other, posted: posted };
    }, { id: ID });
    check(f.reads.length === 1 && f.reads[0] === ORIGIN + '/data/x.json' && f.json.ok === 1 && f.type === 'application/json' && f.url === f.reads[0], 'fetch of the extension\'s own file is read by the host (' + JSON.stringify(f) + ')');
    check(/web page/.test(f.other) && /web page/.test(f.posted), 'another fetch (and a POST) is the page\'s own request');
    check(errors.length === 0, 'no errors (' + errors.join('; ') + ')');
    await context.close();
  }

  // krunk://extensions: the Errors list of an extension.
  async function testExtErrorsList(browser) {
    console.log('0.17.0: krunk://extensions shows an extension\'s errors');
    const ext = (id, name, errors) => ({ id: id, name: name, version: '1.0', description: 'Test', enabled: true, allowedInPrivate: false, source: 'webstore:' + id, installedAt: 1, updatedAt: 1,
      scope: [], apis: [{ api: 'storage', supported: true, reason: '' }], runtimeUnsupported: ['sidePanel'], dnrTranslated: 0, dnrSkipped: 0,
      hasOptions: false, hasPopup: true, hasBackground: true, contentScripts: 1, icon: null, permissions: ['storage'], errors: errors });
    const errs = [
      { message: 'TypeError: Cannot read properties of undefined (reading \'state\')\n    at service-worker.js:1:1', source: 'service-worker.js:15408', context: 'background', count: 1 },
      { message: 'fixture: console.error from the popup', source: '', context: 'popup', count: 3 },
    ];
    const exts = [ext(ID, 'Wallet', errs), ext('b'.repeat(32), 'Quiet', [])];
    const mock = (list) => { window.__exts = list; window.__host.handlers.extList = () => window.__exts; };
    const { page, errors } = await openPage(browser, 'https://krunk.invalid/extensions.html', { width: 900, height: 900 }, 1, mock, exts);
    await page.waitForSelector('.ext');
    const st = () => page.evaluate((id) => {
      const card = document.querySelector('.ext[data-id="' + id + '"]'), quiet = document.querySelector('.ext:not([data-id="' + id + '"])');
      const items = Array.from(card.querySelectorAll('dd.errors li'));
      return { note: (card.querySelector('.error-note') || {}).textContent || '', items: items.map((li) => li.textContent), open: card.querySelector('details.more').open,
        quietNote: !!quiet.querySelector('.error-note'), quietList: !!quiet.querySelector('dd.errors'), unsupported: !!card.querySelector('dd.unsupported'), wide: document.documentElement.scrollWidth > window.innerWidth };
    }, ID);
    let s = await st();
    check(/2 errors/.test(s.note) && !s.quietNote && !s.quietList, 'the card says that the extension reported errors; one without errors says nothing (' + s.note + ')');
    check(s.items.length === 2 && /^popup/.test(s.items[0]) && /console\.error from the popup/.test(s.items[0]) && /×3/.test(s.items[0]), 'newest first, with the context and how often (' + JSON.stringify(s.items[0]) + ')');
    check(/^background/.test(s.items[1]) && /reading 'state'/.test(s.items[1]) && /service-worker\.js:15408/.test(s.items[1]), 'the message with file and line (' + JSON.stringify(s.items[1]).slice(0, 160) + ')');
    check(s.unsupported, 'the "Not in Krunk" list is still there');
    // Open details stay open when the list is drawn again with one more error.
    await page.click('.ext[data-id="' + ID + '"] details.more summary');
    await page.waitForFunction((id) => document.querySelector('.ext[data-id="' + id + '"] details.more').open, ID);
    await page.waitForTimeout(50);
    exts[0].errors = errs.concat([{ message: 'Uncaught (in promise) Error: third', source: 'popup.js:9', context: 'popup', count: 1 }]);
    await page.evaluate((l) => window.__host.emit({ event: 'extChanged', extensions: l }), exts);
    s = await st();
    check(s.open && s.items.length === 3 && /third/.test(s.items[0]), 'a new error appears while the details are open, and they stay open');
    check(!s.wide, 'long messages do not widen the page');
    check(errors.length === 0, 'no errors (' + errors.join('; ') + ')');
    await page.context().close();
  }

  return { testExtWorker, testExtModuleWorker, testExtContentShim, testExtErrorsList };
};
