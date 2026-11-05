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
  function say(text, kind) {
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
          callback: function (t) { ts.token = t; },
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
  function needToken() {
    if (tsToken()) return false;
    say('Please wait for the security check to finish, then try again.', 'error');
    return true;
  }

  function busy(form, on) {
    Array.prototype.forEach.call(form.querySelectorAll('button'), function (b) { b.disabled = on; });
  }

  // A form whose submit posts `build()` to `path`; `done(r)` on success, `fail(r)` otherwise.
  function onSubmit(form, path, build, done, fail) {
    form.addEventListener('submit', function (ev) {
      ev.preventDefault();
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
      say('The two passwords are different.', 'error');
      return false;
    }
    return true;
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
      location.href = '/account/';
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

  pages.index = function () {
    var overview = $('#overview');
    function load() {
      return api('GET', 'me').then(function (r) {
        if (r.status === 401) { location.href = '/account/login'; return; }
        if (!r.ok) { say(errorText(r), 'error'); return; }
        var me = r.body;
        csrf = me.csrf;
        $('#me-email').textContent = me.email;
        $('#me-created').textContent = new Date(me.created).toLocaleDateString();
        $('#totp-state').textContent = me.totp
          ? 'On. Signing in needs a code from your authenticator app. Backup codes left: ' + me.backup_codes_left + '.'
          : 'Off. Turn it on to sign in with a code from an authenticator app as well as your password.';
        show($('#totp-start'), !me.totp);
        show($('#totp-off'), me.totp);
        show($('#totp-confirm'), false);
        show($('.needs-totp'), me.totp);
        show(overview, true);
      });
    }

    $('#logout').addEventListener('click', function () {
      api('POST', 'logout', {}).then(function () { location.href = '/account/login'; });
    });

    var start = $('#totp-start');
    onSubmit(start, 'totp/setup', function () {
      return { password: value(start, 'password') };
    }, function (r) {
      start.reset();
      show(start, false);
      $('#totp-secret').textContent = r.body.secret_b32;
      showQr(r.body.qr_svg);
      show($('#totp-confirm'), true);
    });

    var confirmForm = $('#totp-confirm');
    onSubmit(confirmForm, 'totp/enable', function () {
      return { code: value(confirmForm, 'code').trim() };
    }, function (r) {
      confirmForm.reset();
      var list = $('#totp-code-list');
      list.textContent = '';
      (r.body.backup_codes || []).forEach(function (c) {
        var li = document.createElement('li');
        li.textContent = c;
        list.appendChild(li);
      });
      load().then(function () { show($('#totp-codes'), true); say('Two-step verification is on.', 'ok'); });
    });

    var off = $('#totp-off');
    onSubmit(off, 'totp/disable', function () {
      return { password: value(off, 'password'), code: value(off, 'code').trim() };
    }, function () {
      off.reset();
      show($('#totp-codes'), false);
      load().then(function () { say('Two-step verification is off.', 'ok'); });
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
    });

    load();
  };

  api('GET', 'config').then(function (r) {
    if (!r.ok || !r.body.accounts_enabled) {
      show($('#closed'), true);
      return;
    }
    if (pages[page]) pages[page](r.body);
  });
})();
