// A hex editor for krunk://sitedata's edit dialog: the bytes of a stored value in one of
// several encodings (HexCodec, pure functions), and an editor for them (HexEditor):
// offset / hex / text columns, overwrite or insert, selection, copy and paste as hex, undo.
// Only the rows in view are drawn, so long values stay fast.
(function () {
  'use strict';

  // ---------- codec ----------
  const ENCODINGS = {
    utf8: 'UTF-8 text',
    latin1: 'Binary string (one byte per character)',
    utf16: 'UTF-16LE (two bytes per character)',
    base64: 'Base64',
    percent: 'Percent-encoded (%XX)',
  };
  // Characters percent-encoding keeps as they are (all of them are fine in a cookie).
  const PLAIN = /[A-Za-z0-9\-._~!*'()+/:=@,$&]/;

  function wellFormed(s) {
    if (typeof s.isWellFormed === 'function') return s.isWellFormed();
    return !/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?:^|[^\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(s);
  }
  function latin1String(bytes) {
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return s;
  }
  function utf8Valid(bytes) {
    try { new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes); return true; } catch (e) { return false; }
  }
  // How a Base64 value is written (kept when it is written again).
  function base64Style(s) { return { url: /[-_]/.test(s), pad: /=$/.test(s.trim()) || (s.trim().length % 4 === 0) }; }

  // Text as stored -> bytes, or an error.
  function encode(text, enc) {
    switch (enc) {
      case 'utf8':
        if (!wellFormed(text)) return { error: 'It holds characters UTF-8 cannot represent (unpaired surrogates)' };
        return { bytes: new TextEncoder().encode(text) };
      case 'latin1': {
        const b = new Uint8Array(text.length);
        for (let i = 0; i < text.length; i++) {
          const c = text.charCodeAt(i);
          if (c > 0xff) return { error: 'It holds characters above U+00FF, so it is not a binary string' };
          b[i] = c;
        }
        return { bytes: b };
      }
      case 'utf16': {
        const b = new Uint8Array(text.length * 2);
        for (let i = 0; i < text.length; i++) { const c = text.charCodeAt(i); b[2 * i] = c & 0xff; b[2 * i + 1] = c >> 8; }
        return { bytes: b };
      }
      case 'base64': {
        let s = text.replace(/\s+/g, '').replace(/-/g, '+').replace(/_/g, '/');
        if (!/^[A-Za-z0-9+/]*={0,2}$/.test(s) || s.replace(/=+$/, '').length % 4 === 1) return { error: 'It is not valid Base64' };
        s = s.replace(/=+$/, '');
        while (s.length % 4) s += '=';
        try {
          const bin = atob(s);
          const b = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) b[i] = bin.charCodeAt(i);
          return { bytes: b };
        } catch (e) { return { error: 'It is not valid Base64' }; }
      }
      case 'percent': {
        if (!wellFormed(text)) return { error: 'It holds unpaired surrogates' };
        const out = [];
        const enc8 = new TextEncoder();
        for (let i = 0; i < text.length; i++) {
          const ch = text[i];
          if (ch === '%' && /^[0-9A-Fa-f]{2}$/.test(text.substr(i + 1, 2))) {
            out.push(parseInt(text.substr(i + 1, 2), 16));
            i += 2;
          } else {
            const cp = text.codePointAt(i);
            const s = String.fromCodePoint(cp);
            enc8.encode(s).forEach(function (x) { out.push(x); });
            i += s.length - 1;
          }
        }
        return { bytes: Uint8Array.from(out) };
      }
    }
    return { error: 'Unknown encoding' };
  }

  // Bytes -> text to store, or an error. `style`: how a Base64 value was written.
  function decode(bytes, enc, style) {
    switch (enc) {
      case 'utf8':
        try { return { text: new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes) }; } catch (e) {
          return { error: 'These bytes are not valid UTF-8' };
        }
      case 'latin1': return { text: latin1String(bytes) };
      case 'utf16': {
        if (bytes.length % 2) return { error: 'UTF-16 needs an even number of bytes' };
        let s = '';
        for (let i = 0; i < bytes.length; i += 2) s += String.fromCharCode(bytes[i] | (bytes[i + 1] << 8));
        return { text: s };
      }
      case 'base64': {
        let s = btoa(latin1String(bytes));
        if (style && style.url) s = s.replace(/\+/g, '-').replace(/\//g, '_');
        if (style && !style.pad) s = s.replace(/=+$/, '');
        return { text: s };
      }
      case 'percent': {
        let s = '';
        bytes.forEach(function (b) {
          const ch = String.fromCharCode(b);
          s += b < 0x80 && PLAIN.test(ch) ? ch : '%' + (b < 16 ? '0' : '') + b.toString(16).toUpperCase();
        });
        return { text: s };
      }
    }
    return { error: 'Unknown encoding' };
  }

  // Bytes that are not text: control bytes (other than tab and line breaks) or invalid UTF-8.
  function binaryBytes(bytes) {
    for (let i = 0; i < bytes.length; i++) {
      const b = bytes[i];
      if ((b < 0x20 && b !== 9 && b !== 10 && b !== 13) || b === 0x7f) return true;
    }
    return !utf8Valid(bytes);
  }
  function hasControl(s) { return /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F�]/.test(s); }
  function base64Binary(s) {
    const t = s.trim();
    if (t.length < 16 || !/^[A-Za-z0-9+/_-]+={0,2}$/.test(t) || !/[0-9+/_-]/.test(t)) return false;
    const r = encode(t, 'base64');
    return !r.error && binaryBytes(r.bytes);
  }
  function percentBinary(s) {
    if (!/%[0-9A-Fa-f]{2}/.test(s)) return false;
    const r = encode(s, 'percent');
    return !r.error && binaryBytes(r.bytes);
  }

  // The encoding a value most likely uses.
  function detect(s, cookie) {
    if (!wellFormed(s)) return cookie ? 'percent' : 'utf16';
    if (hasControl(s)) {
      if (cookie) return 'percent';
      return /^[\u0000-ÿ]*$/.test(s) ? 'latin1' : 'utf16';
    }
    if (cookie && percentBinary(s)) return 'percent';
    if (base64Binary(s)) return 'base64';
    if (!cookie && /^[\u0000-ÿ]*$/.test(s) && /[\u0080-ÿ]/.test(s) && !/[a-z]{3}/i.test(s.replace(/[\u0080-ÿ]/g, ''))) return 'latin1';
    return 'utf8';
  }
  // Whether the dialog opens on the Hex tab.
  function looksBinary(s, cookie) {
    return !wellFormed(s) || hasControl(s) || base64Binary(s) || (cookie && percentBinary(s)) || detect(s, cookie) === 'latin1';
  }

  function hex2(b) { return (b < 16 ? '0' : '') + b.toString(16).toUpperCase(); }
  function toHex(bytes) { return Array.prototype.map.call(bytes, hex2).join(' '); }
  // "DE AD be-ef", "0xDE,0xAD", "deadbeef" -> bytes; null if it is not hex.
  function parseHex(text) {
    const t = text.replace(/0x/gi, '').replace(/[\s,;:\-]+/g, '');
    if (!t.length || t.length % 2 || /[^0-9a-f]/i.test(t)) return null;
    const b = new Uint8Array(t.length / 2);
    for (let i = 0; i < b.length; i++) b[i] = parseInt(t.substr(2 * i, 2), 16);
    return b;
  }

  window.HexCodec = { ENCODINGS: ENCODINGS, encode: encode, decode: decode, detect: detect, looksBinary: looksBinary,
    base64Style: base64Style, binaryBytes: binaryBytes, toHex: toHex, parseHex: parseHex };

  // ---------- editor ----------
  const PER_ROW = 16;
  const ROW_H = 20;
  const el = LB.el;

  // An editor in `box`; `onChange(bytes)` after every edit.
  function create(box, onChange) {
    let bytes = [];
    let caret = 0;          // byte index (bytes.length: after the last byte)
    let nibble = 0;         // 1: the high half of the byte at the caret was just typed
    let anchor = null;      // selection anchor (byte index) or null
    let pane = 'hex';
    let insert = false;
    let undo = [];
    let dragging = false;

    const status = el('div', { class: 'hx-status' });
    const spacer = el('div', { class: 'hx-spacer' });
    const rowsBox = el('div', { class: 'hx-rows' });
    const view = el('div', { class: 'hx-view', tabindex: '0', role: 'textbox', 'aria-label': 'Bytes (hex)', 'aria-multiline': 'true' }, [spacer, rowsBox]);
    box.textContent = '';
    box.appendChild(view);
    box.appendChild(status);

    function selRange() {
      if (anchor === null || anchor === caret) return null;
      return [Math.min(anchor, caret), Math.max(anchor, caret)];
    }
    function snapshot() {
      undo.push({ bytes: bytes.slice(), caret: caret });
      if (undo.length > 100) undo.shift();
    }
    function changed() { render(); if (onChange) onChange(Uint8Array.from(bytes)); }
    function deleteSel() {
      const r = selRange();
      if (!r) return false;
      bytes.splice(r[0], r[1] - r[0]);
      caret = r[0]; anchor = null; nibble = 0;
      return true;
    }
    function insertBytes(arr) {
      snapshot();
      deleteSel();
      if (insert || caret >= bytes.length) Array.prototype.splice.apply(bytes, [caret, 0].concat(Array.from(arr)));
      else for (let i = 0; i < arr.length; i++) { if (caret + i < bytes.length) bytes[caret + i] = arr[i]; else bytes.push(arr[i]); }
      caret += arr.length; nibble = 0;
      changed();
      reveal();
    }
    function typeNibble(d) {
      if (selRange()) { snapshot(); deleteSel(); bytes.splice(caret, 0, d << 4); nibble = 1; changed(); reveal(); return; }
      if (nibble === 1) {
        bytes[caret] = (bytes[caret] & 0xf0) | d;
        caret++; nibble = 0;
      } else {
        snapshot();
        if (insert || caret >= bytes.length) bytes.splice(caret, 0, d << 4);
        else bytes[caret] = (bytes[caret] & 0x0f) | (d << 4);
        nibble = 1;
      }
      changed();
      reveal();
    }
    function move(to, extend) {
      to = Math.max(0, Math.min(bytes.length, to));
      if (extend) { if (anchor === null) anchor = caret; } else anchor = null;
      caret = to; nibble = 0;
      render(); reveal();
    }
    function visibleRows() { return Math.max(1, Math.floor(view.clientHeight / ROW_H)); }
    function reveal() {
      const row = Math.floor(caret / PER_ROW);
      if (row * ROW_H < view.scrollTop) view.scrollTop = row * ROW_H;
      else if ((row + 1) * ROW_H > view.scrollTop + view.clientHeight) view.scrollTop = (row + 1) * ROW_H - view.clientHeight;
      render();
    }

    function render() {
      const rows = Math.floor(bytes.length / PER_ROW) + 1;
      spacer.style.height = rows * ROW_H + 'px';
      const first = Math.max(0, Math.floor(view.scrollTop / ROW_H) - 2);
      const last = Math.min(rows, first + visibleRows() + 5);
      const r = selRange();
      rowsBox.textContent = '';
      rowsBox.style.top = first * ROW_H + 'px';
      for (let row = first; row < last; row++) {
        const start = row * PER_ROW;
        const hexes = [], chars = [];
        for (let i = start; i < start + PER_ROW; i++) {
          const has = i < bytes.length;
          if (!has && i !== bytes.length) { hexes.push(el('span', { class: 'b none' })); chars.push(el('span', { class: 'c none' })); continue; }
          let cls = '';
          if (r && i >= r[0] && i < r[1]) cls += ' sel';
          if (i === caret && !r) cls += ' caret' + (nibble ? ' lo' : '') + (pane === 'hex' ? ' here' : '');
          const b = bytes[i];
          hexes.push(el('span', { class: 'b' + cls + (has ? '' : ' end'), 'data-i': String(i), text: has ? hex2(b) : '' }));
          let ccls = '';
          if (r && i >= r[0] && i < r[1]) ccls += ' sel';
          if (i === caret && !r) ccls += ' caret' + (pane === 'ascii' ? ' here' : '');
          chars.push(el('span', { class: 'c' + ccls + (has ? '' : ' end'), 'data-i': String(i), text: has ? (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '.') : '' }));
        }
        rowsBox.appendChild(el('div', { class: 'hx-row' }, [
          el('span', { class: 'o', text: start.toString(16).toUpperCase().padStart(8, '0') }),
          el('span', { class: 'hx' }, hexes),
          el('span', { class: 'tx' }, chars),
        ]));
      }
      const parts = ['Offset 0x' + caret.toString(16).toUpperCase() + ' (' + caret + ')'];
      if (r) parts.push((r[1] - r[0]) + ' selected');
      parts.push(bytes.length + (bytes.length === 1 ? ' byte' : ' bytes'));
      parts.push(insert ? 'Insert' : 'Overwrite');
      status.textContent = parts.join(' · ');
    }

    view.addEventListener('scroll', render);
    view.addEventListener('keydown', function (e) {
      const k = e.key;
      const ctrl = e.ctrlKey || e.metaKey;
      if (ctrl && (k === 'a' || k === 'A')) { anchor = 0; caret = bytes.length; nibble = 0; render(); e.preventDefault(); return; }
      if (ctrl && (k === 'z' || k === 'Z')) {
        const u = undo.pop();
        if (u) { bytes = u.bytes; caret = Math.min(u.caret, bytes.length); anchor = null; nibble = 0; changed(); reveal(); }
        e.preventDefault(); return;
      }
      if (ctrl || e.altKey) return;
      const row = PER_ROW;
      switch (k) {
        case 'ArrowLeft': move(nibble ? caret : caret - 1, e.shiftKey); break;
        case 'ArrowRight': move(caret + 1, e.shiftKey); break;
        case 'ArrowUp': move(caret - row, e.shiftKey); break;
        case 'ArrowDown': move(caret + row, e.shiftKey); break;
        case 'PageUp': move(caret - row * visibleRows(), e.shiftKey); break;
        case 'PageDown': move(caret + row * visibleRows(), e.shiftKey); break;
        case 'Home': move(e.ctrlKey ? 0 : caret - (caret % row), e.shiftKey); break;
        case 'End': move(e.ctrlKey ? bytes.length : Math.min(bytes.length, caret - (caret % row) + row - 1), e.shiftKey); break;
        case 'Insert': insert = !insert; render(); break;
        case 'Backspace':
          snapshot();
          if (!deleteSel() && caret > 0) { bytes.splice(caret - 1, 1); caret--; nibble = 0; }
          changed(); reveal(); break;
        case 'Delete':
          snapshot();
          if (!deleteSel() && caret < bytes.length) { bytes.splice(caret, 1); nibble = 0; }
          changed(); reveal(); break;
        default:
          if (k.length !== 1) return;
          if (pane === 'hex') {
            if (!/^[0-9a-fA-F]$/.test(k)) { e.preventDefault(); return; }
            typeNibble(parseInt(k, 16));
          } else {
            insertBytes(new TextEncoder().encode(k));
          }
      }
      e.preventDefault();
    });
    function selected() {
      const r = selRange();
      if (r) return Uint8Array.from(bytes.slice(r[0], r[1]));
      return caret < bytes.length ? Uint8Array.of(bytes[caret]) : new Uint8Array(0);
    }
    view.addEventListener('copy', function (e) {
      const b = selected();
      e.clipboardData.setData('text/plain', pane === 'hex' ? toHex(b) : latin1String(b));
      e.preventDefault();
    });
    view.addEventListener('cut', function (e) {
      const b = selected();
      e.clipboardData.setData('text/plain', pane === 'hex' ? toHex(b) : latin1String(b));
      e.preventDefault();
      if (selRange()) { snapshot(); deleteSel(); changed(); reveal(); }
    });
    view.addEventListener('paste', function (e) {
      e.preventDefault();
      const text = e.clipboardData.getData('text/plain');
      if (!text) return;
      const asHex = pane === 'hex' ? parseHex(text) : null;
      insertBytes(asHex || new TextEncoder().encode(text));
    });
    function byteAt(e) {
      const t = e.target.closest && e.target.closest('[data-i]');
      return t ? { i: Number(t.dataset.i), pane: t.classList.contains('c') ? 'ascii' : 'hex' } : null;
    }
    view.addEventListener('mousedown', function (e) {
      if (e.button !== 0) return;
      const at = byteAt(e);
      if (!at) return;
      e.preventDefault();
      view.focus();
      pane = at.pane;
      if (e.shiftKey) { if (anchor === null) anchor = caret; } else anchor = at.i;
      caret = at.i; nibble = 0;
      dragging = true;
      render();
    });
    view.addEventListener('mousemove', function (e) {
      if (!dragging) return;
      const at = byteAt(e);
      if (at && at.i !== caret) { caret = at.i; render(); }
    });
    document.addEventListener('mouseup', function () {
      if (!dragging) return;
      dragging = false;
      // A selection runs up to and including the byte released on.
      if (anchor !== null && anchor !== caret) {
        if (caret > anchor) caret = Math.min(bytes.length, caret + 1); else anchor = Math.min(bytes.length, anchor + 1);
      } else anchor = null;
      render();
    });

    return {
      setBytes: function (b) { bytes = Array.from(b); caret = 0; nibble = 0; anchor = null; undo = []; insert = false; view.scrollTop = 0; render(); },
      getBytes: function () { return Uint8Array.from(bytes); },
      focus: function () { view.focus(); render(); },
      render: render,
      view: view,
    };
  }

  window.HexEditor = { create: create };
})();
