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

  if (isTop) {
    // Form submissions, Enter in a password field, and clicks on submit-like
    // buttons (for script-driven login forms without a real submit).
    document.addEventListener('submit', function (e) { capture(e.target); }, true);
    document.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && e.target && e.target.type === 'password') capture(e.target.form);
    }, true);
    document.addEventListener('click', function (e) {
      var b = e.target && e.target.closest && e.target.closest('button, input[type="submit"], [role="button"]');
      if (!b) return;
      var form = b.form || b.closest('form');
      if (form ? passwordFields(form).length : passwordFields().length) capture(form);
    }, true);
  }

  // ---- suggestions: which login field has the focus ----
  function fieldKind(el) {
    if (!el || el.tagName !== 'INPUT' || !visible(el)) return null;
    var type = (el.getAttribute('type') || 'text').toLowerCase();
    if (type === 'password') return 'password';
    if ((type === 'text' || type === 'email' || type === 'tel') && passwordFor(el)) return 'username';
    return null;
  }
  function report(el, kind) {
    var r = el.getBoundingClientRect();
    host.postMessage({ lb: 'fieldFocus', field: kind, rect: { x: r.left, y: r.top, width: r.width, height: r.height } });
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
    var pw = null, user = null;
    var target = focused || document.activeElement;
    var kind = fieldKind(target);
    if (kind === 'password') { pw = target; user = usernameFor(pw); }
    else if (kind === 'username') { user = target; pw = passwordFor(user); }
    if (!pw) { pw = passwordFields()[0]; user = pw ? usernameFor(pw) : null; }
    if (!pw) return;
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
