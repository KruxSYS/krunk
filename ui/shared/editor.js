// Krunk's code editor (0.12.0): a textarea over a coloured copy of its text, for
// krunk://scripts. No third-party code, nothing inline (the pages' CSP), no eval.
//
//   const ed = KrunkEditor.create(container, { value, label, onSave, onChange });
//   ed.getValue() / ed.setValue(text) / ed.focus() / ed.isDirty() / ed.markClean()
//   ed.setError(line, column, message) / ed.clearError()   (1-based)
//   ed.openFind(text?) / ed.closeFind()
//
// The textarea does the typing, selection, undo and scrolling; its text is transparent and a
// <pre> behind it shows the same lines coloured (ui/shared/jstok.js). Only the lines in view,
// plus a margin, are drawn, so long scripts stay fast. Keys: Tab / Shift+Tab indent and
// outdent (the selected lines), Enter keeps the indentation, Ctrl+F finds, Ctrl+S saves.
(function () {
  'use strict';
  // Must match editor.css (.ed-ta padding, line-height).
  const LINE = 20;
  const PAD_Y = 6;
  const MARGIN = 40; // lines drawn above and below the view
  const INDENT = '  ';
  const MAX_MATCHES = 10000;
  const BIG_EDIT_LINES = 300;
  const KINDS = { com: 1, str: 2, kw: 3, num: 4, re: 5, tpl: 6 };
  const CLS = ['', 'tk-com', 'tk-str', 'tk-kw', 'tk-num', 'tk-re', 'tk-tpl'];
  const CLOSE = { '{': '}', '[': ']', '(': ')' };

  function el(tag, cls, attrs) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    Object.keys(attrs || {}).forEach(function (k) {
      if (k === 'text') n.textContent = attrs[k]; else n.setAttribute(k, attrs[k]);
    });
    return n;
  }

  function create(host, opts) {
    opts = opts || {};
    const root = el('div', 'ed');
    const main = el('div', 'ed-main');
    const gutter = el('div', 'ed-gutter', { 'aria-hidden': 'true' });
    const nums = el('div', 'ed-nums');
    gutter.appendChild(nums);
    const body = el('div', 'ed-body');
    const errBar = el('div', 'ed-errbar');
    errBar.hidden = true;
    const pre = el('pre', 'ed-hl', { 'aria-hidden': 'true' });
    const ta = el('textarea', 'ed-ta', {
      wrap: 'off', spellcheck: 'false', autocapitalize: 'off', autocomplete: 'off', autocorrect: 'off',
      'aria-label': opts.label || 'Code', 'aria-multiline': 'true',
    });
    body.append(errBar, pre, ta);
    main.append(gutter, body);

    // The find bar.
    const find = el('div', 'ed-find', { role: 'search' });
    find.hidden = true;
    const findInput = el('input', 'ed-find-input', { type: 'text', placeholder: 'Find', 'aria-label': 'Find in the script', spellcheck: 'false' });
    const findCount = el('span', 'ed-find-count', { 'aria-live': 'polite' });
    const findPrev = el('button', 'ed-find-btn ed-find-prev', { type: 'button', title: 'Previous (Shift+Enter)', 'aria-label': 'Previous match', text: '↑' });
    const findNext = el('button', 'ed-find-btn ed-find-next', { type: 'button', title: 'Next (Enter)', 'aria-label': 'Next match', text: '↓' });
    const findClose = el('button', 'ed-find-btn ed-find-close', { type: 'button', title: 'Close (Esc)', 'aria-label': 'Close find', text: '×' });
    find.append(findInput, findCount, findPrev, findNext, findClose);
    main.appendChild(find);

    const errMsg = el('div', 'ed-error', { role: 'alert' });
    errMsg.hidden = true;
    root.append(main, errMsg);
    host.appendChild(root);

    // ---------------------------------------------------------------- state
    let text = '';
    let clean = '';
    let lineStarts = [0];
    let tok = [];
    let matches = []; // [start, end] pairs
    let cur = -1; // the current match
    let error = null; // {line, column, message} (1-based)
    let drawn = null; // {first, last, version}
    let version = 0;
    let frame = 0;
    let charW = 0;

    function analyse() {
      text = ta.value;
      lineStarts = [0];
      for (let i = text.indexOf('\n'); i >= 0; i = text.indexOf('\n', i + 1)) lineStarts.push(i + 1);
      tok = [];
      window.KrunkJsTok.js(text, 0, text.length, tok, KINDS);
      if (!find.hidden) findAll(false);
      root.style.setProperty('--ed-digits', String(Math.max(2, String(lineStarts.length).length)));
      version++;
    }
    function lineEnd(l) { return l + 1 < lineStarts.length ? lineStarts[l + 1] - 1 : text.length; }
    function lineOf(pos) {
      let lo = 0, hi = lineStarts.length - 1;
      while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (lineStarts[mid] <= pos) lo = mid; else hi = mid - 1; }
      return lo;
    }
    // The first range ending after `pos` in a flat list of `size`-wide records.
    function firstAfter(list, size, pos) {
      let lo = 0, hi = list.length / size;
      while (lo < hi) { const mid = (lo + hi) >> 1; if (list[mid * size + 1] <= pos) lo = mid + 1; else hi = mid; }
      return lo;
    }

    // ---------------------------------------------------------------- drawing
    function schedule() {
      if (!frame) frame = requestAnimationFrame(function () { frame = 0; draw(); });
    }
    function lineContent(row, a, b) {
      let t = firstAfter(tok, 3, a), m = firstAfter(matches, 2, a), pos = a;
      while (pos < b) {
        while (t * 3 < tok.length && tok[t * 3 + 1] <= pos) t++;
        while (m * 2 < matches.length && matches[m * 2 + 1] <= pos) m++;
        let end = b, cls = '', hit = false;
        if (t * 3 < tok.length) {
          if (tok[t * 3] <= pos) { cls = CLS[tok[t * 3 + 2]]; end = Math.min(end, tok[t * 3 + 1]); } else end = Math.min(end, tok[t * 3]);
        }
        if (m * 2 < matches.length) {
          if (matches[m * 2] <= pos) { hit = true; end = Math.min(end, matches[m * 2 + 1]); } else end = Math.min(end, matches[m * 2]);
        }
        if (end <= pos) end = pos + 1;
        const part = text.slice(pos, end);
        if (cls || hit) {
          const s = document.createElement('span');
          s.className = cls + (hit ? (cls ? ' ' : '') + 'ed-match' + (m === cur ? ' ed-match-cur' : '') : '');
          s.textContent = part;
          row.appendChild(s);
        } else row.appendChild(document.createTextNode(part));
        pos = end;
      }
    }
    function draw() {
      const top = ta.scrollTop, h = ta.clientHeight || 400;
      const n = lineStarts.length;
      const first = Math.max(0, Math.floor((top - PAD_Y) / LINE) - MARGIN);
      const last = Math.min(n, Math.ceil((top + h) / LINE) + MARGIN);
      if (!drawn || drawn.first !== first || drawn.last !== last || drawn.version !== version) {
        const lines = document.createDocumentFragment();
        const numbers = document.createDocumentFragment();
        for (let l = first; l < last; l++) {
          const row = document.createElement('div');
          row.className = 'ed-line';
          lineContent(row, lineStarts[l], lineEnd(l));
          lines.appendChild(row);
          const num = document.createElement('div');
          num.className = 'ed-num' + (error && error.line === l + 1 ? ' ed-num-err' : '');
          num.textContent = String(l + 1);
          numbers.appendChild(num);
        }
        pre.textContent = '';
        pre.appendChild(lines);
        nums.textContent = '';
        nums.appendChild(numbers);
        drawn = { first: first, last: last, version: version };
      }
      const y = PAD_Y + first * LINE - top;
      pre.style.transform = 'translate(' + (-ta.scrollLeft) + 'px,' + y + 'px)';
      nums.style.transform = 'translateY(' + y + 'px)';
      if (error) {
        errBar.hidden = false;
        errBar.style.transform = 'translateY(' + (PAD_Y + (error.line - 1) * LINE - top) + 'px)';
      } else errBar.hidden = true;
    }
    function changed() {
      analyse();
      if (error) clearError();
      draw();
      if (opts.onChange) opts.onChange(isDirty());
    }

    // ---------------------------------------------------------------- editing
    // Replaces [start, end) with `str` through the editing commands (so Ctrl+Z undoes it). The
    // engine's text insertion slows down with every line break, so big blocks (indenting a
    // whole long script) are replaced directly, without an undo step.
    function replace(start, end, str, selStart, selEnd) {
      ta.focus();
      ta.setSelectionRange(start, end);
      let breaks = 0;
      for (let i = str.indexOf('\n'); i >= 0 && breaks <= BIG_EDIT_LINES; i = str.indexOf('\n', i + 1)) breaks++;
      const done = breaks > BIG_EDIT_LINES ? false
        : str === '' ? document.execCommand('delete', false) : document.execCommand('insertText', false, str);
      if (!done) {
        ta.setRangeText(str, start, end, 'end');
        changed();
      }
      ta.setSelectionRange(selStart, selEnd === undefined ? selStart : selEnd);
    }
    function lineStartAt(v, pos) { return v.lastIndexOf('\n', pos - 1) + 1; }
    function lineEndAt(v, pos) { const e = v.indexOf('\n', pos); return e < 0 ? v.length : e; }
    function indent(out) {
      const v = ta.value, s = ta.selectionStart, e = ta.selectionEnd;
      const multi = v.slice(s, e).indexOf('\n') >= 0;
      if (!out && !multi) { replace(s, e, INDENT, s + INDENT.length); return; }
      const ls = lineStartAt(v, s);
      const le = lineEndAt(v, multi && e > s && v[e - 1] === '\n' ? e - 1 : e);
      const lines = v.slice(ls, le).split('\n');
      let firstDelta = 0, total = 0;
      const next = lines.map(function (l, i) {
        let r;
        if (out) {
          const m = /^( {1,2}|\t)/.exec(l);
          r = m ? l.slice(m[0].length) : l;
        } else r = l.length ? INDENT + l : l;
        const d = r.length - l.length;
        if (i === 0) firstDelta = d;
        total += d;
        return r;
      });
      const block = next.join('\n');
      if (block === v.slice(ls, le)) return;
      const ns = Math.max(ls, s + firstDelta);
      replace(ls, le, block, ns, Math.max(ns, e + total));
    }
    function newline() {
      const v = ta.value, s = ta.selectionStart, e = ta.selectionEnd;
      const ls = lineStartAt(v, s);
      const lead = /^[ \t]*/.exec(v.slice(ls, s))[0];
      const before = v.slice(ls, s).replace(/\s+$/, '');
      const open = before.slice(-1);
      let ins = '\n' + lead;
      if (CLOSE[open]) {
        ins += INDENT;
        const after = v.slice(e, lineEndAt(v, e)).replace(/^[ \t]+/, '');
        if (after[0] === CLOSE[open]) {
          replace(s, e, ins + '\n' + lead, s + ins.length);
          return;
        }
      }
      replace(s, e, ins, s + ins.length);
    }

    // ---------------------------------------------------------------- find
    function findAll(pick) {
      const q = findInput.value;
      matches = [];
      if (q) {
        const hay = text.toLowerCase(), needle = q.toLowerCase();
        for (let i = hay.indexOf(needle); i >= 0 && matches.length < MAX_MATCHES * 2; i = hay.indexOf(needle, i + needle.length)) {
          matches.push(i, i + needle.length);
        }
      }
      const count = matches.length / 2;
      if (!count) cur = -1;
      else if (pick || cur < 0 || cur >= count) {
        const from = ta.selectionStart;
        cur = 0;
        for (let i = 0; i < count; i++) if (matches[i * 2] >= from) { cur = i; break; }
      }
      findCount.textContent = !q ? '' : count ? (cur + 1) + ' of ' + count.toLocaleString() + (count >= MAX_MATCHES ? '+' : '') : 'No results';
      findCount.classList.toggle('none', !!q && !count);
    }
    function scrollToPos(pos) {
      const l = lineOf(pos);
      const y = PAD_Y + l * LINE, h = ta.clientHeight;
      if (y < ta.scrollTop + LINE || y + LINE > ta.scrollTop + h - LINE) ta.scrollTop = Math.max(0, y - h / 3);
      if (!charW) {
        const probe = el('span', 'ed-probe', { text: 'x'.repeat(100) });
        pre.appendChild(probe);
        charW = probe.getBoundingClientRect().width / 100 || 8;
        probe.remove();
      }
      const x = (pos - lineStarts[l]) * charW;
      const w = ta.clientWidth;
      if (x < ta.scrollLeft || x > ta.scrollLeft + w - 40) ta.scrollLeft = Math.max(0, x - w / 2);
    }
    function showMatch() {
      if (cur < 0) return;
      const a = matches[cur * 2], b = matches[cur * 2 + 1];
      ta.setSelectionRange(a, b);
      scrollToPos(a);
      drawn = null;
      draw();
    }
    function step(dir) {
      if (find.hidden) { openFind(); return; }
      if (!matches.length) return;
      const count = matches.length / 2;
      cur = (cur + dir + count) % count;
      findCount.textContent = (cur + 1) + ' of ' + count.toLocaleString();
      showMatch();
    }
    function openFind(q) {
      const sel = ta.value.slice(ta.selectionStart, ta.selectionEnd);
      if (q === undefined && sel && sel.indexOf('\n') < 0) q = sel;
      find.hidden = false;
      if (q !== undefined) findInput.value = q;
      findInput.focus();
      findInput.select();
      findAll(true);
      showMatch();
      draw();
    }
    function closeFind() {
      if (find.hidden) return;
      find.hidden = true;
      matches = [];
      cur = -1;
      drawn = null;
      draw();
      ta.focus();
    }
    findInput.addEventListener('input', function () { findAll(true); showMatch(); drawn = null; draw(); });
    findInput.addEventListener('keydown', function (e) {
      if (e.key === 'Enter') { e.preventDefault(); step(e.shiftKey ? -1 : 1); } else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeFind(); }
    });
    findPrev.addEventListener('click', function () { step(-1); });
    findNext.addEventListener('click', function () { step(1); });
    findClose.addEventListener('click', closeFind);

    // ---------------------------------------------------------------- keys
    ta.addEventListener('keydown', function (e) {
      if (e.isComposing) return;
      const mod = e.ctrlKey || e.metaKey || e.altKey;
      if (e.key === 'Tab' && !mod) { e.preventDefault(); indent(e.shiftKey); } else if (e.key === 'Enter' && !mod && !e.shiftKey) { e.preventDefault(); newline(); } else if (e.key === 'Escape' && !find.hidden) { e.preventDefault(); e.stopPropagation(); closeFind(); }
    });
    root.addEventListener('keydown', function (e) {
      const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey;
      const k = e.key.toLowerCase();
      if (ctrl && k === 's') { e.preventDefault(); if (opts.onSave) opts.onSave(); } else if (ctrl && k === 'f') { e.preventDefault(); openFind(); } else if (e.key === 'F3') { e.preventDefault(); step(e.shiftKey ? -1 : 1); }
    });
    ta.addEventListener('input', changed);
    ta.addEventListener('scroll', schedule);
    if (window.ResizeObserver) new ResizeObserver(schedule).observe(body);

    // ---------------------------------------------------------------- errors
    function setError(line, column, message) {
      const l = Math.max(1, Math.min(lineStarts.length, Number(line) || 1));
      const c = Math.max(1, Number(column) || 1);
      error = { line: l, column: c, message: String(message || 'Syntax error') };
      errMsg.textContent = 'Line ' + l + ', column ' + c + ': ' + error.message;
      errMsg.hidden = false;
      root.classList.add('has-error');
      const pos = Math.min(lineEnd(l - 1), lineStarts[l - 1] + c - 1);
      ta.setSelectionRange(pos, pos);
      scrollToPos(pos);
      drawn = null;
      draw();
    }
    function clearError() {
      if (!error) return;
      error = null;
      errMsg.hidden = true;
      errMsg.textContent = '';
      root.classList.remove('has-error');
      drawn = null;
      draw();
    }

    // ---------------------------------------------------------------- API
    function getValue() { return ta.value; }
    function setValue(v) {
      ta.value = String(v === undefined || v === null ? '' : v);
      clean = ta.value;
      error = null;
      errMsg.hidden = true;
      root.classList.remove('has-error');
      ta.scrollTop = 0;
      ta.scrollLeft = 0;
      ta.setSelectionRange(0, 0);
      analyse();
      drawn = null;
      draw();
      if (opts.onChange) opts.onChange(false);
    }
    function isDirty() { return ta.value !== clean; }
    function markClean() { clean = ta.value; if (opts.onChange) opts.onChange(false); }

    setValue(opts.value || '');
    return {
      element: root, textarea: ta,
      getValue: getValue, setValue: setValue, focus: function () { ta.focus(); },
      isDirty: isDirty, markClean: markClean,
      setError: setError, clearError: clearError,
      openFind: openFind, closeFind: closeFind,
      refresh: function () { drawn = null; draw(); },
    };
  }

  window.KrunkEditor = { create: create };
})();
