// Login forms: tell the browser a login form exists (key icon), report a
// submitted login so it can offer "Save password?", fill a login the user
// picked in the browser's own (trusted) prompt, and report when a login
// field gets or loses the focus so the browser can show its suggestions.
// Announcing and saving happen in the top-level document only; the focus
// messages come from every http(s) frame (the browser matches the frame's
// own origin), so a login form inside an iframe gets suggestions too.
(function () {
  'use strict';
  if (typeof __lbHost !== 'object' || !__lbHost) return;
  if (location.protocol !== 'https:' && location.protocol !== 'http:') return;
  // Never on Krunk's own pages (they are not web content).
  if (/(^|\.)krunk\.invalid$/.test(location.hostname)) return;
  var host = __lbHost;
  var isTop = window.top === window;
  var announced = false;
  var lastSubmit = { value: null, time: 0 };
  var focused = null; // the login field that has the focus (suggestions fill its form)
  var quietUntil = 0;  // no focus report right after a fill (the filled field gets the focus)

  function visible(el) {
    return !!(el && !el.disabled && (el.offsetWidth || el.offsetHeight || el.getClientRects().length));
  }
  function passwordFields(scope) {
    return Array.prototype.filter.call((scope || document).querySelectorAll('input[type="password"]'), visible);
  }
  // The username is the closest visible text-like input before the password.
  function usernameFor(pw) {
    var scope = pw.form || document;
    var inputs = scope.querySelectorAll('input');
    var found = null;
    for (var i = 0; i < inputs.length; i++) {
      var el = inputs[i];
      if (el === pw) break;
      var type = (el.getAttribute('type') || 'text').toLowerCase();
      if ((type === 'text' || type === 'email' || type === 'tel') && visible(el)) found = el;
    }
    return found;
  }
  // The password field a username field belongs to (the first one after it in its form).
  function passwordFor(user) {
    var scope = user.form || document;
    var pws = passwordFields(scope);
    for (var i = 0; i < pws.length; i++) if (usernameFor(pws[i]) === user) return pws[i];
    return null;
  }

  function check() {
    if (!announced && passwordFields().length) {
      announced = true;
      host.postMessage({ lb: 'loginForm' });
    }
  }

  function capture(scope) {
    var pw = passwordFields(scope && scope.querySelectorAll ? scope : null)[0] || passwordFields()[0];
    if (!pw || !pw.value) return;
    var now = Date.now();
    if (lastSubmit.value === pw.value && now - lastSubmit.time < 3000) return;
    lastSubmit = { value: pw.value, time: now };
    var user = usernameFor(pw);
    host.postMessage({
      lb: 'submit',
      username: user ? String(user.value).slice(0, 512) : '',
      password: String(pw.value).slice(0, 1024),
    });
  }

  // A checkout: a form with a card number is sent (0.6.0). Expiry and name go along when the
  // form has them; the security code never leaves the page.
  var lastCard = { value: null, time: 0 };
  function captureCard(scope) {
    var fields = cardFields(scope && scope.querySelectorAll ? scope : null);
    var num = null;
    for (var i = 0; i < fields.length; i++) if (cardField(fields[i]) === 'cc-number') { num = fields[i]; break; }
    if (!num || !num.value) return;
    var digits = num.value.replace(/[\s-]/g, '');
    if (!/^\d{12,19}$/.test(digits)) return;
    var now = Date.now();
    if (lastCard.value === digits && now - lastCard.time < 3000) return;
    lastCard = { value: digits, time: now };
    var m = { lb: 'cardSubmit', number: digits, expMonth: 0, expYear: 0, name: '' };
    fields.forEach(function (f) {
      var k = cardField(f), v = String(f.value || '').trim();
      if (k === 'cc-name') m.name = v.slice(0, 128);
      else if (k === 'cc-exp-month') m.expMonth = parseInt(v, 10) || 0;
      else if (k === 'cc-exp-year') m.expYear = v.length === 2 ? 2000 + parseInt(v, 10) : parseInt(v, 10) || 0;
      else if (k === 'cc-exp') { var p = v.match(/^(\d{1,2})\s*[\/\-\s]\s*(\d{2,4})$/); if (p) { m.expMonth = parseInt(p[1], 10); m.expYear = p[2].length === 2 ? 2000 + parseInt(p[2], 10) : parseInt(p[2], 10); } }
    });
    host.postMessage(m);
  }

  if (isTop) {
    // Form submissions, Enter in a password field, and clicks on submit-like
    // buttons (for script-driven login forms without a real submit).
    document.addEventListener('submit', function (e) { capture(e.target); captureCard(e.target); }, true);
    document.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && e.target && e.target.type === 'password') capture(e.target.form);
    }, true);
    document.addEventListener('click', function (e) {
      var b = e.target && e.target.closest && e.target.closest('button, input[type="submit"], [role="button"]');
      if (!b) return;
      var form = b.form || b.closest('form');
      if (form ? passwordFields(form).length : passwordFields().length) capture(form);
      if (form ? cardFields(form).length : cardFields().length) captureCard(form);
    }, true);
  }

  // ---- suggestions: which login field has the focus ----
  // Payment-card fields (0.6.0): the autocomplete tokens, else common names.
  var CARD_RE = /(card.?num|cardnumber|ccnum|cc-number|cc_number|credit.?card|\bpan\b)/i;
  function cardField(el) {
    if (!el || el.tagName !== 'INPUT') return null;
    var ac = (el.getAttribute('autocomplete') || '').toLowerCase();
    if (ac.indexOf('cc-') === 0) return ac;
    var hint = (el.name || '') + ' ' + (el.id || '') + ' ' + (el.getAttribute('placeholder') || '');
    return CARD_RE.test(hint) ? 'cc-number' : null;
  }
  function cardFields(scope) {
    return Array.prototype.filter.call((scope || document).querySelectorAll('input'), function (el) { return visible(el) && cardField(el); });
  }
  function fieldKind(el) {
    if (!el || el.tagName !== 'INPUT' || !visible(el)) return null;
    var cc = cardField(el);
    if (cc) return cc === 'cc-csc' ? null : 'card';
    var type = (el.getAttribute('type') || 'text').toLowerCase();
    if (type === 'password') {
      // A new password is being chosen (0.6.0): the browser offers a generated one. The
      // browser re-checks with its own heuristics from the details sent along.
      var ac = (el.getAttribute('autocomplete') || '').toLowerCase();
      return ac === 'new-password' || passwordFields(el.form || document).length >= 2 ? 'newPassword' : 'password';
    }
    if ((type === 'text' || type === 'email' || type === 'tel') && passwordFor(el)) return 'username';
    return null;
  }
  // What the browser needs to decide whether this is a sign-up or change-password form.
  function formDetails(el) {
    var scope = el.form || document;
    var text = (scope.textContent || '') + ' ' + Array.prototype.map.call(scope.querySelectorAll('button, input[type="submit"]'), function (b) { return b.value || b.textContent || ''; }).join(' ');
    return {
      autocomplete: (el.getAttribute('autocomplete') || '').slice(0, 32),
      passwords: passwordFields(scope).length,
      formText: text.replace(/\s+/g, ' ').trim().slice(0, 300),
    };
  }
  function report(el, kind) {
    var r = el.getBoundingClientRect();
    var m = { lb: 'fieldFocus', field: kind, rect: { x: r.left, y: r.top, width: r.width, height: r.height } };
    if (kind !== 'username') {
      var d = formDetails(el);
      m.autocomplete = d.autocomplete; m.passwords = d.passwords; m.formText = d.formText;
    }
    host.postMessage(m);
  }
  function blur() {
    if (!focused) return;
    focused = null;
    host.postMessage({ lb: 'fieldBlur' });
  }
  document.addEventListener('focusin', function (e) {
    var kind = fieldKind(e.target);
    if (!kind) { blur(); return; }
    focused = e.target;
    if (Date.now() < quietUntil) return;
    report(e.target, kind);
  }, true);
  document.addEventListener('focusout', function (e) {
    if (e.target === focused) blur();
  }, true);
  // Typing or Esc dismisses the suggestions; they come back on the next focus.
  document.addEventListener('input', function (e) { if (e.target === focused) blur(); }, true);
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && focused) blur(); }, true);
  window.addEventListener('scroll', function () { if (focused) blur(); }, true);

  function setValue(el, value) {
    var setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }

  // Fill only on the browser's message (sent after the user chose a login). The form of
  // the field that had the focus is filled; otherwise the first login form.
  host.addEventListener('message', function (e) {
    var d = e.data;
    if (!d || !d.lbFill) return;
    if (d.lbFill.card) {
      // A saved card: every card field of the form that has the focus (or the first form
      // with one), by its autocomplete token or name; the security code is never filled.
      var c = d.lbFill.card;
      var scope = (focused && focused.form) || (cardFields()[0] && cardFields()[0].form) || document;
      var mm = String(c.expMonth).padStart(2, '0'), yy = String(c.expYear);
      cardFields(scope).forEach(function (f) {
        var k = cardField(f);
        var hint = ((f.name || '') + ' ' + (f.id || '')).toLowerCase();
        if (k === 'cc-number') setValue(f, c.number);
        else if (k === 'cc-name' || k === 'cc-given-name' || k === 'cc-family-name') setValue(f, c.name);
        else if (k === 'cc-exp-month') setValue(f, mm);
        else if (k === 'cc-exp-year') setValue(f, /2-digit|yy$/.test(hint) || f.maxLength === 2 ? yy.slice(2) : yy);
        else if (k === 'cc-exp') setValue(f, mm + '/' + yy.slice(2));
      });
      focused = null;
      quietUntil = Date.now() + 1000;
      return;
    }
    var pw = null, user = null;
    var target = focused || document.activeElement;
    var kind = fieldKind(target);
    if (kind === 'password' || kind === 'newPassword') { pw = target; user = usernameFor(pw); }
    else if (kind === 'username') { user = target; pw = passwordFor(user); }
    if (!pw) { pw = passwordFields()[0]; user = pw ? usernameFor(pw) : null; }
    if (!pw) return;
    if (d.lbFill.generated) {
      // A generated password: every password field of the form (the confirm field too),
      // the username is left alone.
      passwordFields(pw.form || document).forEach(function (f) { setValue(f, d.lbFill.password); });
      focused = null;
      quietUntil = Date.now() + 1000;
      pw.focus();
      return;
    }
    if (user && d.lbFill.username) setValue(user, d.lbFill.username);
    setValue(pw, d.lbFill.password);
    focused = null;
    quietUntil = Date.now() + 1000;
    pw.focus();
  });

  if (!isTop) return;
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', check, { once: true });
  else check();
  var timer = null;
  // Observe `document`, not documentElement: this runs at document creation,
  // when documentElement may not exist yet (observe(null) would throw).
  new MutationObserver(function () {
    if (announced) return;
    clearTimeout(timer);
    timer = setTimeout(check, 400);
  }).observe(document, { childList: true, subtree: true });
})();
