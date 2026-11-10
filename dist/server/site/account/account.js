// The account pages (docs/sync/PLAN.md §4.6): one script for all of them, chosen by
// <body data-page>. Same-origin fetch only; the CSRF token comes from the login response or
// GET web/me and is never stored. Cloudflare's Turnstile script is added only on the form pages
// and only when accounts are enabled, so otherwise nothing loads from another site.
(function () {
  'use strict';
  var page = document.body.getAttribute('data-page');
  var statusEl = document.getElementById('status');
  var csrf = null;

  function $(sel) { return document.querySelector(sel); }
  function show(el, on) { if (el) el.hidden = !on; }
  // The banner: role=status, or role=alert for an error (set before the text, so it is read).
  function say(text, kind) {
    statusEl.setAttribute('role', kind === 'error' ? 'alert' : 'status');
    statusEl.textContent = text || '';
    statusEl.className = 'status' + (kind ? ' ' + kind : '');
  }
  function field(form, name) { return form.elements.namedItem(name); }
  function value(form, name) { var f = field(form, name); return f ? f.value : ''; }

  function api(method, path, body) {
    var opts = { method: method, credentials: 'same-origin', cache: 'no-store', headers: {} };
    if (body !== undefined) {
      opts.headers['Content-Type'] = 'application/json';
      opts.body = JSON.stringify(body);
    }
    if (csrf && method !== 'GET') opts.headers['X-Krunk-CSRF'] = csrf;
    return fetch('/api/v1/web/' + path, opts).then(jsonResult, networkError);
  }
  function jsonResult(res) {
    return res.text().then(function (t) {
      var j = {};
      try { j = t ? JSON.parse(t) : {}; } catch (e) { j = {}; }
      return { status: res.status, ok: res.ok, body: j || {}, retryAfter: res.headers.get('Retry-After') };
    });
  }
  function networkError() {
    return { status: 0, ok: false, body: { error: 'network', message: 'The connection failed. Check your network and try again.' } };
  }
  // A POST whose success answer is a file (web/export): `blob` and the Content-Disposition on
  // 200, otherwise the same result as api().
  function apiFile(path, body) {
    var opts = { method: 'POST', credentials: 'same-origin', cache: 'no-store',
      headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) };
    if (csrf) opts.headers['X-Krunk-CSRF'] = csrf;
    return fetch('/api/v1/web/' + path, opts).then(function (res) {
      if (!res.ok) return jsonResult(res);
      return res.blob().then(function (b) {
        return { status: res.status, ok: true, body: {}, blob: b, disposition: res.headers.get('Content-Disposition') || '' };
      });
    }).then(null, networkError);
  }

  function errorText(r) {
    var msg = typeof r.body.message === 'string' ? r.body.message : 'Something went wrong. Please try again.';
    if (r.body.error === 'rate_limited' && r.retryAfter) {
      var min = Math.ceil(Number(r.retryAfter) / 60);
      if (min > 0) msg += ' (about ' + min + (min === 1 ? ' minute)' : ' minutes)');
    }
    return msg;
  }

  // ---- Turnstile ----------------------------------------------------------------------------
  var ts = { id: null, token: '' };
  function loadTurnstile(sitekey, action) {
    var box = $('[data-turnstile]');
    if (!box || !sitekey) return;
    var s = document.createElement('script');
    s.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
    s.async = true;
    document.head.appendChild(s);
    var tries = 0;
    (function wait() {
      if (window.turnstile && typeof window.turnstile.render === 'function') {
        ts.id = window.turnstile.render(box, {
          sitekey: sitekey,
          action: action,
          theme: 'auto',
          callback: function (t) {
            ts.token = t;
            if (statusEl.textContent === WAIT_TEXT) say('');
          },
          'expired-callback': function () { ts.token = ''; },
          'error-callback': function () { ts.token = ''; }
        });
      } else if (++tries < 150) {
        setTimeout(wait, 100);
      } else {
        say('The security check could not load. Reload the page to try again.', 'error');
      }
    })();
  }
  function tsToken() { return ts.token; }
  function tsReset() {
    ts.token = '';
    if (window.turnstile && ts.id !== null) window.turnstile.reset(ts.id);
  }
  var WAIT_TEXT = 'Please wait for the security check to finish, then try again.';
  function needToken() {
    if (tsToken()) return false;
    say(WAIT_TEXT, 'error');
    return true;
  }

  // While a request runs: the form's buttons are disabled and its submit button shows a spinner.
  function busy(form, on) {
    Array.prototype.forEach.call(form.querySelectorAll('button'), function (b) {
      b.disabled = on;
      if (b.type === 'submit') { if (on) b.setAttribute('aria-busy', 'true'); else b.removeAttribute('aria-busy'); }
    });
  }

  // ---- Inline validation ---------------------------------------------------------------------
  // A message under the field (aria-describedby), aria-invalid, and the focus on the first one.
  var fieldIds = 0;
  function fieldError(input, text) {
    var label = input.closest('label') || input.parentNode;
    var msg = label.querySelector('.field-error');
    if (!text) {
      input.removeAttribute('aria-invalid');
      if (msg) {
        label.removeChild(msg);
        var ids = (input.getAttribute('aria-describedby') || '').split(' ').filter(function (x) { return x && x !== msg.id; });
        if (ids.length) input.setAttribute('aria-describedby', ids.join(' ')); else input.removeAttribute('aria-describedby');
      }
      return;
    }
    if (!msg) {
      msg = el('span', 'field-error');
      msg.id = 'fe-' + (++fieldIds);
      label.appendChild(msg);
      var d = input.getAttribute('aria-describedby');
      input.setAttribute('aria-describedby', (d ? d + ' ' : '') + msg.id);
    }
    msg.textContent = text;
    input.setAttribute('aria-invalid', 'true');
  }
  function problem(input) {
    var v = input.value;
    if (input.type === 'checkbox') return input.required && !input.checked ? 'Tick this box to continue.' : '';
    if (input.required && !v.trim()) return 'Please fill this in.';
    if (!v) return '';
    if (input.type === 'email' && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v.trim())) return 'Enter an email address like name@example.org.';
    if (input.minLength > 0 && v.length < input.minLength) return 'Use at least ' + input.minLength + ' characters.';
    if (input.maxLength > 0 && v.length > input.maxLength) return 'Use at most ' + input.maxLength + ' characters.';
    if (input.pattern && !new RegExp('^(?:' + input.pattern + ')$').test(v.trim())) return input.getAttribute('data-pattern-error') || 'This doesn\'t look right.';
    return '';
  }
  // True when every visible field of the form is fine; otherwise the messages are shown.
  function validate(form, extra) {
    var first = null;
    Array.prototype.forEach.call(form.querySelectorAll('input'), function (input) {
      if (input.closest('[hidden]')) { fieldError(input, ''); return; }
      var p = problem(input) || (extra ? extra(input) : '');
      fieldError(input, p);
      if (p && !first) first = input;
    });
    if (first) first.focus();
    return !first;
  }
  document.addEventListener('input', function (ev) {
    var t = ev.target;
    if (t && t.getAttribute && t.getAttribute('aria-invalid') === 'true' && !problem(t)) fieldError(t, '');
  });

  // A form whose submit posts `build()` to `path`; `done(r)` on success, `fail(r)` otherwise.
  function onSubmit(form, path, build, done, fail) {
    form.addEventListener('submit', function (ev) {
      ev.preventDefault();
      if (!validate(form)) return;
      var body = build();
      if (!body) return;
      busy(form, true);
      say('');
      api('POST', path, body).then(function (r) {
        busy(form, false);
        if (r.ok) done(r); else (fail || function (x) { say(errorText(x), 'error'); })(r);
      });
    });
  }

  function samePasswords(form, a, b) {
    if (value(form, a) !== value(form, b)) {
      fieldError(field(form, b), 'The two passwords are different.');
      field(form, b).focus();
      return false;
    }
    return true;
  }


  // After signing in, go back to where the user came from: the device approval page, the
  // dashboard's "Add a device" or "Your data", or the delete page. Only these paths are accepted,
  // so the parameter can't send anyone elsewhere.
  function safeNext() {
    var n = new URLSearchParams(location.search).get('next');
    if (!n) return null;
    return /^\/link\?code=[0-9A-Za-z-]{1,12}$/.test(n) || /^\/account\/(#(add-device|export))?$/.test(n) ||
      n === '/account/delete' ? n : null;
  }
  // A device code as the server reads it (devices.rs normalize_user_code): Crockford symbols,
  // O→0, I/L→1, dashes and spaces dropped; 8 symbols are shown as XXXX-XXXX.
  function normCode(c) {
    var s = String(c || '').toUpperCase().replace(/[\s-]/g, '').replace(/O/g, '0').replace(/[IL]/g, '1')
      .replace(/[^0-9A-Z]/g, '').slice(0, 12);
    return s.length === 8 ? s.slice(0, 4) + '-' + s.slice(4) : s;
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  }
  function when(iso) {
    var d = new Date(iso);
    return isNaN(d) ? '' : d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
  }
  var regions = null;
  function country(code) {
    if (!code) return 'unknown';
    try {
      regions = regions || new Intl.DisplayNames(['en'], { type: 'region' });
      return regions.of(code) + ' (' + code + ')';
    } catch (e) {
      return code;
    }
  }
  // "Chrome on Windows" rather than the whole user agent string.
  function uaSummary(ua) {
    if (!ua) return 'A browser';
    var b = /Krunk\//.test(ua) ? 'Krunk' : /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox'
      : /(Chrome|Chromium)\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : null;
    var o = /Windows/.test(ua) ? 'Windows' : /Android/.test(ua) ? 'Android' : /(iPhone|iPad)/.test(ua) ? 'iOS'
      : /Mac OS X/.test(ua) ? 'macOS' : /Linux/.test(ua) ? 'Linux' : null;
    return b && o ? b + ' on ' + o : b || o || ua.slice(0, 60);
  }
  function platformName(p) {
    var names = { android: 'Android', windows: 'Windows', ios: 'iOS', macos: 'macOS', linux: 'Linux' };
    if (!p) return 'Windows';
    return names[String(p).toLowerCase()] || String(p);
  }
  // A browser name from the server: a user agent string is summarised, a name is kept.
  function browserName(b) { return !b ? 'A browser' : /\//.test(b) ? uaSummary(b) : String(b); }

  // Sensitive actions answer 403 reauth_required unless the password was entered in the last
  // 10 minutes. Then this form asks for it (and a code, if two-step verification is on) and the
  // action runs again. `anchor`: the form is moved right after it, next to the action (never
  // inside another form: its submit would bubble there). `fail(r)`, if given, gets the other
  // errors instead of the banner.
  // web/me says whether the last re-auth is still fresh, so the form usually comes up before the
  // action is tried (no 403 on the way); a 403 reauth_required (a race) brings it up too.
  function withReauth(form, totpOn, action, done, anchor, fail) {
    function failed(r) { if (fail) fail(r); else say(errorText(r), 'error'); }
    api('GET', 'me').then(function (m) {
      if (m.ok && m.body.reauth_valid === false) { ask(); return; }
      action().then(function (r) {
        if (r.ok) { done(r); return; }
        if (r.body.error !== 'reauth_required') { failed(r); return; }
        ask();
      });
    });
    function ask() {
      form.reset();
      Array.prototype.forEach.call(form.querySelectorAll('input'), function (i) { fieldError(i, ''); });
      if (anchor) anchor.insertAdjacentElement('afterend', form);
      show(form.querySelector('.needs-totp'), totpOn);
      show(form, true);
      field(form, 'password').focus();
      form.onsubmit = function (ev) {
        ev.preventDefault();
        if (!validate(form)) return;
        var b = { password: value(form, 'password') };
        if (value(form, 'totp')) b.totp = value(form, 'totp').trim();
        busy(form, true);
        api('POST', 'reauth', b).then(function (x) {
          busy(form, false);
          if (x.status === 401 && (x.body.error === 'totp_required' || x.body.error === 'totp_invalid')) {
            show(form.querySelector('.needs-totp'), true);
          }
          if (!x.ok) { say(errorText(x), 'error'); return; }
          show(form, false);
          say('');
          action().then(function (r2) { if (r2.ok) done(r2); else failed(r2); });
        });
      };
      form.querySelector('.cancel').onclick = function () {
        show(form, false);
        say('');
        if (anchor && anchor.focus) anchor.focus();
      };
    }
  }

  // ---- Pages --------------------------------------------------------------------------------
  var pages = {};

  pages.login = function (cfg) {
    var form = $('#form');
    show(form, true);
    loadTurnstile(cfg.turnstile_site_key, 'login');
    onSubmit(form, 'login', function () {
      if (needToken()) return null;
      var b = { email: value(form, 'email'), password: value(form, 'password'), turnstile: tsToken() };
      if (value(form, 'totp')) b.totp = value(form, 'totp').trim();
      return b;
    }, function () {
      location.href = safeNext() || '/account/';
    }, function (r) {
      tsReset();
      if (r.body.error === 'totp_required') {
        show($('#totp-row'), true);
        field(form, 'totp').focus();
      }
      say(errorText(r), 'error');
    });
  };

  pages.register = function (cfg) {
    var form = $('#form');
    show(form, true);
    loadTurnstile(cfg.turnstile_site_key, 'register');
    onSubmit(form, 'register', function () {
      if (!samePasswords(form, 'password', 'password2') || needToken()) return null;
      return { email: value(form, 'email'), password: value(form, 'password'), turnstile: tsToken() };
    }, function () {
      show(form, false);
      show($('#done'), true);
    }, function (r) {
      tsReset();
      say(errorText(r), 'error');
    });
  };

  // Links carry the token in the query; it is taken out of the address bar at once.
  function takeToken() {
    var t = new URLSearchParams(location.search).get('token');
    if (t) history.replaceState(null, '', location.pathname);
    return t && /^[A-Za-z0-9_-]{43}$/.test(t) ? t : null;
  }

  pages.verify = function () {
    var token = takeToken();
    var form = $('#form');
    if (!token) { say('This link is incomplete. Open it again from the email, or request a new one.', 'error'); return; }
    show(form, true);
    onSubmit(form, 'verify-email', function () {
      return { token: token, password: value(form, 'password') };
    }, function () {
      show(form, false);
      show($('#done'), true);
    }, function (r) {
      if (r.status === 400 || r.status === 410) show(form, false);
      say(errorText(r), 'error');
    });
  };

  pages.reset = function (cfg) {
    var token = takeToken();
    var done = $('#done');
    if (token) {
      var nf = $('#form-new');
      show(nf, true);
      onSubmit(nf, 'password-reset/confirm', function () {
        if (!samePasswords(nf, 'new_password', 'new_password2')) return null;
        var b = { token: token, new_password: value(nf, 'new_password') };
        if (value(nf, 'totp')) b.totp = value(nf, 'totp').trim();
        return b;
      }, function () {
        show(nf, false);
        $('#done-text').textContent = 'Your password is changed. Sign in with the new one.';
        show(done, true);
      }, function (r) {
        if (r.body.error === 'totp_required' || r.body.error === 'totp_invalid') show($('#totp-row'), true);
        if (r.status === 410 || r.body.error === 'invalid_link') show(nf, false);
        say(errorText(r), 'error');
      });
      return;
    }
    var form = $('#form');
    show(form, true);
    loadTurnstile(cfg.turnstile_site_key, 'reset');
    onSubmit(form, 'password-reset/request', function () {
      if (needToken()) return null;
      return { email: value(form, 'email'), turnstile: tsToken() };
    }, function () {
      show(form, false);
      $('#done-text').textContent = 'Check your email. If the address belongs to an account, a link is on its way. It works for 1 hour.';
      show(done, true);
    }, function (r) {
      tsReset();
      say(errorText(r), 'error');
    });
  };

  pages.resend = function (cfg) {
    var form = $('#form');
    show(form, true);
    loadTurnstile(cfg.turnstile_site_key, 'resend');
    onSubmit(form, 'resend-verification', function () {
      if (needToken()) return null;
      return { email: value(form, 'email'), turnstile: tsToken() };
    }, function () {
      show(form, false);
      show($('#done'), true);
    }, function (r) {
      tsReset();
      say(errorText(r), 'error');
    });
  };

  // The device approval page (PROTOCOL §6.1): /link?code=XXXX-XXXX from Krunk. The code is
  // taken out of the address bar; the user checks the device and confirms the code.
  pages.link = function () {
    var params = new URLSearchParams(location.search);
    var raw = params.get('code') || '';
    var code = normCode(raw);
    if (raw) history.replaceState(null, '', location.pathname);
    var codeForm = $('#code-form');
    var form = $('#approve-form');
    var me = null;
    // The code form, filled in with the code (normalised) so it can be checked and sent again.
    function askCode(c) {
      codeForm.reset();
      if (c) field(codeForm, 'code').value = normCode(c);
      show(codeForm, true);
    }
    api('GET', 'me').then(function (r) {
      if (r.status === 401) {
        // The normalised code always passes safeNext, so it survives the sign-in.
        location.href = '/account/login?next=' + encodeURIComponent('/link' + (code ? '?code=' + code : ''));
        return;
      }
      if (!r.ok) { say(errorText(r), 'error'); return; }
      me = r.body;
      csrf = me.csrf;
      if (code) lookup(code); else askCode(raw);
    });
    codeForm.addEventListener('submit', function (ev) {
      ev.preventDefault();
      if (!validate(codeForm)) return;
      var c = value(codeForm, 'code').trim();
      if (c) lookup(c);
    });
    function lookup(c) {
      say('');
      api('GET', 'link/' + encodeURIComponent(c)).then(function (r) {
        if (!r.ok) {
          say(r.status === 404 ? 'This code is not valid or has expired. Check it below, or start again in Krunk to get a new one.' : errorText(r), 'error');
          askCode(c);
          show(form, false);
          return;
        }
        var g = r.body;
        code = g.user_code;
        show(codeForm, false);
        $('#d-label').textContent = g.label;
        $('#d-platform').textContent = platformName(g.platform);
        $('#d-country').textContent = country(g.country);
        var mins = Math.max(0, Math.round(g.age_s / 60));
        $('#d-age').textContent = mins < 1 ? 'just now' : mins + (mins === 1 ? ' minute ago' : ' minutes ago');
        var mm = $('#d-mismatch');
        if (g.country && g.your_country && g.country !== g.your_country) {
          mm.textContent = 'This device is in ' + country(g.country) + ', but you are in ' + country(g.your_country) + '. Deny unless you are sure it is yours.';
          show(mm, true);
        } else {
          show(mm, false);
        }
        $('#d-code').textContent = g.user_code;
        field(form, 'same').checked = false;
        form.querySelector('button[type=submit]').disabled = true;
        show(form, true);
      });
    }
    field(form, 'same').addEventListener('change', function () {
      form.querySelector('button[type=submit]').disabled = !field(form, 'same').checked;
    });
    function finished(text) {
      show(form, false);
      show($('#reauth'), false);
      $('#done-text').textContent = text;
      show($('#done'), true);
    }
    form.addEventListener('submit', function (ev) {
      ev.preventDefault();
      if (!field(form, 'same').checked) return;
      busy(form, true);
      withReauth($('#reauth'), me && me.totp, function () {
        return api('POST', 'link/' + encodeURIComponent(code) + '/approve', { confirm_code: code });
      }, function () {
        finished('Approved. Krunk on your device is signed in within a few seconds.');
      });
      busy(form, false);
    });
    $('#deny').addEventListener('click', function () {
      api('POST', 'link/' + encodeURIComponent(code) + '/deny', {}).then(function (r) {
        if (r.ok) finished('Denied. The device was not signed in.');
        else say(errorText(r), 'error');
      });
    });
  };

  // The QR code is an SVG made by the server; it is parsed as SVG (never as HTML) and only an
  // <svg> root of plain shapes is accepted: no script, foreignObject, links or event attributes.
  function showQr(box, svgText) {
    box.textContent = '';
    if (typeof svgText !== 'string') return false;
    var doc = new DOMParser().parseFromString(svgText, 'image/svg+xml');
    var root = doc.documentElement;
    if (!root || root.nodeName !== 'svg' || doc.getElementsByTagName('script').length ||
        doc.getElementsByTagName('foreignObject').length) return false;
    var all = [root].concat(Array.prototype.slice.call(root.getElementsByTagName('*')));
    var bad = all.some(function (n) {
      return Array.prototype.some.call(n.attributes, function (a) { return /^on/i.test(a.name) || /href$/i.test(a.name); });
    });
    if (bad) return false;
    box.appendChild(document.importNode(root, true));
    return true;
  }

  // ---- Dashboard helpers --------------------------------------------------------------------
  var SVG = 'http://www.w3.org/2000/svg';
  // Platform icons, built as SVG elements (presentation attributes only: no inline style).
  var ICONS = {
    windows: [['path', { d: 'M3 5.6 10 4.6v7.1H3zM11 4.4 21 3v8.7H11zM3 12.7h7v7.1l-7-1zM11 12.7h10V21l-10-1.4z', fill: 'currentColor' }]],
    android: [['path', { d: 'M7 10h10v7.2a1 1 0 0 1-1 1h-1V21h-2v-2.8h-2V21H9v-2.8H8a1 1 0 0 1-1-1zM4.2 10h1.8v5.8H4.2zM18 10h1.8v5.8H18zM7 9a5 5 0 0 1 10 0zM9.7 6.6h.01M14.3 6.6h.01', fill: 'currentColor', stroke: 'currentColor', 'stroke-linecap': 'round' }]],
    browser: [['circle', { cx: '12', cy: '12', r: '9', fill: 'none', stroke: 'currentColor', 'stroke-width': '2' }],
      ['path', { d: 'M3 12h18M12 3c2.6 2.6 3.8 5.6 3.8 9s-1.2 6.4-3.8 9M12 3C9.4 5.6 8.2 8.6 8.2 12s1.2 6.4 3.8 9', fill: 'none', stroke: 'currentColor', 'stroke-width': '2' }]],
    other: [['rect', { x: '3', y: '4', width: '18', height: '12', rx: '2', fill: 'none', stroke: 'currentColor', 'stroke-width': '2' }],
      ['path', { d: 'M8 20h8M12 16v4', fill: 'none', stroke: 'currentColor', 'stroke-width': '2' }]]
  };
  function icon(kind) {
    var box = el('span', 'icon');
    box.setAttribute('aria-hidden', 'true');
    var svg = document.createElementNS(SVG, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('focusable', 'false');
    (ICONS[kind] || ICONS.other).forEach(function (part) {
      var n = document.createElementNS(SVG, part[0]);
      Object.keys(part[1]).forEach(function (k) { n.setAttribute(k, part[1][k]); });
      svg.appendChild(n);
    });
    box.appendChild(svg);
    return box;
  }
  // "3 minutes ago"; the exact time is in the element's title.
  function ago(iso) {
    var d = new Date(iso);
    if (isNaN(d)) return '';
    var s = (Date.now() - d.getTime()) / 1000;
    if (s < 90) return 'just now';
    if (s < 3600) return Math.round(s / 60) + ' minutes ago';
    if (s < 86400) { var h = Math.round(s / 3600); return h + (h === 1 ? ' hour ago' : ' hours ago'); }
    if (s < 2 * 86400) return 'yesterday';
    if (s < 7 * 86400) return Math.floor(s / 86400) + ' days ago';
    return 'on ' + d.toLocaleDateString(undefined, { dateStyle: 'medium' });
  }
  // A <time> with the relative text and the exact time in its title and datetime.
  function timeSpan(prefix, iso) {
    var t = el('time', null, prefix + ago(iso));
    t.title = when(iso);
    if (!isNaN(new Date(iso))) t.dateTime = new Date(iso).toISOString();
    return t;
  }
  // "12 s ago", "3 min ago" for the last hour, then as ago().
  function secsAgo(s) {
    s = Math.max(0, Math.round(s));
    if (s < 60) return s + ' s ago';
    if (s < 3600) return Math.round(s / 60) + ' min ago';
    return ago(new Date(Date.now() - s * 1000).toISOString());
  }
  function mmss(s) {
    s = Math.max(0, Math.round(s));
    var m = Math.floor(s / 60), r = s % 60;
    return m + ':' + (r < 10 ? '0' : '') + r;
  }
  function percent(v, max) {
    var p = max > 0 ? (Number(v) || 0) / max * 100 : 0;
    return p > 0 && p < 1 ? 'under 1%' : Math.round(p) + '%';
  }
  function size(bytes) {
    var n = Number(bytes) || 0;
    if (n < 1024) return n + ' B';
    if (n < 1024 * 1024) return (n / 1024).toFixed(n < 10240 ? 1 : 0) + ' KB';
    return (n / 1024 / 1024).toFixed(1) + ' MB';
  }
  function count(n) { return (Number(n) || 0).toLocaleString('en'); }
  // A row of a devices or sessions list: icon, title (name + badges), meta, actions.
  function row(cls, iconKind, name, badges) {
    var li = el('li', cls || null);
    li.appendChild(icon(iconKind));
    var title = el('div', 'title');
    var n = el('span', 'name', name);
    n.title = name;
    title.appendChild(n);
    (badges || []).forEach(function (b) { title.appendChild(el('span', 'badge ' + b[0], b[1])); });
    li.appendChild(title);
    return li;
  }
  function meta(parts) {
    var m = el('p', 'meta');
    parts.forEach(function (p, i) {
      if (i) m.appendChild(document.createTextNode(' · '));
      m.appendChild(typeof p === 'string' ? document.createTextNode(p) : p);
    });
    return m;
  }
  function button(text, cls) {
    var b = el('button', cls || 'small', text);
    b.type = 'button';
    return b;
  }
  // The sync state from web/me (absent before the 0.9.0 server: then sync is off).
  function syncView(me) {
    var s = me.sync;
    if (s && s.rotation_pending) {
      return { cls: 'pending', title: 'Securing your account…', text: 'A device was removed. Your other Krunk browsers re-encrypt your synced data with new keys; this finishes when one of them is online.', tile: 'Securing', tileD: 'after a device was removed', on: true };
    }
    if (s && s.has_keyset) {
      return { cls: '', title: 'Sync is on', text: count(s.records) + ' encrypted items, ' + size(s.bytes) + '. Only your devices can read them: this website stores and sees encrypted data only.', tile: 'On', tileD: count(s.records) + ' items · ' + size(s.bytes), on: true };
    }
    return { cls: '', title: 'Sync is off — turn it on in Krunk (Settings → Sync).', text: 'Until then a device only shows who it is signed in as.', tile: 'Off', tileD: 'Turn it on in Krunk (Settings → Sync)', on: false };
  }
  function copyText(text, what) {
    function done(ok) { say(ok ? what + ' copied to the clipboard.' : 'Copying failed: select the text and copy it yourself.', ok ? 'ok' : 'error'); }
    function fallback() {
      var ta = el('textarea', 'offscreen');
      ta.value = text;
      ta.setAttribute('readonly', '');
      document.body.appendChild(ta);
      ta.select();
      var ok = false;
      try { ok = document.execCommand('copy'); } catch (e) { ok = false; }
      document.body.removeChild(ta);
      done(ok);
    }
    if (navigator.clipboard && window.isSecureContext) navigator.clipboard.writeText(text).then(function () { done(true); }, fallback);
    else fallback();
  }
  function download(name, data) {
    var url = URL.createObjectURL(data instanceof Blob ? data : new Blob([data], { type: 'text/plain;charset=utf-8' }));
    var a = el('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(function () { URL.revokeObjectURL(url); }, 10000);
  }
  // The section nav marks the section in view (aria-current) as the page scrolls.
  function sectionNav() {
    var links = Array.prototype.slice.call(document.querySelectorAll('#secnav a'));
    function mark(id) {
      links.forEach(function (a) {
        if (a.getAttribute('href') === '#' + id) a.setAttribute('aria-current', 'true'); else a.removeAttribute('aria-current');
      });
    }
    links.forEach(function (a) { a.addEventListener('click', function () { mark(a.getAttribute('href').slice(1)); }); });
    mark('profile');
    if (!('IntersectionObserver' in window)) return;
    var visible = {};
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) { visible[e.target.id] = e.isIntersecting; });
      var first = links.map(function (a) { return a.getAttribute('href').slice(1); }).filter(function (id) { return visible[id]; })[0];
      if (first) mark(first);
    }, { rootMargin: '-10% 0px -60% 0px' });
    links.forEach(function (a) { var t = document.getElementById(a.getAttribute('href').slice(1)); if (t) io.observe(t); });
  }

  pages.index = function () {
    var overview = $('#overview');
    var meCache = null;
    var reauthForm = $('#reauth');
    statusEl.tabIndex = -1;
    // /account/#add-device (Krunk's "Add a device") and /account/#export open those parts; kept
    // through the sign-in (safeNext).
    var startHash = /^#(add-device|export)$/.test(location.hash) ? location.hash : '';
    // Krunk's "Manage account" opens /account/?this=<device id>: that row says "This device".
    var params = new URLSearchParams(location.search);
    var thisDevice = params.get('this');
    if (thisDevice && /^[0-9a-f-]{36}$/.test(thisDevice)) {
      try { sessionStorage.setItem('krunk-this-device', thisDevice); } catch (e) { /* storage off */ }
    }
    if (params.has('this')) history.replaceState(null, '', location.pathname + location.hash);
    try { thisDevice = sessionStorage.getItem('krunk-this-device'); } catch (e) { thisDevice = null; }

    function afterAction(focusEl) {
      if (focusEl && document.body.contains(focusEl) && !focusEl.closest('[hidden]')) focusEl.focus();
      else statusEl.focus();
    }

    function loadDevices() {
      return api('GET', 'devices').then(function (r) {
        if (!r.ok) return;
        var active = $('#devices-active');
        var removedBox = $('#devices-removed');
        var removed = removedBox.querySelector('ul');
        active.textContent = '';
        removed.textContent = '';
        var syncOn = meCache && syncView(meCache).on;
        var nActive = 0, nRemoved = 0;
        r.body.forEach(function (d) {
          var badges = [];
          if (!d.revoked_at && d.id === thisDevice) badges.push(['self', 'This device']);
          if (!d.revoked_at && syncOn && d.has_keys === false) badges.push(['warn', 'Linked, no keys yet']);
          var li = row(d.revoked_at ? 'revoked' : '', d.platform === 'android' ? 'android' : 'windows', d.label, badges);
          var parts = [platformName(d.platform), 'version ' + d.client_version];
          if (d.revoked_at) {
            parts.push(timeSpan('removed ', d.revoked_at));
          } else {
            parts.push(d.last_sync ? timeSpan('last synced ', d.last_sync) : timeSpan('last seen ', d.last_used));
            if (d.last_ip) parts.push(d.last_ip);
            parts.push(country(d.last_country));
          }
          li.appendChild(meta(parts));
          if (d.revoked_at) {
            removed.appendChild(li);
            nRemoved++;
            return;
          }
          nActive++;
          var actions = el('div', 'actions');
          var rename = button('Rename');
          var remove = button('Remove', 'small danger');
          rename.setAttribute('aria-label', 'Rename ' + d.label);
          remove.setAttribute('aria-label', 'Remove ' + d.label);
          actions.appendChild(rename);
          actions.appendChild(remove);
          li.appendChild(actions);
          rename.addEventListener('click', function () { renameForm(li, d, rename); });
          remove.addEventListener('click', function () {
            reauthForm.querySelector('.why').textContent = 'Enter your password again to remove "' + d.label + '". It is signed out at once' +
              (syncOn ? ', and your other devices re-encrypt your synced data with new keys.' : '.');
            withReauth(reauthForm, meCache && meCache.totp, function () {
              return api('POST', 'devices/' + d.id + '/revoke', {});
            }, function () {
              say('"' + d.label + '" is removed and signed out.', 'ok');
              Promise.all([loadMe(), loadDevices(), loadSessions()]).then(function () { afterAction($('#devices-removed summary')); });
            }, li);
          });
          active.appendChild(li);
        });
        show($('#no-devices'), nActive === 0);
        $('#removed-count').textContent = String(nRemoved);
        show(removedBox, nRemoved > 0);
      });
    }
    function renameForm(li, d, opener) {
      if (li.querySelector('form')) { li.querySelector('form input').focus(); return; }
      var f = el('form');
      f.noValidate = true;
      var label = el('label');
      label.appendChild(el('span', 'sr-label', 'New name'));
      var input = el('input');
      input.name = 'label';
      input.required = true;
      input.maxLength = 64;
      input.value = d.label;
      input.autocomplete = 'off';
      label.appendChild(input);
      var save = el('button', 'small primary', 'Save');
      save.type = 'submit';
      var cancel = button('Cancel');
      f.appendChild(label);
      f.appendChild(save);
      f.appendChild(cancel);
      li.appendChild(f);
      input.focus();
      input.select();
      cancel.addEventListener('click', function () { li.removeChild(f); opener.focus(); });
      f.addEventListener('submit', function (ev) {
        ev.preventDefault();
        if (!validate(f, function (i) { return /[\u0000-\u001f\u007f]/.test(i.value) ? 'No line breaks or control characters.' : ''; })) return;
        busy(f, true);
        api('POST', 'devices/' + d.id + '/rename', { label: input.value }).then(function (r) {
          busy(f, false);
          if (!r.ok) { fieldError(input, errorText(r)); input.focus(); return; }
          say('Renamed to "' + r.body.label + '".', 'ok');
          loadDevices().then(function () { afterAction(null); });
        });
      });
    }
    function loadSessions() {
      return api('GET', 'sessions').then(function (r) {
        if (!r.ok) return;
        var list = $('#sessions');
        list.textContent = '';
        var others = 0;
        r.body.filter(function (x) { return x.kind === 'web'; }).forEach(function (x) {
          var badges = x.current ? [['self', 'This browser']] : [];
          if (x.via === 'qr') badges.push(['info', 'Signed in by QR']);
          var li = row('', 'browser', uaSummary(x.user_agent), badges);
          // `ip` since the 0.13.0 server (the owner sees the full address); `ip_masked` before.
          li.appendChild(meta([timeSpan('signed in ', x.created),
            x.current ? 'active now' : timeSpan('last seen ', x.last_used),
            x.ip || x.ip_masked || 'address not kept', country(x.country)]));
          if (!x.current) {
            others++;
            var actions = el('div', 'actions');
            var b = button('Sign out');
            b.setAttribute('aria-label', 'Sign out ' + uaSummary(x.user_agent));
            actions.appendChild(b);
            li.appendChild(actions);
            b.addEventListener('click', function () {
              b.disabled = true;
              b.setAttribute('aria-busy', 'true');
              api('POST', 'sessions/' + x.id + '/revoke', {}).then(function (rr) {
                if (rr.ok) { say('That browser is signed out.', 'ok'); loadSessions().then(function () { afterAction(null); }); }
                else { b.disabled = false; b.removeAttribute('aria-busy'); say(errorText(rr), 'error'); }
              });
            });
          }
          list.appendChild(li);
        });
        $('#revoke-all-open').disabled = others === 0 && !(meCache && meCache.devices > 0);
      });
    }
    function loadMe() {
      return api('GET', 'me').then(function (r) {
        if (r.status === 401) {
          location.href = '/account/login' + (startHash ? '?next=' + encodeURIComponent('/account/' + startHash) : '');
          return;
        }
        if (!r.ok) { say(errorText(r), 'error'); return; }
        var me = r.body;
        meCache = me;
        csrf = me.csrf;
        $('#me-email').textContent = me.email;
        var who = $('#who');
        who.textContent = me.email;
        who.title = me.email;
        show(who, true);
        show($('#logout'), true);
        $('#me-created').textContent = new Date(me.created).toLocaleDateString(undefined, { dateStyle: 'long' });
        $('#tile-devices').textContent = String(me.devices);
        $('#tile-devices-d').textContent = me.devices === 1 ? 'Krunk browser signed in' : 'Krunk browsers signed in';
        var sv = syncView(me);
        $('#tile-sync').textContent = sv.tile;
        $('#tile-sync-d').textContent = sv.tileD;
        $('#sync-state').className = 'sync-state' + (sv.cls ? ' ' + sv.cls : '');
        $('#sync-title').textContent = sv.title;
        $('#sync-text').textContent = sv.text;
        $('#tile-totp').textContent = me.totp ? 'On' : 'Off';
        $('#tile-totp-d').textContent = me.totp ? me.backup_codes_left + ' backup codes left' : 'Recommended';
        var badge = $('#totp-badge');
        badge.textContent = me.totp ? 'On' : 'Off';
        badge.className = 'badge ' + (me.totp ? 'ok' : 'off');
        $('#totp-state').textContent = me.totp
          ? 'Signing in needs a code from your authenticator app. Backup codes left: ' + me.backup_codes_left + '.'
          : 'Turn it on to sign in with a code from an authenticator app as well as your password.';
        show($('#totp-start'), !me.totp);
        show($('#totp-off'), me.totp);
        show($('#totp-confirm'), false);
        show($('#password .needs-totp'), me.totp);
        show(overview, true);
        show($('#secnav'), true);
      });
    }

    $('#logout').addEventListener('click', function () {
      api('POST', 'logout', {}).then(function () { location.href = '/account/login'; });
    });

    var openAll = $('#revoke-all-open');
    var allForm = $('#revoke-all');
    openAll.addEventListener('click', function () {
      var open = allForm.hidden;
      show(allForm, open);
      openAll.setAttribute('aria-expanded', open ? 'true' : 'false');
      if (open) allForm.querySelector('button[type=submit]').focus();
    });
    allForm.querySelector('.cancel').addEventListener('click', function () {
      show(allForm, false);
      openAll.setAttribute('aria-expanded', 'false');
      openAll.focus();
    });
    allForm.addEventListener('submit', function (ev) {
      ev.preventDefault();
      var devices = field(allForm, 'devices').checked;
      reauthForm.querySelector('.why').textContent = 'Enter your password again to sign out everywhere else.';
      withReauth(reauthForm, meCache && meCache.totp, function () {
        return api('POST', 'sessions/revoke-all', { include_devices: devices });
      }, function () {
        show(allForm, false);
        openAll.setAttribute('aria-expanded', 'false');
        allForm.reset();
        say(devices ? 'Every other browser is signed out, and your Krunk browsers are removed.' : 'Every other browser is signed out.', 'ok');
        Promise.all([loadMe(), loadDevices(), loadSessions()]).then(function () { afterAction(null); });
      }, allForm);
    });

    var start = $('#totp-start');
    onSubmit(start, 'totp/setup', function () {
      return { password: value(start, 'password') };
    }, function (r) {
      start.reset();
      show(start, false);
      // The key in groups of 4 for reading; copied without spaces.
      $('#totp-secret').textContent = String(r.body.secret_b32).replace(/(.{4})(?=.)/g, '$1 ');
      $('#totp-secret').setAttribute('data-key', r.body.secret_b32);
      showQr($('#totp-qr'), r.body.qr_svg);
      show($('#totp-confirm'), true);
      field($('#totp-confirm'), 'code').focus();
    });
    $('#totp-copy').addEventListener('click', function () {
      copyText($('#totp-secret').getAttribute('data-key') || '', 'The key');
    });

    var confirmForm = $('#totp-confirm');
    var codes = [];
    onSubmit(confirmForm, 'totp/enable', function () {
      return { code: value(confirmForm, 'code').trim() };
    }, function (r) {
      confirmForm.reset();
      var list = $('#totp-code-list');
      list.textContent = '';
      codes = r.body.backup_codes || [];
      codes.forEach(function (c) { list.appendChild(el('li', null, c)); });
      loadMe().then(function () {
        show($('#totp-codes'), true);
        say('Two-step verification is on. Save your backup codes now.', 'ok');
        $('#codes-copy').focus();
      });
    });
    function codesText() {
      return 'Krunk backup codes for ' + (meCache ? meCache.email : '') + '\n' +
        'Made ' + new Date().toLocaleString() + '. Each code works once.\n\n' + codes.join('\n') + '\n';
    }
    $('#codes-copy').addEventListener('click', function () { copyText(codesText(), 'The backup codes'); });
    $('#codes-download').addEventListener('click', function () {
      download('krunk-backup-codes.txt', codesText());
      say('Saved as krunk-backup-codes.txt.', 'ok');
    });

    var off = $('#totp-off');
    onSubmit(off, 'totp/disable', function () {
      return { password: value(off, 'password'), code: value(off, 'code').trim() };
    }, function () {
      off.reset();
      show($('#totp-codes'), false);
      loadMe().then(function () { say('Two-step verification is off.', 'ok'); afterAction(null); });
    });

    var pw = $('#password');
    onSubmit(pw, 'password/change', function () {
      if (!samePasswords(pw, 'new', 'new2')) return null;
      var b = { current: value(pw, 'current'), new: value(pw, 'new') };
      if (value(pw, 'totp')) b.totp = value(pw, 'totp').trim();
      return b;
    }, function () {
      pw.reset();
      say('Your password is changed. Other places where you were signed in are signed out.', 'ok');
      loadSessions();
    });

    // ---- Add a device: a QR code for a phone (PROTOCOL, pairing) ------------------------------
    // web/pair/start makes the code (one per account; a new one voids the old); the phone claims
    // it and shows a 6-digit code; this page polls web/pair every 2 s while it is open and
    // visible, shows who claimed it and the same code, and approves or denies.
    var pair = { open: false, live: false, id: null, state: null, code: null, claimant: null,
      deadline: 0, claimedAt: 0, poll: null, tick: null, announced: false };
    var pairPanel = $('#pair-panel');
    var pairOpener = $('#add-device-open');
    var pairForm = $('#pair-claimed');
    var pairSame = $('#pair-same');
    var pairApprove = $('#pair-approve');
    function pairMsg(text) { $('#pair-msg').textContent = text || ''; }
    // Which part shows: 'wait' (the QR), 'claimed' (approve or deny), 'result' or 'none'.
    function pairView(v) {
      show($('#pair-wait'), v === 'wait');
      show(pairForm, v === 'claimed');
      show($('#pair-result'), v === 'result');
      show($('#pair-timer'), v === 'wait' || v === 'claimed');
      show($('#pair-new'), v !== 'claimed');
    }
    function pairFocus(target) {
      // Only when the user is in the panel (or nowhere): a phone's claim shouldn't pull focus away.
      var a = document.activeElement;
      if (!a || a === document.body || pairPanel.contains(a)) target.focus();
    }
    function setPairOpen(on) {
      pair.open = on;
      show(pairPanel, on);
      show(pairOpener, !on);
      pairOpener.setAttribute('aria-expanded', on ? 'true' : 'false');
    }
    function pairStopPoll() { if (pair.poll) { clearTimeout(pair.poll); pair.poll = null; } }
    function pairSchedule(ms) { pairStopPoll(); if (pair.live) pair.poll = setTimeout(pairPoll, ms); }
    function pairPoll() {
      pair.poll = null;
      if (!pair.open || !pair.live || document.hidden) return; // resumed by visibilitychange
      var id = pair.id;
      api('GET', 'pair').then(function (r) {
        if (!pair.live || pair.id !== id) return;
        if (r.status === 401) { location.href = '/account/login?next=' + encodeURIComponent('/account/#add-device'); return; }
        if (r.ok) pairState(r.body);
        pairSchedule(r.body.error === 'rate_limited' ? 10000 : 2000);
      });
    }
    function pairTick() {
      if (!pair.open) return;
      var left = (pair.deadline - Date.now()) / 1000;
      if (pair.live) {
        $('#pair-countdown').textContent = left > 0 ? 'Expires in ' + mmss(left) : 'Expired';
        if (left <= 60 && left > 0 && !pair.announced) { pair.announced = true; $('#pair-announce').textContent = 'One minute left.'; }
        // The server says "expired" first; this is only for a lost connection.
        if (left < -6) pairEnd('This code has expired. Make a new one to try again.', 'error');
      }
      var age = $('#pair-age');
      if (age && pair.claimedAt) {
        var iso = new Date(pair.claimedAt).toISOString();
        age.textContent = secsAgo((Date.now() - pair.claimedAt) / 1000);
        age.dateTime = iso;
        age.title = when(iso);
      }
    }
    function pairLive(b) {
      pair.live = true;
      pair.id = b.pair_id;
      pair.state = 'waiting';
      pair.code = null;
      pair.claimant = null;
      pair.claimedAt = 0;
      pair.announced = false;
      $('#pair-announce').textContent = '';
      pair.deadline = Date.now() + (Number(b.expires_in) || 300) * 1000;
      if (!pair.tick) pair.tick = setInterval(pairTick, 1000);
      pairTick();
      pairSchedule(2000);
    }
    function pairStart() {
      reauthForm.querySelector('.why').textContent = 'Enter your password again to add a device.';
      var anchor = pair.open ? $('#pair-msg') : pairOpener;
      withReauth(reauthForm, meCache && meCache.totp, function () {
        return api('POST', 'pair/start', {});
      }, function (r) {
        var b = r.body;
        setPairOpen(true);
        showQr($('#pair-qr'), b.qr_svg);
        var url = $('#pair-url');
        url.textContent = b.url || '';
        $('#pair-title').textContent = 'Scan this code with your phone';
        pairMsg('');
        pairView('wait');
        pairLive(b);
        $('#pair-title').focus();
      }, anchor, pairFailed);
    }
    // Server states (web/pair): none, waiting, claimed, approved, denied, expired.
    function pairState(b) {
      if (b.pair_id && pair.id && b.pair_id !== pair.id) {
        pairEnd('A newer code was made in another tab or window. Use that one, or make a new code here.', '');
        return;
      }
      if (typeof b.expires_in === 'number' && (b.state === 'waiting' || b.state === 'claimed')) {
        pair.deadline = Date.now() + b.expires_in * 1000;
      }
      if (b.state === 'waiting') return;
      if (b.state === 'claimed') { pairClaimed(b); return; }
      if (b.state === 'approved') { pairApproved(); return; }
      if (b.state === 'denied') { pairEnd('Denied. Nothing was signed in.', ''); return; }
      if (b.state === 'expired') { pairEnd('This code has expired. Make a new one to try again.', 'error'); return; }
      pairEnd('This code is no longer active. Make a new one to try again.', 'error');
    }
    function pairClaimed(b) {
      var c = b.claimant || {};
      if (pair.state === 'claimed' && pair.code === b.confirm_code) return;
      pair.state = 'claimed';
      pair.code = b.confirm_code;
      pair.claimant = c;
      pair.claimedAt = Date.now() - (Number(c.age_s) || 0) * 1000;
      var dl = $('#pair-claimant');
      dl.textContent = '';
      function add(k, v) {
        if (v === null || v === undefined || v === '') return;
        dl.appendChild(el('dt', null, k));
        var dd = el('dd');
        if (typeof v === 'string') dd.textContent = v; else dd.appendChild(v);
        dl.appendChild(dd);
      }
      var device = c.kind === 'device';
      add('What', device ? 'The Krunk app on ' + platformName(c.platform) : 'A web browser: approving signs it in to this website');
      if (device) add('Device', c.label);
      else add('Browser', browserName(c.browser || c.label));
      if (!device && c.platform) add('System', platformName(c.platform));
      if (c.client_version) add('Version', String(c.client_version));
      add('IP address', c.ip || 'unknown');
      add('Location', country(c.country));
      var age = el('time');
      age.id = 'pair-age';
      add('Asked', age);
      $('#pair-code').textContent = b.confirm_code || '';
      pairSame.checked = false;
      pairApprove.disabled = true;
      $('#pair-title').textContent = 'Check the code';
      pairMsg('');
      $('#pair-announce').textContent = 'Someone scanned the code. Check that your phone shows the same code before you approve.';
      pairView('claimed');
      pairTick();
      pairFocus($('#pair-claimed-title'));
    }
    function pairApproved() {
      var c = pair.claimant || {};
      pairEnd(c.kind === 'web'
        ? 'Approved. That browser is now signed in to this website.'
        : 'Added. ' + (c.label ? '"' + c.label + '"' : 'Your phone') + ' is signed in to your account within a few seconds.', 'ok');
      Promise.all([loadMe(), loadDevices(), loadSessions(), loadUsage(), loadActivity(true)]);
      // A device is listed once it has collected its tokens (a few seconds after approval).
      setTimeout(function () { loadDevices(); loadUsage(); }, 8000);
    }
    // Ends this code: no more polling; the result and "New code".
    function pairEnd(text, kind) {
      pair.live = false;
      pair.state = 'ended';
      pairStopPoll();
      if (pair.tick) { clearInterval(pair.tick); pair.tick = null; }
      if (!pair.open) setPairOpen(true);
      $('#pair-title').textContent = kind === 'ok' ? 'Device added' : 'Add a device';
      pairMsg('');
      var res = $('#pair-result');
      res.textContent = text;
      res.className = 'pair-result result' + (kind ? ' ' + kind : '');
      pairView('result');
      pairFocus(res);
    }
    function pairFailed(r) {
      var e = r.body.error;
      if (e === 'pair_not_claimed') { pairMsg('Nobody has scanned this code yet. Scan it with your phone first.'); return; }
      var text = e === 'expired_token' ? 'This code has expired. Make a new one to try again.'
        : e === 'quota_exceeded' ? 'Your account has the most devices it can have (' + ((meCache && meCache.max_devices) || 10) +
          '). Remove one above, then make a new code.'
        : e === 'bad_request' ? 'The code didn\'t match. Make a new code and scan it again.'
        : e === 'rate_limited' ? errorText(r) + ' Then make a new code.'
        : errorText(r);
      pairEnd(text, 'error');
    }
    function pairClose() {
      if (pair.live) api('POST', 'pair/cancel', {});
      pair.live = false;
      pairStopPoll();
      if (pair.tick) { clearInterval(pair.tick); pair.tick = null; }
      if (pairPanel.contains(reauthForm)) show(reauthForm, false);
      setPairOpen(false);
      pairOpener.focus();
    }
    pairOpener.addEventListener('click', pairStart);
    $('#pair-new').addEventListener('click', pairStart);
    $('#pair-close').addEventListener('click', pairClose);
    $('#pair-copy').addEventListener('click', function () { copyText($('#pair-url').textContent, 'The link'); });
    pairSame.addEventListener('change', function () { pairApprove.disabled = !pairSame.checked; });
    pairForm.addEventListener('submit', function (ev) {
      ev.preventDefault();
      if (!pairSame.checked || !pair.code || !pair.live) return;
      var code = pair.code;
      reauthForm.querySelector('.why').textContent = 'Enter your password again to approve.';
      withReauth(reauthForm, meCache && meCache.totp, function () {
        return api('POST', 'pair/approve', { confirm_code: code });
      }, pairApproved, pairForm, pairFailed);
    });
    $('#pair-deny').addEventListener('click', function () {
      var b = $('#pair-deny');
      b.disabled = true;
      api('POST', 'pair/deny', {}).then(function (r) {
        b.disabled = false;
        if (r.ok) { pairEnd('Denied. Nothing was signed in.', ''); loadActivity(true); } else pairFailed(r);
      });
    });
    document.addEventListener('visibilitychange', function () {
      if (!document.hidden && pair.open && pair.live) { pairStopPoll(); pairPoll(); }
    });
    // On load: a code someone already claimed (say, after a reload) is shown for approval;
    // otherwise #add-device makes a new one.
    function pairResume() {
      return api('GET', 'pair').then(function (r) {
        if (r.ok && r.body.state === 'claimed') {
          setPairOpen(true);
          pairLive(r.body);
          pairState(r.body);
          $('#add-device').scrollIntoView({ block: 'start' });
          return;
        }
        if (startHash === '#add-device') {
          $('#add-device').scrollIntoView({ block: 'start' });
          pairStart();
        }
      });
    }

    // ---- Usage ---------------------------------------------------------------------------------
    var COLLECTIONS = { history: 'History', bookmarks: 'Bookmarks', settings: 'Settings', tabs: 'Open tabs',
      allowlist: 'Ad blocker exceptions', pins: 'Pinned sites', device_meta: 'Device names', passwords: 'Passwords',
      never_save: 'Sites never to save passwords for', payments: 'Payment cards' };
    function meterSet(name, v, max, text) {
      var m = $('#usage-' + name + '-meter');
      max = Math.max(1, Number(max) || 1);
      m.max = max;
      m.value = Math.min(Math.max(0, Number(v) || 0), max);
      m.low = max * 0.8;
      m.high = max * 0.95;
      m.optimum = 0;
      $('#usage-' + name + '-text').textContent = text;
    }
    function cell(tr, content, cls, header) {
      var c = el(header ? 'th' : 'td', cls || null);
      if (header) c.scope = 'row';
      if (typeof content === 'string') c.textContent = content; else if (content) c.appendChild(content);
      tr.appendChild(c);
      return c;
    }
    function loadUsage() {
      return api('GET', 'usage').then(function (r) {
        var err = $('#usage-error');
        if (!r.ok) {
          err.textContent = r.status === 404 ? 'Usage isn\'t available on this server yet.' : errorText(r);
          show(err, true);
          show($('#usage-body'), false);
          return;
        }
        show(err, false);
        var u = r.body, rec = u.records || {}, dev = u.devices || {}, acc = u.account || {};
        meterSet('records', rec.total, rec.max, count(rec.total) + ' of ' + count(rec.max) + ' items (' + percent(rec.total, rec.max) + ')');
        meterSet('bytes', rec.bytes, rec.max_bytes, size(rec.bytes) + ' of ' + size(rec.max_bytes) + ' (' + percent(rec.bytes, rec.max_bytes) + ')');
        meterSet('history', rec.history, rec.max_history, count(rec.history) + ' of ' + count(rec.max_history) +
          ' history entries (' + percent(rec.history, rec.max_history) + '). Krunk removes the oldest before the limit.');
        $('#usage-record-limit').textContent = 'Sizes are of the encrypted data. One item can be at most ' +
          size(rec.max_record_bytes || 65536) + ' encrypted.';
        var tb = $('#usage-collections tbody');
        tb.textContent = '';
        var cols = (rec.collections || []).slice().sort(function (a, b) { return (b.bytes || 0) - (a.bytes || 0); });
        cols.forEach(function (c) {
          var tr = el('tr');
          var th = cell(tr, COLLECTIONS[c.c] || String(c.c), null, true);
          th.title = String(c.c);
          cell(tr, count(c.records), 'num');
          cell(tr, size(c.bytes), 'num');
          tb.appendChild(tr);
        });
        show($('#usage-coll-empty'), cols.length === 0);
        show($('#usage-collections').parentNode, cols.length > 0);
        $('#usage-devices-count').textContent = count(dev.active) + ' of ' + count(dev.max || 10);
        var dtb = $('#usage-devices tbody');
        dtb.textContent = '';
        (dev.items || []).forEach(function (d) {
          var tr = el('tr');
          cell(tr, d.label + ' (' + platformName(d.platform) + ')', null, true);
          cell(tr, d.last_sync ? timeSpan('', d.last_sync) : 'never');
          cell(tr, d.last_used ? timeSpan('', d.last_used) : '–');
          cell(tr, (d.last_ip || '–') + (d.last_country ? ' · ' + country(d.last_country) : ''));
          dtb.appendChild(tr);
        });
        show($('#usage-devices').parentNode, (dev.items || []).length > 0);
        function fact(id, iso, none) {
          var dd = $(id);
          dd.textContent = '';
          if (iso) {
            var t = timeSpan('', iso);
            var rel = ago(iso);
            t.textContent = when(iso) + (/^on /.test(rel) ? '' : ' (' + rel + ')');
            dd.appendChild(t);
          } else dd.textContent = none;
        }
        fact('#ua-created', acc.created, 'unknown');
        fact('#ua-sign-in', acc.last_sign_in, 'unknown');
        fact('#ua-password', acc.password_changed, 'not changed since the account was made');
        $('#ua-totp').textContent = acc.totp ? 'On' : 'Off';
        $('#ua-codes').textContent = acc.totp ? count(acc.backup_codes_left) : '– (two-step verification is off)';
        show($('#usage-body'), true);
      });
    }

    // ---- Activity -------------------------------------------------------------------------------
    var KINDS = {
      register: 'Account created', login: 'Signed in', qr_sign_in: 'Signed in by QR', logout: 'Signed out',
      password_change: 'Password changed', password_changed: 'Password changed', password_reset: 'Password reset by email',
      totp_enabled: 'Two-step verification turned on', totp_disabled: 'Two-step verification turned off',
      device_linked: 'Device added', device_revoked: 'Device removed', sessions_revoked: 'Signed out everywhere else',
      token_reuse: 'Stolen sign-in suspected: device signed out', pair_started: '"Add a device" code made',
      pair_claimed: '"Add a device" code scanned', pair_approved: '"Add a device" approved', pair_denied: '"Add a device" denied',
      export: 'Data downloaded', sync_reset: 'Sync data reset'
    };
    var act = { before: null, loading: false };
    var moreBtn = $('#activity-more');
    function sourceCell(s) {
      var box = el('span');
      if (!s || !s.kind) { box.textContent = '–'; return box; }
      if (s.kind === 'device') box.textContent = s.label ? s.label : 'a removed device';
      else box.textContent = s.label ? browserName(s.label) : 'a browser';
      if (s.current) { box.appendChild(document.createTextNode(' ')); box.appendChild(el('span', 'badge self', 'This is you')); }
      return box;
    }
    function loadActivity(reset) {
      if (act.loading) return Promise.resolve(null);
      act.loading = true;
      moreBtn.disabled = true;
      moreBtn.setAttribute('aria-busy', 'true');
      var q = 'activity?limit=50' + (!reset && act.before !== null && act.before !== undefined ? '&before=' + encodeURIComponent(act.before) : '');
      return api('GET', q).then(function (r) {
        act.loading = false;
        moreBtn.disabled = false;
        moreBtn.removeAttribute('aria-busy');
        var err = $('#activity-error');
        if (!r.ok) {
          err.textContent = r.status === 404 ? 'The activity list isn\'t available on this server yet.' : errorText(r);
          show(err, true);
          return null;
        }
        show(err, false);
        var tb = $('#activity-rows');
        if (reset) tb.textContent = '';
        var first = null;
        (r.body.events || []).forEach(function (e) {
          var tr = el('tr');
          tr.tabIndex = -1;
          var th = cell(tr, KINDS[e.kind] || String(e.kind), null, true);
          th.title = String(e.kind);
          cell(tr, timeSpan('', e.at));
          cell(tr, e.ip || '–');
          cell(tr, e.country ? country(e.country) : '–');
          cell(tr, sourceCell(e.source));
          tb.appendChild(tr);
          if (!first) first = tr;
        });
        act.before = r.body.next_before;
        show(moreBtn, act.before !== null && act.before !== undefined && act.before !== '');
        var days = Number(r.body.retention_days) || 90;
        $('#activity-retention').textContent = 'Kept for ' + days + ' days, then the entries and their IP addresses are deleted.';
        var none = tb.children.length === 0;
        show($('#activity-empty'), none);
        $('#activity-empty').textContent = 'No activity in the last ' + days + ' days.';
        show($('#activity-table').parentNode, !none);
        return first;
      });
    }
    moreBtn.addEventListener('click', function () {
      loadActivity(false).then(function (first) { if (first) first.focus(); else if (moreBtn.hidden) statusEl.focus(); });
    });

    // ---- Your data: the export ----------------------------------------------------------------
    var expBtn = $('#export-download');
    function resultLine(id, text, kind) {
      var p = $(id);
      p.textContent = text || '';
      p.className = 'state-line result' + (kind ? ' ' + kind : '');
      show(p, !!text);
      return p;
    }
    function today() {
      var d = new Date();
      return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0');
    }
    function exportName(disp) {
      var m = /filename\*?=(?:UTF-8'')?"?([^";]+)"?/i.exec(disp || '');
      var n = m ? m[1].trim() : '';
      return /^krunk-account-[0-9]{4}-[0-9]{2}-[0-9]{2}\.json$/.test(n) ? n : 'krunk-account-' + today() + '.json';
    }
    expBtn.addEventListener('click', function () {
      reauthForm.querySelector('.why').textContent = 'Enter your password again to download your data.';
      resultLine('#export-result', '');
      withReauth(reauthForm, meCache && meCache.totp, function () {
        expBtn.disabled = true;
        expBtn.setAttribute('aria-busy', 'true');
        resultLine('#export-result', 'Preparing your data. This can take up to a minute…');
        return apiFile('export', {}).then(function (r) {
          expBtn.disabled = false;
          expBtn.removeAttribute('aria-busy');
          if (r.body.error === 'reauth_required') resultLine('#export-result', '');
          return r;
        });
      }, function (r) {
        var name = exportName(r.disposition);
        download(name, r.blob);
        afterAction(resultLine('#export-result', 'Saved as ' + name + ' (' + size(r.blob.size) + '). Keep it somewhere safe: it lists your devices and IP addresses.', 'ok'));
        loadActivity(true);
      }, expBtn.parentNode, function (r) {
        afterAction(resultLine('#export-result', errorText(r), 'error'));
      });
    });

    // ---- Sync reset -----------------------------------------------------------------------------
    var PHRASE = 'DELETE SYNC DATA';
    var srForm = $('#sync-reset');
    var srInput = $('#sync-reset-confirm');
    var srGo = $('#sync-reset-go');
    srInput.addEventListener('input', function () { srGo.disabled = srInput.value.trim() !== PHRASE; });
    srForm.addEventListener('submit', function (ev) {
      ev.preventDefault();
      if (srInput.value.trim() !== PHRASE) { fieldError(srInput, 'Type DELETE SYNC DATA exactly, in capitals.'); srInput.focus(); return; }
      fieldError(srInput, '');
      reauthForm.querySelector('.why').textContent = 'Enter your password again to delete your sync data.';
      resultLine('#sync-reset-result', '');
      withReauth(reauthForm, meCache && meCache.totp, function () {
        return api('POST', 'sync/reset', { confirm: PHRASE });
      }, function (r) {
        srForm.reset();
        srGo.disabled = true;
        var text = 'Your sync data and keys are deleted from the server. Your devices stay signed in; turn sync on again in Krunk ' +
          '(Settings → Sync) to set it up with a new passphrase.';
        var d = r.body && r.body.deleted;
        if (d && typeof d === 'object') {
          var parts = Object.keys(d).filter(function (k) { return typeof d[k] === 'number'; }).map(function (k) { return k.replace(/_/g, ' ') + ': ' + count(d[k]); });
          if (parts.length) text += ' Deleted: ' + parts.join(', ') + '.';
        }
        var p = resultLine('#sync-reset-result', text, 'ok');
        Promise.all([loadMe(), loadDevices(), loadUsage(), loadActivity(true)]).then(function () { afterAction(p); });
      }, srForm, function (r) {
        afterAction(resultLine('#sync-reset-result', errorText(r), 'error'));
      });
    });

    window.addEventListener('hashchange', function () {
      if (location.hash === '#add-device' && !pair.open && csrf) pairStart();
      if (location.hash === '#export' && csrf) expBtn.focus();
    });

    sectionNav();
    loadMe().then(function () {
      if (!csrf) return;
      loadDevices();
      loadSessions();
      loadUsage();
      loadActivity(true);
      pairResume();
      if (startHash === '#export') { $('#export').scrollIntoView({ block: 'start' }); expBtn.focus(); }
    });
  };

  // ---- Delete the account (/account/delete) ---------------------------------------------------
  // Signed-in only. A fresh password (and code), the typed email and the bot check; on success
  // the account is gone and the server has cleared the cookie.
  pages['delete'] = function (cfg) {
    var form = $('#form');
    api('GET', 'me').then(function (r) {
      if (r.status === 401) { location.href = '/account/login?next=' + encodeURIComponent('/account/delete'); return; }
      if (!r.ok) { say(errorText(r), 'error'); return; }
      var me = r.body;
      csrf = me.csrf;
      $('#del-email').textContent = me.email;
      show($('#totp-row'), !!me.totp);
      show($('#intro'), true);
      show(form, true);
      loadTurnstile(cfg.turnstile_site_key, 'delete');
      form.addEventListener('submit', function (ev) {
        ev.preventDefault();
        var ok = validate(form, function (i) {
          return i.name === 'confirm_email' && i.value.trim().toLowerCase() !== String(me.email).toLowerCase()
            ? 'This isn\'t the email address of this account.' : '';
        });
        if (!ok || needToken()) return;
        var b = { password: value(form, 'password'), confirm_email: value(form, 'confirm_email').trim(), turnstile: tsToken() };
        if (value(form, 'totp')) b.totp = value(form, 'totp').trim();
        busy(form, true);
        say('');
        api('POST', 'account/delete', b).then(function (x) {
          busy(form, false);
          if (x.ok) { deleted(x.body); return; }
          tsReset();
          var e = x.body.error;
          if (e === 'totp_required' || e === 'totp_invalid') {
            show($('#totp-row'), true);
            field(form, 'totp').focus();
          } else if (e === 'unauthorized') {
            field(form, 'password').value = '';
            field(form, 'password').focus();
          } else if (x.status === 401) {
            say('You are signed out. Sign in again to delete your account.', 'error');
            return;
          }
          say(errorText(x), 'error');
        });
      });
    });
    function deleted(b) {
      csrf = null;
      show(form, false);
      show($('#intro'), false);
      Array.prototype.forEach.call(document.querySelectorAll('[data-signed-in]'), function (n) { show(n, false); });
      var list = $('#remaining');
      list.textContent = '';
      var rem = (b && b.remaining) || {};
      var names = { records: 'Synced records', devices: 'Devices', sessions: 'Sign-in sessions', keys: 'Sync keys' };
      Object.keys(rem).forEach(function (k) {
        list.appendChild(el('dt', null, names[k] || k.replace(/_/g, ' ')));
        list.appendChild(el('dd', null, count(rem[k]) + ' left'));
      });
      show(list, Object.keys(rem).length > 0);
      show($('#done'), true);
      $('#done-title').focus();
    }
  };

  // ---- Sign in by QR on a phone (/pair#<token>) -------------------------------------------------
  // The token is in the fragment (never sent to a server by the browser) and is dropped from the
  // address bar at once. The claim answers a 6-digit code to compare with the signed-in screen;
  // then this page polls until it is approved there (the server sets the session cookie).
  pages.pair = function () {
    var t = location.hash.slice(1);
    if (location.hash) history.replaceState(null, '', location.pathname + location.search);
    var token = /^[A-Za-z0-9_-]{43}$/.test(t) ? t : null;
    var wait = $('#wait');
    var timer = $('#timer');
    var p = { poll: null, tick: null, interval: 2, started: Date.now(), deadline: 0, pollToken: null, done: false };
    function stop(text, kind) {
      p.done = true;
      if (p.poll) clearTimeout(p.poll);
      if (p.tick) clearInterval(p.tick);
      show(wait, false);
      if (text) say(text, kind);
      show($('#again'), kind === 'error');
      statusEl.focus();
    }
    statusEl.tabIndex = -1;
    if (!token) {
      stop('Open the link from the QR code on your signed-in browser: this link is incomplete.', 'error');
      return;
    }
    var CLAIM_ERRORS = {
      not_found: 'This code isn\'t valid. Make a new one on your signed-in browser (Account → Add a device) and scan it again.',
      expired_token: 'This code has expired — make a new one on your signed-in browser.',
      pair_used: 'This code was already used — make a new one on your signed-in browser.'
    };
    say('Checking the code…');
    api('POST', 'pair/claim', { token: token }).then(function (r) {
      if (!r.ok) { stop(CLAIM_ERRORS[r.body.error] || errorText(r), 'error'); return; }
      var b = r.body;
      p.pollToken = b.poll_token;
      p.interval = Math.max(1, Number(b.interval) || 2);
      p.deadline = Date.now() + (Number(b.expires_in) || 300) * 1000;
      $('#claim-code').textContent = b.confirm_code;
      say('');
      show(wait, true);
      $('#wait-title').focus();
      p.tick = setInterval(tick, 1000);
      tick();
      schedule();
    });
    function tick() {
      var left = (p.deadline - Date.now()) / 1000;
      timer.textContent = 'Waiting for approval for ' + secsAgo((Date.now() - p.started) / 1000).replace(' ago', '') +
        (left > 0 ? ' · the code expires in ' + mmss(left) : ' · the code has expired');
      if (left < -6) stop('This code has expired — make a new one on your signed-in browser.', 'error');
    }
    function schedule() { if (!p.done) p.poll = setTimeout(poll, p.interval * 1000); }
    function poll() {
      p.poll = null;
      if (p.done) return;
      api('POST', 'pair/poll', { poll_token: p.pollToken }).then(function (r) {
        if (p.done) return;
        if (r.ok) {
          csrf = r.body.csrf || null;
          stop('Signed in. Opening your account…', 'ok');
          setTimeout(function () { location.href = '/account/'; }, 1200);
          return;
        }
        var e = r.body.error;
        if (e === 'authorization_pending' || r.status === 0) { schedule(); return; }
        if (e === 'slow_down') { p.interval += 2; schedule(); return; }
        if (e === 'rate_limited') { p.interval = Math.max(p.interval, 10); schedule(); return; }
        if (e === 'access_denied') { stop('Denied on your other browser. This browser is not signed in.', 'error'); return; }
        if (e === 'expired_token') { stop('This code has expired — make a new one on your signed-in browser.', 'error'); return; }
        stop(errorText(r), 'error');
      });
    }
    document.addEventListener('visibilitychange', function () {
      if (!document.hidden && !p.done && p.pollToken && p.poll) { clearTimeout(p.poll); poll(); }
    });
  };

  api('GET', 'config').then(function (r) {
    if (!r.ok || !r.body.accounts_enabled) {
      show($('#closed'), true);
      return;
    }
    if (pages[page]) pages[page](r.body);
  });
})();
