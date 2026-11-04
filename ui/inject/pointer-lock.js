// Pointer Lock for Krunk's engine (every frame, before the page's scripts).
//
// CEF's Alloy runtime style refuses every pointer-lock request (it does not
// implement WebContentsDelegate::RequestPointerLock), so this script provides
// the API: requestPointerLock() asks the browser, which confines and hides the
// cursor, reads raw mouse input and sends the movement back here. While locked,
// the page keeps getting the real (trusted) mouse events, with movementX/Y taken
// from that raw input and the position frozen where the lock started; movement
// no real event carried goes out as a synthetic mousemove/pointermove. Called with the browser's
// `send(json)`; returns the function the browser's messages are delivered to.
(function (send) {
  'use strict';
  var defineProperty = Object.defineProperty;
  var stringify = JSON.stringify, parse = JSON.parse;
  var DOMExc = DOMException, Prom = Promise, Ev = Event, MouseEv = MouseEvent, PointerEv = window.PointerEvent;
  var ShadowRootClass = window.ShadowRoot;
  var nativeToString = Function.prototype.toString;
  var natives = new WeakMap();

  // Patched functions look native to feature detection.
  function nativeLike(fn, name) { natives.set(fn, name); return fn; }
  var toString = nativeLike(function toString() {
    var name = natives.get(this);
    return name !== undefined ? 'function ' + name + '() { [native code] }' : nativeToString.call(this);
  }, 'toString');
  defineProperty(Function.prototype, 'toString', { value: toString, writable: true, configurable: true });

  var locked = null;   // the element holding the lock
  var pending = null;  // { el, resolve, reject }
  var last = { cx: 0, cy: 0, sx: 0, sy: 0 };  // mouse position when the lock started
  var frozen = last;

  function fire(doc, type) {
    try { doc.dispatchEvent(new Ev(type, { bubbles: true })); } catch (e) { /* detached */ }
  }

  function retarget(el, scope) {
    var n = el;
    while (n) {
      var root = n.getRootNode();
      if (root === scope) return n;
      if (ShadowRootClass && root instanceof ShadowRootClass) n = root.host; else return null;
    }
    return null;
  }

  var request = nativeLike(function requestPointerLock(options) {
    var el = this;
    return new Prom(function (resolve, reject) {
      var doc = el.ownerDocument;
      var fail = function (name, message) { fire(doc, 'pointerlockerror'); reject(new DOMExc(message, name)); };
      if (!el.isConnected) return fail('WrongDocumentError', 'The element is not attached to a document.');
      if (locked === el) return resolve();
      // As in Chrome: a user gesture is needed, unless the document is
      // already full screen (entering full screen used the gesture up).
      var activation = navigator.userActivation;
      if (!locked && activation && !activation.isActive && !doc.fullscreenElement) {
        return fail('NotAllowedError', 'A user gesture is required to request Pointer Lock.');
      }
      if (pending) pending.reject(new DOMExc('A newer request replaced this one.', 'AbortError'));
      pending = { el: el, resolve: resolve, reject: reject };
      send(stringify({ lb: 'pointerLock', unadjusted: !!(options && options.unadjustedMovement) }));
    });
  }, 'requestPointerLock');
  defineProperty(Element.prototype, 'requestPointerLock', { value: request, writable: true, configurable: true, enumerable: true });

  var exit = nativeLike(function exitPointerLock() {
    if (locked && locked.ownerDocument === this) send(stringify({ lb: 'pointerUnlock' }));
  }, 'exitPointerLock');
  defineProperty(Document.prototype, 'exitPointerLock', { value: exit, writable: true, configurable: true, enumerable: true });

  var docGetter = nativeLike(function () { return locked ? retarget(locked, this) : null; }, 'get pointerLockElement');
  defineProperty(Document.prototype, 'pointerLockElement', { get: docGetter, configurable: true, enumerable: true });
  if (ShadowRootClass) {
    var shadowGetter = nativeLike(function () { return locked ? retarget(locked, this) : null; }, 'get pointerLockElement');
    defineProperty(ShadowRootClass.prototype, 'pointerLockElement', { get: shadowGetter, configurable: true, enumerable: true });
  }

  // While locked, the real (trusted) move events keep going to the page, but
  // their movementX/Y come from the browser's raw mouse input and their
  // positions stay where the lock started, as in Chrome. All events of one
  // input (pointerrawupdate, pointermove, mousemove) share a timestamp and
  // get the same movement; the browser's cursor re-centring then never shows.
  var MOVES = { mousemove: 1, pointermove: 1, pointerrawupdate: 1 };
  var acc = { x: 0, y: 0 };      // movement from the browser not given out yet
  var byStamp = new Map();       // event timestamp -> movement given to it
  function movementOf(e) {
    var m = byStamp.get(e.timeStamp);
    if (!m) {
      m = { x: acc.x, y: acc.y };
      acc.x = 0; acc.y = 0;
      byStamp.set(e.timeStamp, m);
      if (byStamp.size > 32) byStamp.delete(byStamp.keys().next().value);
    }
    return m;
  }
  function inLock(e) { return locked !== null && e.isTrusted && MOVES[e.type] === 1; }
  function override(proto, prop, value) {
    var desc = Object.getOwnPropertyDescriptor(proto, prop);
    if (!desc || !desc.get) return;
    var orig = desc.get;
    var get = nativeLike(function () { return inLock(this) ? value(this) : orig.call(this); }, 'get ' + prop);
    defineProperty(proto, prop, { get: get, set: desc.set, configurable: true, enumerable: desc.enumerable });
  }
  var MP = MouseEv.prototype;
  override(MP, 'movementX', function (e) { return movementOf(e).x; });
  override(MP, 'movementY', function (e) { return movementOf(e).y; });
  override(MP, 'clientX', function () { return frozen.cx; });
  override(MP, 'clientY', function () { return frozen.cy; });
  override(MP, 'x', function () { return frozen.cx; });
  override(MP, 'y', function () { return frozen.cy; });
  override(MP, 'pageX', function () { return frozen.cx + window.scrollX; });
  override(MP, 'pageY', function () { return frozen.cy + window.scrollY; });
  override(MP, 'screenX', function () { return frozen.sx; });
  override(MP, 'screenY', function () { return frozen.sy; });

  // The position when the lock starts.
  window.addEventListener('mousemove', function (e) {
    if (!e.isTrusted || locked) return;
    last = { cx: e.clientX, cy: e.clientY, sx: e.screenX, sy: e.screenY };
  }, true);

  // While locked, Chrome sends every mouse event to the locked element. Here
  // the real events go to whatever is under the (hidden) cursor, so a style
  // sheet makes everything but the locked element transparent to the mouse,
  // and the rare move that still lands elsewhere is re-sent to the element.
  var markAttr = 'data-krunk-pl-' + Math.random().toString(36).slice(2, 10);
  var sheet = null;
  function captureTargets(on) {
    try {
      if (on) {
        locked.setAttribute(markAttr, '');
        if (!sheet && typeof CSSStyleSheet === 'function') {
          sheet = new CSSStyleSheet();
          sheet.replaceSync('* { pointer-events: none !important; } [' + markAttr + '], [' + markAttr + '] * { pointer-events: auto !important; }');
        }
        var doc = locked.ownerDocument;
        if (sheet && doc.adoptedStyleSheets.indexOf(sheet) < 0) doc.adoptedStyleSheets = doc.adoptedStyleSheets.concat([sheet]);
      } else if (sheet) {
        var d = document;
        d.adoptedStyleSheets = d.adoptedStyleSheets.filter(function (x) { return x !== sheet; });
      }
    } catch (e) { /* the page froze adoptedStyleSheets; the re-send below still works */ }
  }
  window.addEventListener('mousemove', function (e) {
    var el = locked;
    if (!el || !e.isTrusted || e.target === el || el.contains(e.target)) return;
    e.stopImmediatePropagation();
    var m = movementOf(e);
    el.dispatchEvent(new MouseEv('mousemove', {
      bubbles: true, cancelable: true, composed: true, view: window,
      clientX: frozen.cx, clientY: frozen.cy, screenX: frozen.sx, screenY: frozen.sy,
      movementX: m.x, movementY: m.y, buttons: e.buttons,
    }));
  }, true);

  // Movement that no real event picked up (the mouse moved less than a pixel
  // on screen) goes out as a synthetic event after a moment.
  var flushTimer = 0;
  function moved(dx, dy, buttons) {
    if (!locked) return;
    acc.x += dx; acc.y += dy;
    if (flushTimer) return;
    flushTimer = setTimeout(function () {
      flushTimer = 0;
      var el = locked;
      if (!el || (acc.x === 0 && acc.y === 0)) return;
      var init = {
        bubbles: true, cancelable: true, composed: true, view: window,
        clientX: frozen.cx, clientY: frozen.cy, screenX: frozen.sx, screenY: frozen.sy,
        movementX: acc.x, movementY: acc.y, buttons: buttons, button: -1,
      };
      acc.x = 0; acc.y = 0;
      if (PointerEv) {
        el.dispatchEvent(new PointerEv('pointermove', Object.assign({ pointerId: 1, pointerType: 'mouse', isPrimary: true }, init)));
      }
      init.button = 0;
      el.dispatchEvent(new MouseEv('mousemove', init));
    }, 40);
  }

  // Messages from the browser.
  return function (json) {
    var m;
    try { m = parse(json); } catch (e) { return; }
    if (m.t === 'move') {
      moved(m.dx | 0, m.dy | 0, m.b | 0);
    } else if (m.t === 'granted') {
      if (!pending) { send(stringify({ lb: 'pointerUnlock' })); return; }
      var p = pending; pending = null;
      if (!p.el.isConnected) { send(stringify({ lb: 'pointerUnlock' })); p.reject(new DOMExc('The element was removed.', 'WrongDocumentError')); return; }
      var before = locked;
      if (before && before !== p.el) before.removeAttribute(markAttr);
      locked = p.el; frozen = last;
      captureTargets(true);
      p.resolve();
      if (before !== locked) fire(locked.ownerDocument, 'pointerlockchange');
    } else if (m.t === 'denied') {
      if (!pending) return;
      var d = pending; pending = null;
      fire(d.el.ownerDocument, 'pointerlockerror');
      d.reject(new DOMExc('Pointer lock was refused.', 'NotAllowedError'));
    } else if (m.t === 'released') {
      var was = locked;
      if (was) { was.removeAttribute(markAttr); captureTargets(false); }
      locked = null;
      acc.x = 0; acc.y = 0;
      if (was) fire(was.ownerDocument, 'pointerlockchange');
    }
  };
})
