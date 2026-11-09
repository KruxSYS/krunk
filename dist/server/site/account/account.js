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
    return fetch('/api/v1/web/' + path, opts).then(function (res) {
      return res.text().then(function (t) {
        var j = {};
        try { j = t ? JSON.parse(t) : {}; } catch (e) { j = {}; }
        return { status: res.status, ok: res.ok, body: j || {}, retryAfter: res.headers.get('Retry-After') };
      });
    }, function () {
      return { status: 0, ok: false, body: { error: 'network', message: 'The connection failed. Check your network and try again.' } };
    });
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


  // After signing in, go back to the device approval page if that is where the user came from.
  // Only that one path is accepted, so the parameter can't send anyone elsewhere.
  function safeNext() {
    var n = new URLSearchParams(location.search).get('next');
    return n && /^\/link\?code=[0-9A-Za-z-]{1,12}$/.test(n) ? n : null;
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
  function platformName(p) { return p === 'android' ? 'Android' : 'Windows'; }

  // Sensitive actions answer 403 reauth_required unless the password was entered in the last
  // 10 minutes. Then this form asks for it (and a code, if two-step verification is on) and the
  // action runs again. `anchor`: the form is moved right after it, next to the action.
  // web/me says whether the last re-auth is still fresh, so the form usually comes up before the
  // action is tried (no 403 on the way); a 403 reauth_required (a race) brings it up too.
  function withReauth(form, totpOn, action, done, anchor) {
    api('GET', 'me').then(function (m) {
      if (m.ok && m.body.reauth_valid === false) { ask(); return; }
      action().then(function (r) {
        if (r.ok) { done(r); return; }
        if (r.body.error !== 'reauth_required') { say(errorText(r), 'error'); return; }
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
          action().then(function (r2) { if (r2.ok) done(r2); else say(errorText(r2), 'error'); });
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
    var code = (params.get('code') || '').trim().slice(0, 12);
    if (code) history.replaceState(null, '', location.pathname);
    var codeForm = $('#code-form');
    var form = $('#approve-form');
    var me = null;
    api('GET', 'me').then(function (r) {
      if (r.status === 401) {
        location.href = '/account/login?next=' + encodeURIComponent('/link' + (code ? '?code=' + code : ''));
        return;
      }
      if (!r.ok) { say(errorText(r), 'error'); return; }
      me = r.body;
      csrf = me.csrf;
      if (code) lookup(code); else show(codeForm, true);
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
          say(r.status === 404 ? 'This code is not valid or has expired. Start again in Krunk to get a new one.' : errorText(r), 'error');
          codeForm.reset();
          show(codeForm, true);
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
  // <svg> root is accepted.
  function showQr(svgText) {
    var box = $('#totp-qr');
    box.textContent = '';
    if (typeof svgText !== 'string') return;
    var doc = new DOMParser().parseFromString(svgText, 'image/svg+xml');
    var root = doc.documentElement;
    if (!root || root.nodeName !== 'svg' || doc.getElementsByTagName('script').length) return;
    box.appendChild(document.importNode(root, true));
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
  function timeSpan(prefix, iso) {
    var t = el('span', null, prefix + ago(iso));
    t.title = when(iso);
    return t;
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
  function download(name, text) {
    var url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
    var a = el('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(function () { URL.revokeObjectURL(url); }, 2000);
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
          var li = row('', 'browser', uaSummary(x.user_agent), x.current ? [['self', 'This browser']] : []);
          li.appendChild(meta([timeSpan('signed in ', x.created), x.current ? 'active now' : timeSpan('last seen ', x.last_used),
            x.ip_masked || 'address not kept', country(x.country)]));
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
        if (r.status === 401) { location.href = '/account/login'; return; }
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
      showQr(r.body.qr_svg);
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

    sectionNav();
    loadMe().then(function () { if (csrf) { loadDevices(); loadSessions(); } });
  };

  api('GET', 'config').then(function (r) {
    if (!r.ok || !r.body.accounts_enabled) {
      show($('#closed'), true);
      return;
    }
    if (pages[page]) pages[page](r.body);
  });
})();
