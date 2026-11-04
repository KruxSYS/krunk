// Login forms: tell the browser a login form exists (key icon), report a
// submitted login so it can offer "Save password?", and fill a login the
// user picked in the browser's own (trusted) prompt.
// Runs in the top-level document only; iframes never take part.
(function () {
  'use strict';
  if (window.top !== window || !window.__lbHost) return;
  if (location.protocol !== 'https:' && location.protocol !== 'http:') return;
  // Never on LightBrowser's own pages (they are not web content).
  if (/(^|\.)lightbrowser\.invalid$/.test(location.hostname)) return;
  var host = window.__lbHost;
  var announced = false;
  var lastSubmit = { value: null, time: 0 };

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

  function setValue(el, value) {
    var setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }

  // Fill only on the browser's message (sent after the user confirmed).
  host.addEventListener('message', function (e) {
    var d = e.data;
    if (!d || !d.lbFill) return;
    var pw = passwordFields()[0];
    if (!pw) return;
    var user = usernameFor(pw);
    if (user && d.lbFill.username) setValue(user, d.lbFill.username);
    setValue(pw, d.lbFill.password);
    pw.focus();
  });

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
