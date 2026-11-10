// Krunk 0.12.0: the AI agent's probe. The browser evaluates this function expression in an
// isolated world ("krunk-agent") of the tab's main frame with the sensitive-field rules from
// core (`agent::redact::probe_rules()`), then calls `__krunkProbe.<fn>(…)` with DevTools
// `Runtime.evaluate`. Page scripts can't see or change it (they run in another world); the DOM
// is shared, so reading and editing the page works. Same-origin frames are walked from here.
//
// It never puts the value of a password or payment-card field into a result: those are
// blanked, refused or masked. `secrets()` is the one exception: it hands the current values
// to the browser process only, which removes them from every tool result.
(function (rulesIn) {
  'use strict';
  if (globalThis.__krunkProbe) return globalThis.__krunkProbe;

  // The same defaults as core's rules (used when the browser sends none).
  var R = {
    selector: 'input[type="password" i], input[autocomplete^="cc-" i], input[autocomplete*=" cc-" i], ' +
      'textarea[autocomplete^="cc-" i], textarea[autocomplete*=" cc-" i], select[autocomplete^="cc-" i], ' +
      'select[autocomplete*=" cc-" i]',
    namePattern: 'card.?num|cc.?(num|no)|credit.?card|cvv|cvc|csc|security.?code|card.?code|cc.?exp',
    nameAttributes: ['name', 'id', 'aria-label', 'placeholder'],
    luhn: true,
    minDigits: 13,
    maxDigits: 19,
    redacted: '[redacted]',
  };
  if (rulesIn && typeof rulesIn === 'object') {
    for (var k in R) if (Object.prototype.hasOwnProperty.call(rulesIn, k) && rulesIn[k] != null) R[k] = rulesIn[k];
  }
  var nameRe;
  try { nameRe = new RegExp(R.namePattern, 'i'); } catch (e) { nameRe = /card.?num|cvv|cvc/i; }
  var REDACTED = R.redacted;
  var PAYMENT_FRAME = /stripe|braintree|adyen|checkout|card|payment|paypal|klarna|worldpay|square/i;
  var MAX_TEXT = 2000000;

  // ---- helpers ----

  function trim(s, n) {
    s = String(s == null ? '' : s).replace(/\s+/g, ' ').trim();
    return s.length > n ? s.slice(0, n) + '…' : s;
  }

  // A Luhn-valid run of minDigits–maxDigits digits (single spaces or dashes between digits).
  function luhnText(v) {
    if (!R.luhn || !v) return false;
    v = String(v).trim();
    if (!/^\d(?:[ -]?\d)*$/.test(v)) return false;
    var d = v.replace(/[ -]/g, '');
    if (d.length < R.minDigits || d.length > R.maxDigits) return false;
    var sum = 0, alt = false;
    for (var i = d.length - 1; i >= 0; i--) {
      var n = d.charCodeAt(i) - 48;
      if (alt) { n *= 2; if (n > 9) n -= 9; }
      sum += n;
      alt = !alt;
    }
    return sum % 10 === 0;
  }

  function isField(el) {
    if (!el || el.nodeType !== 1) return false;
    var t = el.localName;
    return t === 'input' || t === 'textarea' || t === 'select';
  }

  // Is `el` a password or payment-card field?
  function isSensitive(el) {
    if (!isField(el)) return false;
    try { if (el.matches(R.selector)) return true; } catch (e) { /* a bad selector: the other rules */ }
    for (var i = 0; i < R.nameAttributes.length; i++) {
      var a = el.getAttribute(R.nameAttributes[i]);
      if (a && nameRe.test(a)) return true;
    }
    if (el.localName !== 'select') {
      var type = (el.getAttribute('type') || 'text').toLowerCase();
      if (/^(text|tel|number|search|)$/.test(type) || el.localName === 'textarea') {
        if (luhnText(el.value)) return true;
      }
    }
    return false;
  }

  // This document and every same-origin frame, breadth-first (at most 64).
  function docs() {
    var out = [document];
    for (var i = 0; i < out.length && out.length < 64; i++) {
      var frames = out[i].querySelectorAll('iframe, frame');
      for (var j = 0; j < frames.length && out.length < 64; j++) {
        try { var d = frames[j].contentDocument; if (d && d.documentElement) out.push(d); } catch (e) { /* another site */ }
      }
    }
    return out;
  }

  function queryAll(sel) {
    var all = [], ds = docs();
    for (var i = 0; i < ds.length; i++) {
      var list = ds[i].querySelectorAll(sel);   // throws on a bad selector: reported to the agent
      for (var j = 0; j < list.length; j++) all.push(list[j]);
    }
    return all;
  }

  // The first match in the document, else in the same-origin frames.
  function find(sel) {
    var ds = docs();
    for (var i = 0; i < ds.length; i++) {
      var el = ds[i].querySelector(sel);
      if (el) return el;
    }
    return null;
  }

  function cssEscape(s) {
    return (typeof CSS !== 'undefined' && CSS.escape) ? CSS.escape(s) : String(s).replace(/[^\w-]/g, '\\$&');
  }

  // A selector that finds `el` in its own document.
  function selectorOf(el) {
    var doc = el.ownerDocument;
    var unique = function (s) { try { return doc.querySelectorAll(s).length === 1; } catch (e) { return false; } };
    if (el.id) {
      var s = '#' + cssEscape(el.id);
      if (unique(s)) return s;
    }
    var tag = el.localName;
    var name = el.getAttribute('name');
    if (name) {
      var s2 = tag + '[name="' + name.replace(/["\\]/g, '\\$&') + '"]';
      if (unique(s2)) return s2;
    }
    var parts = [];
    var node = el;
    while (node && node.nodeType === 1 && node !== doc.documentElement && parts.length < 12) {
      if (node.id && unique('#' + cssEscape(node.id))) { parts.unshift('#' + cssEscape(node.id)); break; }
      var part = node.localName;
      var parent = node.parentElement;
      if (parent) {
        var same = 0, idx = 0;
        for (var c = parent.firstElementChild; c; c = c.nextElementSibling) {
          if (c.localName === node.localName) { same++; if (c === node) idx = same; }
        }
        if (same > 1) part += ':nth-of-type(' + idx + ')';
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(' > ');
  }

  // The element's box in the top viewport (CSS px), through same-origin frames.
  function boxOf(el) {
    var r = el.getBoundingClientRect();
    var x = r.left, y = r.top;
    var win = el.ownerDocument.defaultView;
    while (win && win !== window) {
      var fe;
      try { fe = win.frameElement; } catch (e) { fe = null; }
      if (!fe) break;
      var fr = fe.getBoundingClientRect();
      x += fr.left + fe.clientLeft;
      y += fr.top + fe.clientTop;
      win = fe.ownerDocument.defaultView;
    }
    return { x: Math.round(x), y: Math.round(y), w: Math.round(r.width), h: Math.round(r.height) };
  }

  function isVisible(el) {
    var r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    var st = el.ownerDocument.defaultView.getComputedStyle(el);
    return st.visibility !== 'hidden' && st.display !== 'none' && st.opacity !== '0';
  }

  function inViewport(b) {
    return b.x + b.w > 0 && b.y + b.h > 0 && b.x < innerWidth && b.y < innerHeight;
  }

  function labelOf(el) {
    var t = '';
    try { if (el.labels && el.labels.length) t = el.labels[0].innerText; } catch (e) { /* none */ }
    if (!t) t = el.getAttribute('aria-label') || '';
    if (!t) {
      var by = el.getAttribute('aria-labelledby');
      if (by) {
        t = by.split(/\s+/).map(function (id) {
          var n = el.ownerDocument.getElementById(id);
          return n ? n.innerText : '';
        }).join(' ');
      }
    }
    if (!t) t = el.getAttribute('placeholder') || el.getAttribute('title') || '';
    return trim(t, 120);
  }

  function textOf(el) {
    if (el.localName === 'input') return '';
    return trim(el.innerText !== undefined ? el.innerText : el.textContent, 200);
  }

  // A field's value for results: never a sensitive one.
  function valueOf(el) {
    if (isSensitive(el)) return el.value ? REDACTED : '';
    if (el.localName === 'select') {
      return Array.prototype.filter.call(el.options, function (o) { return o.selected; })
        .map(function (o) { return trim(o.label || o.text, 120); }).join(', ');
    }
    var type = (el.getAttribute('type') || '').toLowerCase();
    if (type === 'checkbox' || type === 'radio') return el.checked;
    if (type === 'file') return el.files && el.files.length ? el.files.length + ' file(s)' : '';
    return trim(el.value, 200);
  }

  function deepActive() {
    var a = document.activeElement;
    for (var i = 0; i < 16 && a; i++) {
      if ((a.localName === 'iframe' || a.localName === 'frame')) {
        var d = null;
        try { d = a.contentDocument; } catch (e) { d = null; }
        if (!d || !d.activeElement) break;
        a = d.activeElement;
      } else if (a.shadowRoot && a.shadowRoot.activeElement) {
        a = a.shadowRoot.activeElement;
      } else {
        break;
      }
    }
    return a;
  }

  function isEditable(el) {
    if (!el) return false;
    if (el.isContentEditable) return true;
    if (el.localName === 'textarea') return !el.readOnly && !el.disabled;
    if (el.localName === 'input') {
      var t = (el.getAttribute('type') || 'text').toLowerCase();
      return !/^(button|submit|reset|image|checkbox|radio|file|hidden|range|color)$/.test(t) && !el.readOnly && !el.disabled;
    }
    return false;
  }

  function fire(el, type) {
    var ev = new Event(type, { bubbles: true, cancelable: false, composed: true });
    el.dispatchEvent(ev);
  }

  // The native setter of `value`, so frameworks that watch the property see a change.
  function setValue(el, v) {
    var proto = Object.getPrototypeOf(el);
    var desc = null;
    while (proto && !desc) { desc = Object.getOwnPropertyDescriptor(proto, 'value'); proto = Object.getPrototypeOf(proto); }
    if (desc && desc.set) desc.set.call(el, v); else el.value = v;
  }

  function anySecret() {
    var ds = docs();
    for (var i = 0; i < ds.length; i++) {
      var f = ds[i].querySelectorAll('input, textarea, select');
      for (var j = 0; j < f.length; j++) if (f[j].value && isSensitive(f[j])) return true;
    }
    return false;
  }

  function pad(n) { return (n < 10 ? '0' : '') + n; }

  // ---- the API ----

  // Facts about the page for the agent's context (no field values, no page text beyond
  // headings, labels and the selection).
  function context() {
    var ds = docs();
    var sel = '';
    try {
      var active = deepActive();
      if (!isSensitive(active)) sel = trim(String(getSelection() || ''), 500);
    } catch (e) { sel = ''; }
    var headings = [];
    var hs = document.querySelectorAll('h1, h2, h3');
    for (var i = 0; i < hs.length && headings.length < 30; i++) {
      var t = trim(hs[i].innerText, 120);
      if (t) headings.push({ level: Number(hs[i].localName.slice(1)), text: t });
    }
    var forms = [];
    var counts = { links: 0, buttons: 0, inputs: 0, images: 0, iframes: 0, media: 0 };
    for (var d = 0; d < ds.length; d++) {
      var doc = ds[d];
      counts.links += doc.querySelectorAll('a[href], area[href]').length;
      counts.buttons += doc.querySelectorAll('button, input[type=submit], input[type=button], [role=button]').length;
      counts.inputs += doc.querySelectorAll('input:not([type=hidden]), textarea, select').length;
      counts.images += doc.images.length;
      counts.iframes += doc.querySelectorAll('iframe').length;
      counts.media += doc.querySelectorAll('video, audio').length;
      var fs = doc.forms;
      for (var f = 0; f < fs.length && forms.length < 10; f++) {
        var fields = [];
        var els = fs[f].elements;
        for (var e = 0; e < els.length && fields.length < 12; e++) {
          var el = els[e];
          if (!isField(el) || (el.getAttribute('type') || '').toLowerCase() === 'hidden') continue;
          fields.push({ selector: selectorOf(el), label: labelOf(el), type: el.localName === 'input' ? (el.getAttribute('type') || 'text').toLowerCase() : el.localName, sensitive: isSensitive(el) });
        }
        forms.push({ selector: selectorOf(fs[f]), fields: fields, inFrame: d > 0 });
      }
    }
    var a = deepActive();
    var focused = a && a !== document.body && a !== document.documentElement
      ? { selector: selectorOf(a), tag: a.localName, editable: isEditable(a), sensitive: isSensitive(a) } : null;
    var se = document.scrollingElement || document.documentElement;
    return {
      readyState: document.readyState,
      lang: document.documentElement.lang || '',
      viewport: { width: innerWidth, height: innerHeight, devicePixelRatio: devicePixelRatio },
      scroll: { x: Math.round(scrollX), y: Math.round(scrollY), maxY: Math.max(0, se.scrollHeight - innerHeight) },
      selection: sel,
      focused: focused,
      headings: headings,
      forms: forms,
      counts: counts,
      sameOriginFrames: ds.length - 1,
    };
  }

  // Cleaned outerHTML of the page or of the first match: sensitive values blanked, script and
  // style bodies dropped unless asked, whitespace collapsed. The browser pages the result.
  function html(sel, scripts) {
    var root = sel ? find(sel) : document.documentElement;
    if (!root) return { found: false };
    var clone = root.cloneNode(true);
    // The clone keeps attributes, not live values: walk both in the same order.
    var orig = [root].concat(Array.prototype.slice.call(root.querySelectorAll('input, textarea, select')));
    var copy = [clone].concat(Array.prototype.slice.call(clone.querySelectorAll('input, textarea, select')));
    for (var i = 0; i < orig.length && i < copy.length; i++) {
      var o = orig[i], c = copy[i];
      if (!isField(o)) continue;
      if (isSensitive(o)) {
        if (c.hasAttribute('value') || o.value) c.setAttribute('value', o.value || c.getAttribute('value') ? REDACTED : '');
        if (c.localName === 'textarea') c.textContent = o.value ? REDACTED : '';
        c.setAttribute('data-krunk-redacted', '');
      }
    }
    var drop = clone.querySelectorAll(scripts ? 'template' : 'script, style, template, noscript');
    for (var j = 0; j < drop.length; j++) drop[j].textContent = '';
    if (!scripts && (clone.localName === 'script' || clone.localName === 'style')) clone.textContent = '';
    var s = clone.outerHTML.replace(/\s+/g, ' ');
    return { found: true, html: s.length > MAX_TEXT ? s.slice(0, MAX_TEXT) : s };
  }

  // Readable text (innerText) of the page or of the first match; form values are not part of it.
  function text(sel) {
    var parts = [];
    var root = sel ? find(sel) : null;
    if (sel && !root) return { found: false };
    if (root) {
      parts.push(root.innerText !== undefined ? root.innerText : root.textContent);
    } else {
      var ds = docs();
      for (var i = 0; i < ds.length; i++) {
        var b = ds[i].body || ds[i].documentElement;
        if (!b) continue;
        var t = b.innerText || '';
        if (t.trim()) parts.push(i === 0 ? t : '[frame]\n' + t);
      }
    }
    var s = parts.join('\n\n').replace(/[ \t\u00a0]+/g, ' ').replace(/ ?\n[ \n]*/g, function (m) {
      return m.indexOf('\n', m.indexOf('\n') + 1) >= 0 ? '\n\n' : '\n';
    }).trim();
    return { found: true, text: s.length > MAX_TEXT ? s.slice(0, MAX_TEXT) : s };
  }

  function attrsOf(el) {
    var out = {};
    var sens = isSensitive(el);
    for (var i = 0; i < el.attributes.length && i < 20; i++) {
      var a = el.attributes[i];
      var v = a.value;
      if (sens && a.name === 'value') v = v ? REDACTED : '';
      out[a.name] = trim(v, 200);
    }
    return out;
  }

  function query(sel, limit, offset) {
    var all = queryAll(sel);
    limit = Math.max(1, Math.min(50, limit || 20));
    offset = Math.max(0, offset || 0);
    var items = all.slice(offset, offset + limit).map(function (el) {
      var b = boxOf(el);
      var item = { tag: el.localName, selector: selectorOf(el), text: textOf(el), attributes: attrsOf(el), box: b, visible: isVisible(el), inViewport: inViewport(b) };
      if (isField(el)) { item.value = valueOf(el); if (isSensitive(el)) item.sensitive = true; }
      if (el.ownerDocument !== document) item.inFrame = true;
      return item;
    });
    return { total: all.length, offset: offset, items: items };
  }

  function links(filter, limit, offset) {
    var all = queryAll('a[href], area[href]');
    var f = filter ? String(filter).toLowerCase() : '';
    var out = [];
    for (var i = 0; i < all.length; i++) {
      var a = all[i];
      var t = trim(a.innerText || a.getAttribute('aria-label') || a.getAttribute('title') || (a.querySelector('img[alt]') || { alt: '' }).alt || '', 200);
      var href = a.href;
      if (f && t.toLowerCase().indexOf(f) < 0 && String(href).toLowerCase().indexOf(f) < 0) continue;
      var l = { text: t, href: href };
      if (a.rel) l.rel = a.rel;
      if (a.target) l.target = a.target;
      out.push(l);
    }
    limit = Math.max(1, Math.min(200, limit || 100));
    offset = Math.max(0, offset || 0);
    return { total: out.length, offset: offset, links: out.slice(offset, offset + limit) };
  }

  function fieldInfo(el) {
    var type = el.localName === 'input' ? (el.getAttribute('type') || 'text').toLowerCase() : el.localName;
    var f = { selector: selectorOf(el), label: labelOf(el), type: type, name: el.getAttribute('name') || '', required: !!el.required, value: valueOf(el) };
    if (isSensitive(el)) f.sensitive = true;
    if (el.disabled) f.disabled = true;
    if (el.localName === 'select') {
      f.multiple = !!el.multiple;
      f.options = Array.prototype.slice.call(el.options, 0, 50).map(function (o) { return { value: trim(o.value, 120), label: trim(o.label || o.text, 120) }; });
    }
    if (el.ownerDocument !== document) f.inFrame = true;
    return f;
  }

  function isButton(el) {
    if (el.localName === 'button') return true;
    if (el.localName !== 'input') return false;
    return /^(submit|button|reset|image)$/.test((el.getAttribute('type') || '').toLowerCase());
  }

  function forms() {
    var out = [];
    var ds = docs();
    for (var d = 0; d < ds.length; d++) {
      var doc = ds[d];
      var loose = { selector: null, action: null, method: null, fields: [], buttons: [] };
      var all = doc.querySelectorAll('input, textarea, select, button');
      var byForm = new Map();
      for (var i = 0; i < all.length; i++) {
        var el = all[i];
        if (el.localName === 'input' && (el.getAttribute('type') || '').toLowerCase() === 'hidden') continue;
        var form = el.form;
        var entry = form ? byForm.get(form) : loose;
        if (!entry) {
          entry = { selector: selectorOf(form), action: form.action || '', method: (form.method || 'get').toLowerCase(), fields: [], buttons: [] };
          if (form.getAttribute('name')) entry.name = form.getAttribute('name');
          if (doc !== document) entry.inFrame = true;
          byForm.set(form, entry);
          out.push(entry);
        }
        if (isButton(el)) {
          if (entry.buttons.length < 20) entry.buttons.push({ selector: selectorOf(el), text: trim(el.innerText || el.value || el.getAttribute('aria-label') || '', 120), type: (el.getAttribute('type') || 'submit').toLowerCase() });
        } else if (entry.fields.length < 100) {
          entry.fields.push(fieldInfo(el));
        }
      }
      if (loose.fields.length || loose.buttons.length) { if (doc !== document) loose.inFrame = true; out.push(loose); }
      if (out.length >= 50) break;
    }
    return { forms: out.slice(0, 50) };
  }

  var MEDIA_EXT = /\.(m3u8|mpd|mp4|webm|m4s|ts|mp3|m4a|m4v|aac|ogg|oga|ogv|opus|flac|wav|mov)(?:[?#]|$)/i;
  function mediaUrl(u) {
    if (!u) return '';
    return /^blob:/i.test(u) ? 'blob: (object URL, not fetchable)' : String(u);
  }

  function media() {
    var elements = [];
    var ds = docs();
    for (var d = 0; d < ds.length; d++) {
      var list = ds[d].querySelectorAll('video, audio');
      for (var i = 0; i < list.length && elements.length < 50; i++) {
        var m = list[i];
        var sources = Array.prototype.map.call(m.querySelectorAll('source'), function (s) {
          return { src: mediaUrl(s.src), type: s.type || '' };
        });
        elements.push({
          tag: m.localName, selector: selectorOf(m), src: mediaUrl(m.getAttribute('src') ? m.src : ''),
          currentSrc: mediaUrl(m.currentSrc), duration: isFinite(m.duration) ? Math.round(m.duration * 10) / 10 : (m.duration === Infinity ? 'live' : null),
          paused: m.paused, muted: m.muted, sources: sources, inFrame: d > 0,
        });
      }
    }
    var seen = new Set();
    var requests = [];
    var entries = [];
    try { entries = performance.getEntriesByType('resource'); } catch (e) { entries = []; }
    for (var j = entries.length - 1; j >= 0 && requests.length < 200; j--) {
      var en = entries[j];
      var media = en.initiatorType === 'video' || en.initiatorType === 'audio' || en.initiatorType === 'track';
      if (!media && !MEDIA_EXT.test(en.name)) continue;
      if (seen.has(en.name)) continue;
      seen.add(en.name);
      requests.push({ url: mediaUrl(en.name), type: en.initiatorType });
    }
    requests.reverse();
    return { elements: elements, requests: requests };
  }

  // Scrolls the first match into the middle of the viewport; its centre and whether it is a
  // sensitive field.
  function box(sel) {
    var el = find(sel);
    if (!el) return { found: false };
    try { el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' }); } catch (e) { el.scrollIntoView(); }
    var b = boxOf(el);
    var x = b.x + b.w / 2, y = b.y + b.h / 2;
    var covered = false;
    if (el.ownerDocument === document) {
      var at = document.elementFromPoint(x, y);
      covered = !!at && at !== el && !el.contains(at) && !(at.shadowRoot && at.contains(el));
    }
    return { found: true, x: x, y: y, box: b, visible: isVisible(el), inViewport: inViewport(b), covered: covered, sensitive: isSensitive(el), tag: el.localName };
  }

  function refusedText() { return 'refused: password/card field'; }

  // Sets fields (selector → value) with the native setter plus input and change events.
  function fill(map) {
    var results = {};
    var filled = 0;
    Object.keys(map || {}).forEach(function (sel) {
      var el;
      try { el = find(sel); } catch (e) { results[sel] = 'bad selector'; return; }
      if (!el) { results[sel] = 'not found'; return; }
      if (isSensitive(el)) { results[sel] = refusedText(); return; }
      var v = String(map[sel]);
      if (el.disabled || el.readOnly) { results[sel] = 'refused: the field is disabled or read-only'; return; }
      if (el.isContentEditable && !isField(el)) {
        el.focus();
        el.textContent = v;
        fire(el, 'input');
        results[sel] = 'ok'; filled++;
        return;
      }
      if (!isField(el)) { results[sel] = 'not a form field'; return; }
      if (el.localName === 'select') {
        var r = choose(el, [v]);
        results[sel] = r.ok ? 'ok' : 'no such option';
        if (r.ok) filled++;
        return;
      }
      var type = (el.getAttribute('type') || '').toLowerCase();
      if (type === 'checkbox' || type === 'radio') {
        var on = !/^(false|0|off|no|)$/i.test(v.trim());
        if (el.checked !== on) { el.checked = on; fire(el, 'input'); fire(el, 'change'); }
        results[sel] = 'ok'; filled++;
        return;
      }
      if (type === 'file') { results[sel] = 'refused: file fields can\'t be filled'; return; }
      try { el.focus(); } catch (e) { /* not focusable */ }
      setValue(el, v);
      fire(el, 'input');
      fire(el, 'change');
      // A value that turned the field into a card field is not echoed either.
      results[sel] = 'ok'; filled++;
    });
    return { filled: filled, results: results };
  }

  function choose(el, values) {
    var opts = Array.prototype.slice.call(el.options);
    var picked = [], missing = [];
    var wanted = values.map(function (v) { return String(v); });
    var matches = function (o, v) {
      return o.value === v || trim(o.label || o.text, 500).toLowerCase() === v.trim().toLowerCase();
    };
    if (!el.multiple) wanted = wanted.slice(0, 1);
    var chosen = [];
    wanted.forEach(function (v) {
      var o = opts.filter(function (x) { return matches(x, v); })[0];
      if (o) chosen.push(o); else missing.push(v);
    });
    if (!chosen.length) return { ok: false, missing: missing };
    if (el.multiple) opts.forEach(function (o) { o.selected = chosen.indexOf(o) >= 0; });
    else chosen[0].selected = true;
    chosen.forEach(function (o) { picked.push(trim(o.label || o.text, 120)); });
    fire(el, 'input');
    fire(el, 'change');
    return { ok: true, selected: picked, missing: missing };
  }

  function select(sel, values) {
    var el = find(sel);
    if (!el) return { error: 'no element matches the selector' };
    if (el.localName !== 'select') return { error: 'the element is not a <select>' };
    if (isSensitive(el)) return { refused: refusedText() };
    if (el.disabled) return { error: 'the field is disabled' };
    var r = choose(el, values || []);
    if (!r.ok) return { error: 'no option matches', missing: r.missing };
    return { selected: r.selected, missing: r.missing };
  }

  // Focuses the first match unless it is sensitive.
  function focus(sel) {
    var el = find(sel);
    if (!el) return { found: false };
    if (isSensitive(el)) return { found: true, sensitive: true };
    try { el.scrollIntoView({ block: 'center', behavior: 'instant' }); } catch (e) { /* old engines */ }
    el.focus();
    var a = deepActive();
    return { found: true, sensitive: false, focused: a === el, editable: isEditable(el) };
  }

  function focusedSensitive() {
    var a = deepActive();
    return { sensitive: isSensitive(a), editable: isEditable(a), tag: a ? a.localName : null };
  }

  function scroll(dx, dy, to, sel) {
    if (sel) {
      var el = find(sel);
      if (!el) return { error: 'no element matches the selector' };
      try { el.scrollIntoView({ block: 'center', behavior: 'instant' }); } catch (e) { el.scrollIntoView(); }
    } else if (to === 'top') {
      window.scrollTo({ top: 0, left: scrollX, behavior: 'instant' });
    } else if (to === 'bottom') {
      var se0 = document.scrollingElement || document.documentElement;
      window.scrollTo({ top: se0.scrollHeight, left: scrollX, behavior: 'instant' });
    } else {
      window.scrollBy({ left: dx || 0, top: dy || 0, behavior: 'instant' });
    }
    var se = document.scrollingElement || document.documentElement;
    return { x: Math.round(scrollX), y: Math.round(scrollY), maxX: Math.max(0, se.scrollWidth - innerWidth), maxY: Math.max(0, se.scrollHeight - innerHeight) };
  }

  // Resolves {found:true} when a match appears, {found:false} after `timeout` ms.
  function waitFor(sel, timeout) {
    find(sel);   // a bad selector throws here
    return new Promise(function (resolve) {
      var done = false, obs = null, timer = null, poll = null;
      var end = function (found) {
        if (done) return;
        done = true;
        if (obs) obs.disconnect();
        clearTimeout(timer);
        clearInterval(poll);
        resolve({ found: found });
      };
      var check = function () { try { if (find(sel)) end(true); } catch (e) { end(false); } };
      check();
      if (done) return;
      obs = new MutationObserver(check);
      obs.observe(document.documentElement, { childList: true, subtree: true, attributes: true });
      poll = setInterval(check, 250);   // same-origin frames are not observed
      timer = setTimeout(function () { end(false); }, Math.max(0, Math.min(30000, timeout || 0)));
    });
  }

  var CSS_FETCH = /url\s*\(|@import|image-set\s*\(|image\s*\(/i;

  function addSheet(doc, cssText) {
    try {
      var s = new doc.defaultView.CSSStyleSheet();
      s.replaceSync(cssText);
      doc.adoptedStyleSheets = doc.adoptedStyleSheets.concat([s]);
      return { sheet: s };
    } catch (e) {
      var st = doc.createElement('style');
      st.textContent = cssText;
      (doc.head || doc.documentElement).appendChild(st);
      return { style: st };
    }
  }

  function removeSheet(doc, h) {
    if (!h) return;
    if (h.sheet) {
      try { doc.adoptedStyleSheets = doc.adoptedStyleSheets.filter(function (s) { return s !== h.sheet; }); } catch (e) { /* gone */ }
    }
    if (h.style && h.style.parentNode) h.style.parentNode.removeChild(h.style);
  }

  // Live edits of the document. Sensitive fields can't be changed, and nothing that could run
  // script or send a field's value away (event handler attributes, javascript: addresses,
  // CSS that loads resources while a password or card field holds a value) is accepted.
  function edit(action, sel, name, value, css, all) {
    if (action === 'insert_css') {
      if (!css) return { error: 'css is missing' };
      if (CSS_FETCH.test(css) && anySecret()) return { refused: 'CSS that loads resources is not allowed while a password or card field holds a value' };
      addSheet(document, String(css));
      return { ok: true, count: 1 };
    }
    if (!sel) return { error: 'selector is missing' };
    var targets = all ? queryAll(sel) : [find(sel)].filter(Boolean);
    if (!targets.length) return { error: 'no element matches the selector' };
    for (var i = 0; i < targets.length; i++) if (isSensitive(targets[i])) return { refused: refusedText() };
    var n = String(name || '').toLowerCase();
    if (action === 'set_attribute' || action === 'remove_attribute') {
      if (!n) return { error: 'name is missing' };
      if (/^on/.test(n) || n === 'srcdoc') return { refused: 'event handler attributes are not allowed' };
      if (action === 'set_attribute' && /^\s*(javascript|vbscript|data\s*:\s*text\/html)/i.test(String(value || ''))) return { refused: 'script addresses are not allowed' };
      if (n === 'type' || n === 'autocomplete' || n === 'name' || n === 'id') {
        // Changing these could turn a password or card field into a plain one.
        for (var j = 0; j < targets.length; j++) {
          var inner = targets[j].querySelectorAll ? targets[j].querySelectorAll('input, textarea, select') : [];
          for (var q = 0; q < inner.length; q++) if (isSensitive(inner[q])) return { refused: refusedText() };
        }
      }
      if (n === 'style' && CSS_FETCH.test(String(value || '')) && anySecret()) return { refused: 'CSS that loads resources is not allowed while a password or card field holds a value' };
    }
    var decl = String(value != null ? value : (css || ''));
    if (action === 'set_style' && CSS_FETCH.test(decl) && anySecret()) return { refused: 'CSS that loads resources is not allowed while a password or card field holds a value' };
    targets.forEach(function (el) {
      switch (action) {
        case 'set_text': el.textContent = String(value == null ? '' : value); break;
        case 'set_attribute': el.setAttribute(name, String(value == null ? '' : value)); break;
        case 'remove_attribute': el.removeAttribute(name); break;
        case 'set_style': el.style.cssText = el.style.cssText + ';' + decl; break;
        case 'remove': el.remove(); break;
      }
    });
    return { ok: true, count: targets.length };
  }

  function setTitle(t) {
    document.title = String(t);
    return { title: document.title };
  }

  // The current values of the sensitive fields (for the browser's scrub of tool results only).
  function secrets() {
    var out = [];
    var ds = docs();
    for (var i = 0; i < ds.length; i++) {
      var f = ds[i].querySelectorAll('input, textarea, select');
      for (var j = 0; j < f.length; j++) {
        var el = f[j];
        if (el.localName === 'select' || !isSensitive(el)) continue;
        var v = el.value;
        if (v && out.indexOf(v) < 0) out.push(v);
        var dv = el.getAttribute('value');
        if (dv && out.indexOf(dv) < 0) out.push(dv);
      }
    }
    return out;
  }

  // Screenshot masking: sensitive fields drawn as discs in transparent text, and a grey cover
  // over other sites' frames that look like payment forms.
  var masked = [];   // [el, old inline values]
  var covers = [];
  var sheets = [];
  var MASK_PROPS = ['-webkit-text-security', 'color', 'text-shadow', 'caret-color', '-webkit-text-fill-color'];
  function mask(on) {
    unmask();
    if (!on) return Promise.resolve({ masked: 0, covered: 0 });
    var ds = docs();
    for (var i = 0; i < ds.length; i++) {
      var doc = ds[i];
      var f = doc.querySelectorAll('input, textarea, select');
      var any = false;
      for (var j = 0; j < f.length; j++) {
        var el = f[j];
        if (!isSensitive(el)) continue;
        any = true;
        var old = MASK_PROPS.map(function (p) { return [p, el.style.getPropertyValue(p), el.style.getPropertyPriority(p)]; });
        masked.push([el, old]);
        el.setAttribute('data-krunk-mask', '');
        el.style.setProperty('-webkit-text-security', 'disc', 'important');
        el.style.setProperty('color', 'transparent', 'important');
        el.style.setProperty('-webkit-text-fill-color', 'transparent', 'important');
        el.style.setProperty('text-shadow', 'none', 'important');
        el.style.setProperty('caret-color', 'transparent', 'important');
      }
      if (any) {
        sheets.push([doc, addSheet(doc, '[data-krunk-mask]{-webkit-text-security:disc!important;color:transparent!important;' +
          '-webkit-text-fill-color:transparent!important;text-shadow:none!important;caret-color:transparent!important}')]);
      }
      var frames = doc.querySelectorAll('iframe, frame');
      for (var k = 0; k < frames.length; k++) {
        var fr = frames[k];
        var same = false;
        try { same = !!(fr.contentDocument && fr.contentDocument.documentElement); } catch (e) { same = false; }
        if (same) continue;
        var hint = [fr.getAttribute('src') || '', fr.getAttribute('name') || '', fr.getAttribute('title') || '', fr.id || '', fr.className || ''].join(' ');
        if (!PAYMENT_FRAME.test(hint)) continue;
        var r = fr.getBoundingClientRect();
        if (r.width <= 0 || r.height <= 0) continue;
        var c = doc.createElement('div');
        c.setAttribute('data-krunk-mask-cover', '');
        var st = c.style;
        st.setProperty('position', 'fixed', 'important');
        st.setProperty('left', r.left + 'px', 'important');
        st.setProperty('top', r.top + 'px', 'important');
        st.setProperty('width', r.width + 'px', 'important');
        st.setProperty('height', r.height + 'px', 'important');
        st.setProperty('background', '#9ca3af', 'important');
        st.setProperty('z-index', '2147483647', 'important');
        st.setProperty('pointer-events', 'none', 'important');
        st.setProperty('margin', '0', 'important');
        st.setProperty('border', '0', 'important');
        (doc.body || doc.documentElement).appendChild(c);
        covers.push(c);
      }
    }
    var result = { masked: masked.length, covered: covers.length };
    // Painted before the capture (a timer too: a hidden page gets no animation frames).
    return new Promise(function (resolve) {
      var done = false;
      var end = function () { if (!done) { done = true; resolve(result); } };
      try { requestAnimationFrame(function () { requestAnimationFrame(end); }); } catch (e) { /* none */ }
      setTimeout(end, 120);
    });
  }

  function unmask() {
    masked.forEach(function (m) {
      var el = m[0];
      el.removeAttribute('data-krunk-mask');
      m[1].forEach(function (p) {
        if (p[1]) el.style.setProperty(p[0], p[1], p[2]); else el.style.removeProperty(p[0]);
      });
      if (el.getAttribute('style') === '') el.removeAttribute('style');
    });
    masked = [];
    covers.forEach(function (c) { if (c.parentNode) c.parentNode.removeChild(c); });
    covers = [];
    sheets.forEach(function (s) { removeSheet(s[0], s[1]); });
    sheets = [];
  }

  // The local time for file names: "2026-11-10 14.03.07".
  function now() {
    var d = new Date();
    return d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate()) + ' ' +
      pad(d.getHours()) + '.' + pad(d.getMinutes()) + '.' + pad(d.getSeconds());
  }

  var api = Object.freeze({
    context: context, html: html, text: text, query: query, links: links, forms: forms, media: media,
    box: box, fill: fill, select: select, focus: focus, focusedSensitive: focusedSensitive,
    scroll: scroll, waitFor: waitFor, edit: edit, setTitle: setTitle, secrets: secrets, mask: mask,
    now: now,
  });
  Object.defineProperty(globalThis, '__krunkProbe', { value: api, writable: false, configurable: false, enumerable: false });
  return api;
})
