// Krunk userscript loader (0.12.0). A function expression that core::userscripts calls with
// (binding name, nonce, version, script ids); the host registers the result with
// Page.addScriptToEvaluateOnNewDocument in the main world of every frame of a tab.
//
// - The DevTools binding is taken and removed from `window` before any page script runs. GM
//   calls carry the nonce, which page code can't read: it lives only in this closure.
// - It returns `{add, go}`. Each userscript is added as its own function, outside this scope,
//   so a script can't reach the binding, the nonce or another script's values. `go` starts the
//   added scripts at their run-at time; after it nothing more can be added.
// - The matcher between "match:begin" and "match:end" is the twin of
//   core::userscripts::matches_url; both are tested against
//   core/tests/fixtures/userscripts/match-vectors.json.
(function (binding, nonce, version, ids) {
  const send = window[binding]; try { delete window[binding]; } catch (_) { /* not configurable */ }
  // Built-ins taken before page scripts could replace them.
  const str = JSON.stringify, parse = JSON.parse, keys = Object.keys, create = Object.create;
  const freeze = Object.freeze, apply = Reflect.apply, URLCtor = URL, P = Promise;
  const later = window.setTimeout, idle = window.requestIdleCallback, con = console, conError = console.error;
  const CSSSheet = window.CSSStyleSheet, Observer = window.MutationObserver;
  const href = location.href;
  const top = window.top === window;

  // match:begin
  // Krunk's own origins (pages, the shell, extension pages) never get userscripts.
  function usOwnHost(host) {
    return /(^|\.)krunk\.invalid\.*$/i.test(host);
  }
  function usLower(s) {
    return s.replace(/[A-Z]+/g, function (c) { return c.toLowerCase(); });
  }
  // `*` matches anything (the same as core::ext::glob_match).
  function usGlob(p, t) {
    let pi = 0, ti = 0, star = -1, mark = 0;
    while (ti < t.length) {
      if (pi < p.length && p[pi] === t[ti]) { pi++; ti++; }
      else if (pi < p.length && p[pi] === '*') { star = pi; mark = ti; pi++; }
      else if (star >= 0) { pi = star + 1; mark++; ti = mark; }
      else return false;
    }
    while (pi < p.length && p[pi] === '*') pi++;
    return pi === p.length;
  }
  // `/…/flags`: a regular expression (not supported, never matches).
  function usIsRegex(p) {
    if (p.length < 2 || p[0] !== '/') return false;
    const body = p.slice(1), i = body.lastIndexOf('/');
    return i >= 0 && /^[gimsuy]*$/.test(body.slice(i + 1));
  }
  // A Chrome match pattern (the same rules as core::ext::MatchPattern).
  function usMatchPattern(p, u, pathQuery) {
    let scheme, host, path;
    if (p === '<all_urls>') { scheme = '*'; host = '*'; path = '/*'; }
    else {
      const i = p.indexOf('://');
      if (i < 0) return false;
      scheme = p.slice(0, i);
      const rest = p.slice(i + 3);
      if (['*', 'http', 'https', 'file', 'ftp'].indexOf(scheme) < 0) return false;
      const j = rest.indexOf('/');
      host = j < 0 ? rest : rest.slice(0, j);
      path = j < 0 ? '/' : rest.slice(j);
      if (scheme !== 'file') {
        if (!host || host.indexOf(':') >= 0 || host.indexOf('@') >= 0) return false;
        const bare = host.indexOf('*.') === 0 ? host.slice(2) : host;
        if (bare !== '*' && (bare.indexOf('*') >= 0 || !bare)) return false;
      }
      if (!path || path.indexOf('\0') >= 0) return false;
      host = usLower(host);
    }
    const s = u.protocol.slice(0, -1);
    if (scheme === '*' ? !(s === 'http' || s === 'https') : s !== scheme) return false;
    const h = usLower(u.hostname);
    if (scheme !== 'file' && host !== '*') {
      if (host.indexOf('*.') === 0) {
        const d = host.slice(2);
        if (h !== d && !(h.length > d.length && h.slice(-d.length - 1) === '.' + d)) return false;
      } else if (h !== host) return false;
    }
    return usGlob(path, pathQuery);
  }
  // Does a script with these patterns run on `url`? `@exclude` wins; `@include`/`@exclude` are
  // whole-address globs (without the #fragment).
  function usMatches(m, url) {
    let u;
    try { u = new URLCtor(url); } catch (_) { return false; }
    if (!/^(https?|file):$/.test(u.protocol) || usOwnHost(u.hostname)) return false;
    const whole = u.href.split('#')[0];
    const q = whole.indexOf('?');
    const pathQuery = u.pathname + (q >= 0 ? whole.slice(q) : '');
    const glob = function (p) { return !usIsRegex(p) && usGlob(p, whole); };
    if (m.excludes.some(glob)) return false;
    return m.matches.some(function (p) { return usMatchPattern(p, u, pathQuery); }) || m.includes.some(glob);
  }
  // match:end

  const web = /^(https?|file):$/.test(location.protocol) && !usOwnHost(location.hostname);
  const NAMES = ['addStyle', 'getValue', 'setValue', 'deleteValue', 'listValues', 'setClipboard', 'notification', 'openInTab'];
  const expected = create(null);
  for (let i = 0; i < ids.length; i++) expected[ids[i]] = true;
  const added = [];
  let closed = false;

  // A GM call to the host: built by hand from strings, so no page hook (toJSON on a prototype)
  // sees the nonce.
  function post(id, op, fields) {
    if (typeof send !== 'function') return;
    let t = '{"nonce":' + str(nonce) + ',"script":' + str(id) + ',"op":' + str(op);
    for (let i = 0; i < fields.length; i += 2) {
      const v = fields[i + 1];
      t += ',' + str(fields[i]) + ':' + (typeof v === 'boolean' ? (v ? 'true' : 'false') : str(String(v)));
    }
    try { send(t + '}'); } catch (_) { /* the binding is gone */ }
  }

  // A <style> in the document; when the page's CSP refuses inline styles, a constructed
  // stylesheet instead (CSP doesn't apply to those).
  function addStyle(css) {
    const el = document.createElement('style');
    el.textContent = css;
    const put = function () {
      const root = document.head || document.documentElement;
      if (!root) return false;
      root.appendChild(el);
      if (!el.sheet && CSSSheet) {
        try { const s = new CSSSheet(); s.replaceSync(css); document.adoptedStyleSheets = document.adoptedStyleSheets.concat([s]); } catch (_) { /* no constructed sheets */ }
      }
      return true;
    };
    if (!put() && Observer) {
      const mo = new Observer(function () { if (put()) mo.disconnect(); });
      mo.observe(document, { childList: true, subtree: true });
    }
    return el;
  }

  // The arguments of a script's function: GM_info, the GM_* functions (undefined unless
  // granted), GM and unsafeWindow.
  function api(s) {
    const granted = function (n) { return s.grants.indexOf('GM_' + n) >= 0 || s.grants.indexOf('GM.' + n) >= 0; };
    const vals = create(null);
    for (let i = 0; i < s.values.length; i++) vals[s.values[i][0]] = s.values[i][1];
    const info = freeze({ script: freeze(s.info), scriptHandler: 'Krunk', version: version });
    const fns = create(null);
    fns.addStyle = function (css) { return addStyle(String(css)); };
    fns.getValue = function (key, def) {
      key = String(key);
      if (!(key in vals)) return def;
      try { return parse(vals[key]); } catch (_) { return def; }
    };
    fns.setValue = function (key, value) {
      key = String(key);
      const text = str(value);
      if (text === undefined) { fns.deleteValue(key); return; }
      vals[key] = text;
      post(s.id, 'set', ['key', key, 'value', text]);
    };
    fns.deleteValue = function (key) {
      key = String(key);
      delete vals[key];
      post(s.id, 'delete', ['key', key]);
    };
    fns.listValues = function () { return keys(vals); };
    fns.setClipboard = function (text) { post(s.id, 'clipboard', ['text', String(text)]); };
    fns.notification = function (details, title) {
      let text = details, head = title;
      if (details && typeof details === 'object') { text = details.text; head = details.title; }
      post(s.id, 'notify', ['text', text == null ? '' : String(text), 'title', head == null ? '' : String(head)]);
    };
    fns.openInTab = function (url, options) {
      let background = false;
      if (typeof options === 'boolean') background = options;
      else if (options && typeof options === 'object') background = options.active !== undefined ? !options.active : !!options.loadInBackground;
      let abs;
      try { abs = new URLCtor(String(url), href).href; } catch (_) { return null; }
      post(s.id, 'openTab', ['url', abs, 'background', background]);
      return { closed: false, close: function () {}, onclose: null };
    };
    const gm = { info: info };
    const args = [info];
    for (let i = 0; i < NAMES.length; i++) {
      const n = NAMES[i], on = granted(n), f = fns[n];
      args[args.length] = on ? f : undefined;
      if (on) {
        gm[n] = function () {
          const a = arguments;
          return new P(function (ok, fail) { try { ok(apply(f, undefined, a)); } catch (e) { fail(e); } });
        };
      }
    }
    args[args.length] = gm;
    args[args.length] = window;
    return args;
  }

  // Runs `f` at a run-at stage: 0 document-start (now), 1 document-end (DOMContentLoaded),
  // 2 document-idle (after load, when the page is idle).
  function when(stage, f) {
    if (stage === 0) { f(); return; }
    const state = document.readyState;
    if (stage === 1) {
      if (state === 'loading') document.addEventListener('DOMContentLoaded', f, { once: true });
      else f();
      return;
    }
    if (state === 'complete') { f(); return; }
    window.addEventListener('load', function () {
      if (typeof idle === 'function') apply(idle, window, [f, { timeout: 2000 }]);
      else apply(later, window, [f, 0]);
    }, { once: true });
  }

  function add(s, run) {
    if (closed || !web || typeof run !== 'function' || !(s.id in expected)) return;
    delete expected[s.id];
    if (s.noframes && !top) return;
    if (!s.force && !usMatches(s, href)) return;
    added[added.length] = [s, run, api(s)];
  }

  function go() {
    if (closed) return;
    closed = true;
    for (let i = 0; i < added.length; i++) {
      const s = added[i][0], run = added[i][1], args = added[i][2];
      when(s.runAt, function () {
        try { apply(run, window, args); } catch (e) {
          try { apply(conError, con, ['Userscript "' + s.name + '" failed:', e]); } catch (_) { /* no console */ }
        }
        if (top) post(s.id, 'ran', []);
      });
    }
  }

  return freeze({ add: add, go: go });
})
