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
  // 0.28.0: no Function.prototype.toString override any more. It made every native function
  // fail the "lie" checks of bot defences (CreepJS counted 205). The API's functions are method
  // and getter definitions: like native ones they have no prototype and can't be constructed.
  function getterOf(obj, name) { return Object.getOwnPropertyDescriptor(obj, name).get; }

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

  var request = ({ requestPointerLock(options) {
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
      pending = { el: el, resolve: resolve, reject: reject, raw: !!(options && options.unadjustedMovement) };
      send(stringify({ lb: 'pointerLock', unadjusted: !!(options && options.unadjustedMovement) }));
    });
  } }).requestPointerLock;
  defineProperty(Element.prototype, 'requestPointerLock', { value: request, writable: true, configurable: true, enumerable: true });

  var exit = ({ exitPointerLock() {
    if (locked && locked.ownerDocument === this) send(stringify({ lb: 'pointerUnlock' }));
  } }).exitPointerLock;
  defineProperty(Document.prototype, 'exitPointerLock', { value: exit, writable: true, configurable: true, enumerable: true });

  var docGetter = getterOf({ get pointerLockElement() { return locked ? retarget(locked, this) : null; } }, 'pointerLockElement');
  defineProperty(Document.prototype, 'pointerLockElement', { get: docGetter, configurable: true, enumerable: true });
  if (ShadowRootClass) {
    var shadowGetter = getterOf({ get pointerLockElement() { return locked ? retarget(locked, this) : null; } }, 'pointerLockElement');
    defineProperty(ShadowRootClass.prototype, 'pointerLockElement', { get: shadowGetter, configurable: true, enumerable: true });
  }

  // While locked, the real (trusted) move events keep going to the page, but
  // their movementX/Y come from the browser's raw mouse input and their
  // positions stay where the lock started, as in Chrome. As in Chrome, each
  // event stream reports all the movement since its own previous event:
  // pointermove and mousemove (dispatched in pairs, with one timestamp) share
  // an account, pointerrawupdate (its own timing) has another, and every
  // movement from the browser goes into both. A page listening to several
  // streams then sees the full movement in each.
  var MOVES = { mousemove: 'move', pointermove: 'move', pointerrawupdate: 'raw' };
  function account() { return { x: 0, y: 0, byStamp: new Map() }; }
  // 0.7.2 (R7): a real mousemove/pointermove reports the engine's own movement (the hidden
  // cursor's), which belongs to the frame it is dispatched in, as in Chrome. The browser's
  // movement arrives by message and missed that frame now and then, so the turn per frame went
  // 3-9-3 px instead of 6-6-6 (three times as uneven as Chrome). The browser's account still
  // runs alongside: the engine's movement is taken out of it, and where the two disagree (the
  // browser moved the cursor back to the centre of the page: a jump the engine reports as
  // movement) the event gets the account instead. A lock with unadjustedMovement keeps the
  // browser's raw movement only.
  var nativeMove = null;  // the engine's own movementX/Y getters
  var lockRaw = false;
  var AGREE_PX = 8;
  function engineMovement(e, a) {
    var s = window.devicePixelRatio || 1;
    var nx = Math.round(nativeMove.x.call(e) * s), ny = Math.round(nativeMove.y.call(e) * s);
    var near = function (n, p) { return Math.abs(n - p) <= Math.max(AGREE_PX, Math.abs(p) / 2); };
    if (!near(nx, a.x) || !near(ny, a.y)) return null;
    a.x -= nx; a.y -= ny;
    return { x: nx, y: ny };
  }
  var acc = { move: account(), raw: account() };  // movement not given out yet, per stream
  var lastReal = 0;  // when a real event last took movement (performance.now())
  var lastRaw = 0;   // … a real pointerrawupdate
  var now = performance.now.bind(performance);
  function movementOf(e) {
    if (e.isTrusted) {
      lastReal = now();
      if (MOVES[e.type] === 'raw') lastRaw = lastReal;
    }
    var a = acc[MOVES[e.type]] || acc.move;
    var m = a.byStamp.get(e.timeStamp);
    if (!m) {
      m = a === acc.move && e.isTrusted && nativeMove && !lockRaw ? engineMovement(e, a) : null;
      if (!m) {
        m = { x: a.x, y: a.y };
        a.x = 0; a.y = 0;
      }
      a.byStamp.set(e.timeStamp, m);
      if (a.byStamp.size > 32) a.byStamp.delete(a.byStamp.keys().next().value);
    }
    return m;
  }
  function inLock(e) { return locked !== null && e.isTrusted && MOVES[e.type] !== undefined; }
  function override(proto, prop, value) {
    var desc = Object.getOwnPropertyDescriptor(proto, prop);
    if (!desc || !desc.get) return;
    var orig = desc.get;
    var get = getterOf({ get [prop]() { return inLock(this) ? value(this) : orig.call(this); } }, prop);
    defineProperty(proto, prop, { get: get, set: desc.set, configurable: true, enumerable: desc.enumerable });
  }
  // Installed with the first lock of this document (0.7.0, R24): until then every page read
  // of an event's position went through these wrappers for nothing.
  var gettersInstalled = false;
  function installGetters() {
    if (gettersInstalled) return;
    gettersInstalled = true;
    var MP = MouseEv.prototype;
    var dx = Object.getOwnPropertyDescriptor(MP, 'movementX'), dy = Object.getOwnPropertyDescriptor(MP, 'movementY');
    if (dx && dx.get && dy && dy.get) nativeMove = { x: dx.get, y: dy.get };
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
  }

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
  // on screen) goes out as a synthetic event after a moment. While real events
  // keep coming, they carry it: the check just waits again.
  var FALLBACK_MS = 40;
  var flushTimer = 0;
  function moved(dx, dy, buttons) {
    if (!locked) return;
    acc.move.x += dx; acc.move.y += dy;
    // pointerrawupdate events come only while a page listens for them: until they do, the
    // raw account holds just the latest movement (no stale pile-up for a late listener).
    if (now() - lastRaw > 100) { acc.raw.x = 0; acc.raw.y = 0; }
    acc.raw.x += dx; acc.raw.y += dy;
    if (flushTimer) return;
    var check = function () {
      flushTimer = 0;
      var el = locked;
      if (!el) return;
      var since = now() - lastReal;
      if (since < FALLBACK_MS && (acc.move.x !== 0 || acc.move.y !== 0 || acc.raw.x !== 0 || acc.raw.y !== 0)) {
        flushTimer = setTimeout(check, FALLBACK_MS - since);
        return;
      }
      var init = function (a, button) {
        var i = {
          bubbles: true, cancelable: true, composed: true, view: window,
          clientX: frozen.cx, clientY: frozen.cy, screenX: frozen.sx, screenY: frozen.sy,
          movementX: a.x, movementY: a.y, buttons: buttons, button: button,
        };
        a.x = 0; a.y = 0;
        return i;
      };
      var pointer = function (i) { return Object.assign({ pointerId: 1, pointerType: 'mouse', isPrimary: true }, i); };
      if (acc.raw.x !== 0 || acc.raw.y !== 0) {
        var r = init(acc.raw, -1);
        if (PointerEv) el.dispatchEvent(new PointerEv('pointerrawupdate', pointer(r)));
      }
      if (acc.move.x !== 0 || acc.move.y !== 0) {
        var m = init(acc.move, -1);
        if (PointerEv) el.dispatchEvent(new PointerEv('pointermove', pointer(m)));
        m.button = 0;
        el.dispatchEvent(new MouseEv('mousemove', m));
      }
    };
    flushTimer = setTimeout(check, FALLBACK_MS);
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
      installGetters();
      locked = p.el; frozen = last; lockRaw = p.raw;
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
      acc = { move: account(), raw: account() };
      if (was) fire(was.ownerDocument, 'pointerlockchange');
    }
  };
})
