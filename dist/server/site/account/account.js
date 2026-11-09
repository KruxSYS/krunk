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
  // action runs again.
  function withReauth(form, totpOn, action, done) {
    action().then(function (r) {
      if (r.ok) { done(r); return; }
      if (r.body.error !== 'reauth_required') { say(errorText(r), 'error'); return; }
      form.reset();
      show(form.querySelector('.needs-totp'), totpOn);
      show(form, true);
      field(form, 'password').focus();
      form.onsubmit = function (ev) {
        ev.preventDefault();
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
      form.querySelector('.cancel').onclick = function () { show(form, false); say(''); };
    });
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

  pages.index = function () {
    var overview = $('#overview');

    var meCache = null;
    var reauthForm = $('#reauth');
    function loadDevices() {
      return api('GET', 'devices').then(function (r) {
        if (!r.ok) return;
        var list = $('#devices');
        list.textContent = '';
        show($('#no-devices'), r.body.length === 0);
        r.body.forEach(function (d) {
          var li = el('li', d.revoked_at ? 'revoked' : '');
          li.appendChild(el('div', 'name', d.label));
          var meta = platformName(d.platform) + ' · Krunk ' + d.client_version + ' · signed in ' + when(d.created);
          meta += d.revoked_at ? ' · removed ' + when(d.revoked_at) : ' · last seen ' + when(d.last_used) + ' · ' + country(d.last_country);
          li.appendChild(el('div', 'meta', meta));
          if (!d.revoked_at) {
            var row = el('div', 'row-buttons');
            var rename = el('button', 'small', 'Rename');
            rename.type = 'button';
            var remove = el('button', 'small', 'Remove');
            remove.type = 'button';
            row.appendChild(rename);
            row.appendChild(remove);
            li.appendChild(row);
            rename.addEventListener('click', function () { renameForm(li, d); });
            remove.addEventListener('click', function () {
              withReauth(reauthForm, meCache && meCache.totp, function () {
                return api('POST', 'devices/' + d.id + '/revoke', {});
              }, function () {
                say('"' + d.label + '" is removed and signed out.', 'ok');
                loadDevices();
                loadSessions();
              });
            });
          }
          list.appendChild(li);
        });
      });
    }
    function renameForm(li, d) {
      if (li.querySelector('form')) return;
      var f = el('form');
      f.noValidate = true;
      var input = el('input');
      input.name = 'label';
      input.maxLength = 64;
      input.value = d.label;
      input.setAttribute('aria-label', 'New name');
      var save = el('button', 'small primary', 'Save');
      save.type = 'submit';
      var cancel = el('button', 'small', 'Cancel');
      cancel.type = 'button';
      f.appendChild(input);
      f.appendChild(save);
      f.appendChild(cancel);
      li.appendChild(f);
      input.focus();
      input.select();
      cancel.addEventListener('click', function () { li.removeChild(f); });
      f.addEventListener('submit', function (ev) {
        ev.preventDefault();
        api('POST', 'devices/' + d.id + '/rename', { label: input.value }).then(function (r) {
          if (!r.ok) { say(errorText(r), 'error'); return; }
          say('Renamed.', 'ok');
          loadDevices();
        });
      });
    }
    function loadSessions() {
      return api('GET', 'sessions').then(function (r) {
        if (!r.ok) return;
        var list = $('#sessions');
        list.textContent = '';
        r.body.filter(function (x) { return x.kind === 'web'; }).forEach(function (x) {
          var li = el('li');
          li.appendChild(el('div', 'name', uaSummary(x.user_agent) + (x.current ? ' (this browser)' : '')));
          li.appendChild(el('div', 'meta', 'Signed in ' + when(x.created) + ' · last seen ' + when(x.last_used) +
            ' · ' + (x.ip_masked || 'address not kept') + ' · ' + country(x.country)));
          if (!x.current) {
            var b = el('button', 'small', 'Sign out');
            b.type = 'button';
            b.addEventListener('click', function () {
              api('POST', 'sessions/' + x.id + '/revoke', {}).then(function (rr) {
                if (rr.ok) { say('That browser is signed out.', 'ok'); loadSessions(); } else say(errorText(rr), 'error');
              });
            });
            li.appendChild(b);
          }
          list.appendChild(li);
        });
      });
    }
    function load() {
      return api('GET', 'me').then(function (r) {
        if (r.status === 401) { location.href = '/account/login'; return; }
        if (!r.ok) { say(errorText(r), 'error'); return; }
        var me = r.body;
        meCache = me;
        csrf = me.csrf;
        $('#me-email').textContent = me.email;
        $('#me-created').textContent = new Date(me.created).toLocaleDateString();
        $('#totp-state').textContent = me.totp
          ? 'On. Signing in needs a code from your authenticator app. Backup codes left: ' + me.backup_codes_left + '.'
          : 'Off. Turn it on to sign in with a code from an authenticator app as well as your password.';
        show($('#totp-start'), !me.totp);
        show($('#totp-off'), me.totp);
        show($('#totp-confirm'), false);
        show($('#password .needs-totp'), me.totp);
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

    load().then(function () { if (csrf) { loadDevices(); loadSessions(); } });
  };

  api('GET', 'config').then(function (r) {
    if (!r.ok || !r.body.accounts_enabled) {
      show($('#closed'), true);
      return;
    }
    if (pages[page]) pages[page](r.body);
  });
})();
