// krunk://source (0.11.0): a page's source, with line numbers, colours and two options:
// Beautify (re-indents HTML by its nesting and formats inline scripts and styles) and Wrap
// lines; both are remembered. The host loads the address in the page's own URL again, the way
// the tab did (cache first); a page that cannot be loaded again shows the tab's current
// document, and the viewer says so.
//
// Large sources stay smooth: lines come in chunks of 200 that layout skips while out of view
// (content-visibility); their colours are added when a chunk comes near the screen. Up to
// 20,000 lines all the text is in the page at once, so Ctrl+F finds anything.
(function () {
  'use strict';
  const box = document.getElementById('src');
  const optWrap = document.getElementById('opt-wrap');
  const optBeautify = document.getElementById('opt-beautify');
  const CHUNK = 200;
  const LINE_PX = 18;
  const EAGER_LINES = 20000;

  // Token kinds (CSS classes).
  const K = { tag: 1, attr: 2, val: 3, com: 4, doc: 5, ent: 6, kw: 7, str: 8, num: 9, prop: 10 };
  const CLS = ['', 'h-tag', 'h-attr', 'h-val', 'h-com', 'h-doc', 'h-ent', 'h-kw', 'h-str', 'h-num', 'h-prop'];
  const LINK_ATTRS = new Set(['href', 'src', 'action', 'poster', 'srcset', 'data']);
  const JS_KW = new Set(('break case catch class const continue debugger default delete do else export extends false finally for function if import in instanceof let new null return super switch this throw true try typeof undefined var void while with yield async await of static get set').split(' '));
  const VOID = new Set('area base br col embed hr img input link meta param source track wbr'.split(' '));
  const INLINE = new Set(('a abbr b bdi bdo br cite code data dfn em i img kbd label mark q s samp small span strong sub sup time u var wbr ' +
    'input button select textarea').split(' '));

  let meta = null; // {url, from, status, mime, text}

  // ---------------------------------------------------------------- tokenizers
  // Each pushes [start, end, kind] ranges (sorted, not overlapping) into `out`.
  function isWs(c) { return c === 32 || c === 9 || c === 10 || c === 13 || c === 12; }
  function isIdStart(c) { return (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 36 || c === 95 || c > 127; }
  function isId(c) { return isIdStart(c) || (c >= 48 && c <= 57); }
  function isDigit(c) { return c >= 48 && c <= 57; }

  const ENT = /&(?:#\d+|#x[0-9a-f]+|[a-z][a-z0-9]{1,31});/gi;
  function entities(s, a, b, out) {
    if (s.indexOf('&', a) < 0) return;
    ENT.lastIndex = a;
    let m;
    while ((m = ENT.exec(s)) && m.index < b) {
      if (m.index + m[0].length > b) break;
      out.push(m.index, m.index + m[0].length, K.ent);
    }
  }
  const TAGRE = /<\/?([A-Za-z][\w:.-]*)/y;
  function tokHtml(s, out, links) {
    const n = s.length;
    let i = 0;
    while (i < n) {
      const lt = s.indexOf('<', i);
      if (lt < 0) { entities(s, i, n, out); break; }
      entities(s, i, lt, out);
      if (s.startsWith('<!--', lt)) {
        let e = s.indexOf('-->', lt + 4);
        e = e < 0 ? n : e + 3;
        out.push(lt, e, K.com); i = e; continue;
      }
      const c1 = s.charCodeAt(lt + 1);
      if (c1 === 33 || c1 === 63) { // <! or <?
        let e = s.indexOf('>', lt);
        e = e < 0 ? n : e + 1;
        out.push(lt, e, K.doc); i = e; continue;
      }
      TAGRE.lastIndex = lt;
      const m = TAGRE.exec(s);
      if (!m) { i = lt + 1; continue; }
      const closing = c1 === 47;
      const name = m[1].toLowerCase();
      out.push(lt, lt + m[0].length, K.tag);
      let j = lt + m[0].length, self = false;
      while (j < n) {
        const c = s.charCodeAt(j);
        if (isWs(c)) { j++; continue; }
        if (c === 62) { out.push(j, j + 1, K.tag); j++; break; }
        if (c === 47 && s.charCodeAt(j + 1) === 62) { out.push(j, j + 2, K.tag); j += 2; self = true; break; }
        if (c === 60) break; // a broken tag: the next one starts
        const ns = j;
        while (j < n) {
          const d = s.charCodeAt(j);
          if (isWs(d) || d === 61 || d === 62 || d === 60 || (d === 47 && s.charCodeAt(j + 1) === 62)) break;
          j++;
        }
        if (j === ns) { j++; continue; }
        out.push(ns, j, K.attr);
        const aname = s.slice(ns, j).toLowerCase();
        let k = j;
        while (k < n && isWs(s.charCodeAt(k))) k++;
        if (s.charCodeAt(k) !== 61) continue;
        k++;
        while (k < n && isWs(s.charCodeAt(k))) k++;
        const q = s.charCodeAt(k);
        let vs = k, ve;
        if (q === 34 || q === 39) {
          ve = s.indexOf(String.fromCharCode(q), k + 1);
          ve = ve < 0 ? n : ve + 1;
        } else {
          ve = k;
          while (ve < n && !isWs(s.charCodeAt(ve)) && s.charCodeAt(ve) !== 62) ve++;
        }
        out.push(vs, ve, K.val);
        if (links && LINK_ATTRS.has(aname) && aname !== 'srcset') links.add(out.length - 3);
        j = ve;
      }
      i = j;
      if (closing || self) continue;
      if (name === 'script' || name === 'style' || name === 'textarea' || name === 'title') {
        const re = new RegExp('</' + name + '\\s*>', 'ig');
        re.lastIndex = i;
        const em = re.exec(s);
        const end = em ? em.index : n;
        if (name === 'script') tokJs(s, i, end, out);
        else if (name === 'style') tokCss(s, i, end, out);
        else entities(s, i, end, out); // title and textarea: text with entities, no tags
        i = end;
      }
    }
  }
  function tokString(s, i, b) {
    const q = s.charCodeAt(i);
    let j = i + 1;
    while (j < b) {
      const c = s.charCodeAt(j);
      if (c === 92) { j += 2; continue; }
      if (c === q) return j + 1;
      if (c === 10 && q !== 96) return j;
      j++;
    }
    return b;
  }
  function tokJs(s, a, b, out) {
    let i = a;
    while (i < b) {
      const c = s.charCodeAt(i);
      if (c === 47 && s.charCodeAt(i + 1) === 47) {
        let e = s.indexOf('\n', i);
        if (e < 0 || e > b) e = b;
        out.push(i, e, K.com); i = e; continue;
      }
      if (c === 47 && s.charCodeAt(i + 1) === 42) {
        let e = s.indexOf('*/', i + 2);
        e = e < 0 || e > b ? b : e + 2;
        out.push(i, e, K.com); i = e; continue;
      }
      if (c === 34 || c === 39 || c === 96) { const e = tokString(s, i, b); out.push(i, e, K.str); i = e; continue; }
      if (isDigit(c) && !(i > a && isId(s.charCodeAt(i - 1)))) {
        let e = i + 1;
        while (e < b && (isId(s.charCodeAt(e)) || s.charCodeAt(e) === 46)) e++;
        out.push(i, e, K.num); i = e; continue;
      }
      if (isIdStart(c)) {
        let e = i + 1;
        while (e < b && isId(s.charCodeAt(e))) e++;
        if (JS_KW.has(s.slice(i, e)) && s.charCodeAt(i - 1) !== 46) out.push(i, e, K.kw);
        i = e; continue;
      }
      i++;
    }
  }
  function tokCss(s, a, b, out) {
    let i = a, depth = 0;
    while (i < b) {
      const c = s.charCodeAt(i);
      if (c === 47 && s.charCodeAt(i + 1) === 42) {
        let e = s.indexOf('*/', i + 2);
        e = e < 0 || e > b ? b : e + 2;
        out.push(i, e, K.com); i = e; continue;
      }
      if (c === 34 || c === 39) { const e = tokString(s, i, b); out.push(i, e, K.str); i = e; continue; }
      if (c === 123) { depth++; i++; continue; }
      if (c === 125) { depth = Math.max(0, depth - 1); i++; continue; }
      if (c === 64) { // @media …
        let e = i + 1;
        while (e < b && (isId(s.charCodeAt(e)) || s.charCodeAt(e) === 45)) e++;
        out.push(i, e, K.kw); i = e; continue;
      }
      if (depth > 0 && (isIdStart(c) || c === 45)) {
        let e = i + 1;
        while (e < b && (isId(s.charCodeAt(e)) || s.charCodeAt(e) === 45)) e++;
        let k = e;
        while (k < b && isWs(s.charCodeAt(k))) k++;
        if (s.charCodeAt(k) === 58) out.push(i, e, K.prop);
        i = e; continue;
      }
      if (depth > 0 && (isDigit(c) || c === 35)) {
        let e = i + 1;
        while (e < b && (isId(s.charCodeAt(e)) || s.charCodeAt(e) === 46 || s.charCodeAt(e) === 37)) e++;
        out.push(i, e, K.num); i = e; continue;
      }
      i++;
    }
  }

  // ---------------------------------------------------------------- beautify
  // Brace languages (scripts and styles): a line per statement, nesting by braces. Strings,
  // comments and template literals are copied as they are.
  function beautifyCode(code, css) {
    const lines = [];
    let line = '', depth = 0, paren = 0;
    const push = function () { const t = line.trim(); if (t) lines.push('  '.repeat(Math.max(0, depth)) + t); line = ''; };
    const n = code.length;
    for (let i = 0; i < n; i++) {
      const ch = code[i], c = code.charCodeAt(i);
      if (c === 47 && code.charCodeAt(i + 1) === 42) {
        let e = code.indexOf('*/', i + 2); e = e < 0 ? n : e + 2;
        line += code.slice(i, e); i = e - 1; continue;
      }
      if (!css && c === 47 && code.charCodeAt(i + 1) === 47) {
        let e = code.indexOf('\n', i); if (e < 0) e = n;
        line += code.slice(i, e); push(); i = e; continue;
      }
      if (c === 34 || c === 39 || (!css && c === 96)) { const e = tokString(code, i, n); line += code.slice(i, e); i = e - 1; continue; }
      if (ch === '\n' || ch === '\r') { if (paren === 0) push(); else line += ' '; continue; }
      if (isWs(c)) { if (line && !line.endsWith(' ')) line += ' '; continue; }
      if (ch === '(' || ch === '[') paren++;
      if (ch === ')' || ch === ']') paren = Math.max(0, paren - 1);
      if (ch === '{') { line += '{'; push(); depth++; continue; }
      if (ch === '}') {
        push(); depth = Math.max(0, depth - 1); line = '}';
        let k = i + 1; while (k < n && isWs(code.charCodeAt(k)) && code[k] !== '\n') k++;
        if (!/[,;)\].]/.test(code[k] || '') && !(code.startsWith('else', k) || code.startsWith('catch', k) || code.startsWith('finally', k) || code.startsWith('while', k))) push();
        continue;
      }
      line += ch;
      if (ch === ';' && paren === 0) push();
    }
    push();
    return lines;
  }
  // HTML: block elements on their own lines, indented by nesting; inline ones stay in their
  // text; <pre> and <textarea> as they are; scripts and styles formatted inside.
  function beautifyHtml(s) {
    const out = [];
    let level = 0, line = '';
    const ind = function () { return '  '.repeat(level); };
    const flush = function () { const t = line.trim(); if (t) out.push(ind() + t); line = ''; };
    const n = s.length;
    let i = 0;
    while (i < n) {
      const lt = s.indexOf('<', i);
      const textEnd = lt < 0 ? n : lt;
      if (textEnd > i) {
        const text = s.slice(i, textEnd).replace(/\s+/g, ' ');
        if (text !== ' ' || line) line += text;
      }
      if (lt < 0) break;
      if (s.startsWith('<!--', lt)) {
        let e = s.indexOf('-->', lt + 4); e = e < 0 ? n : e + 3;
        flush(); out.push(ind() + s.slice(lt, e).replace(/\n\s*/g, ' ')); i = e; continue;
      }
      // The tag's end: the first > outside quotes.
      let j = lt + 1, q = 0;
      while (j < n) {
        const c = s.charCodeAt(j);
        if (q) { if (c === q) q = 0; } else if (c === 34 || c === 39) q = c; else if (c === 62) break;
        j++;
      }
      const tag = s.slice(lt, Math.min(n, j + 1)).replace(/\s*\n\s*/g, ' ');
      i = Math.min(n, j + 1);
      const m = /^<(\/?)([A-Za-z][\w:.-]*)/.exec(tag);
      if (!m) {
        if (/^<[!?]/.test(tag)) { flush(); out.push(ind() + tag); } else line += tag;
        continue;
      }
      const closing = m[1] === '/', name = m[2].toLowerCase();
      const self = /\/>$/.test(tag) || VOID.has(name);
      if (INLINE.has(name) && name !== 'textarea') { line += tag; continue; }
      flush();
      if (closing) { level = Math.max(0, level - 1); out.push(ind() + tag); continue; }
      out.push(ind() + tag);
      if (self) continue;
      if (name === 'script' || name === 'style' || name === 'pre' || name === 'textarea') {
        const re = new RegExp('</' + name + '\\s*>', 'ig');
        re.lastIndex = i;
        const em = re.exec(s);
        const end = em ? em.index : n;
        const body = s.slice(i, end);
        if (name === 'pre' || name === 'textarea') {
          // As it is: its spaces and line breaks are its content.
          out[out.length - 1] += body + (em ? em[0] : '');
          i = em ? end + em[0].length : n;
          continue;
        }
        level++;
        const css = name === 'style';
        const isCode = css || !/type=["']?(text\/(html|template|x-template)|application\/(ld\+)?json)/i.test(tag);
        const lines = isCode ? beautifyCode(body, css) : body.split('\n').map(function (l) { return l.trim(); }).filter(Boolean);
        lines.forEach(function (l) { out.push(ind() + l); });
        level--;
        if (em) { out.push(ind() + em[0]); i = end + em[0].length; } else i = n;
        continue;
      }
      level++;
    }
    flush();
    return out.join('\n');
  }

  // ---------------------------------------------------------------- rendering
  let text = '', lineStarts = [], tok = [], links = new Set(), kind = 'html';
  function kindOf(mime, url) {
    const m = (mime || '').toLowerCase();
    if (/javascript|ecmascript|json/.test(m) || /\.(m?js|json)(\?|$)/i.test(url)) return 'js';
    if (/css/.test(m) || /\.css(\?|$)/i.test(url)) return 'css';
    if (/html|xml|svg/.test(m) || !m) return 'html';
    return 'text';
  }
  function prepare() {
    const src = meta.text.replace(/\r\n?/g, '\n');
    kind = kindOf(meta.mime, meta.url);
    text = !optBeautify.checked ? src
      : kind === 'html' ? beautifyHtml(src)
        : kind === 'js' || kind === 'css' ? beautifyCode(src, kind === 'css').join('\n') : src;
    tok = []; links = new Set();
    if (kind === 'html') tokHtml(text, tok, links);
    else if (kind === 'js') tokJs(text, 0, text.length, tok);
    else if (kind === 'css') tokCss(text, 0, text.length, tok);
    lineStarts = [0];
    for (let i = text.indexOf('\n'); i >= 0; i = text.indexOf('\n', i + 1)) lineStarts.push(i + 1);
  }
  function lineEnd(l) { return l + 1 < lineStarts.length ? lineStarts[l + 1] - 1 : text.length; }
  // The first token that ends after `pos` (tokens are [start, end, kind] triples).
  function firstToken(pos) {
    let lo = 0, hi = tok.length / 3;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (tok[mid * 3 + 1] <= pos) lo = mid + 1; else hi = mid; }
    return lo;
  }
  function linkFor(t, a, b) {
    if (!links.has(t * 3)) return null;
    const raw = text.slice(a, b).replace(/^["']|["']$/g, '').replace(/&amp;/g, '&').trim();
    try {
      const u = new URL(raw, meta.url);
      if (u.protocol !== 'https:' && u.protocol !== 'http:') return null;
      return 'source.html?url=' + encodeURIComponent(u.href);
    } catch (e) { return null; }
  }
  function fillPlain(chunk) {
    const first = chunk._first, last = Math.min(lineStarts.length, first + CHUNK);
    const frag = document.createDocumentFragment();
    for (let l = first; l < last; l++) {
      const row = document.createElement('div');
      row.className = 'ln';
      row.dataset.n = String(l + 1);
      const t = document.createElement('span');
      t.className = 't';
      t.textContent = text.slice(lineStarts[l], lineEnd(l));
      row.appendChild(t);
      frag.appendChild(row);
    }
    chunk.textContent = '';
    chunk.appendChild(frag);
    chunk._filled = true;
  }
  function highlight(chunk) {
    if (!chunk._filled) fillPlain(chunk);
    chunk._lit = true;
    if (!tok.length) return;
    const first = chunk._first;
    let t = firstToken(lineStarts[first]);
    const rows = chunk.children;
    for (let r = 0; r < rows.length; r++) {
      const l = first + r, a = lineStarts[l], b = lineEnd(l);
      if (t * 3 >= tok.length || tok[t * 3] >= b) continue;
      const span = rows[r].firstChild;
      const frag = document.createDocumentFragment();
      let pos = a;
      while (t * 3 < tok.length && tok[t * 3] < b) {
        const ts = Math.max(a, tok[t * 3]), te = Math.min(b, tok[t * 3 + 1]);
        if (ts > pos) frag.appendChild(document.createTextNode(text.slice(pos, ts)));
        if (te > ts) {
          const s = document.createElement('span');
          s.className = CLS[tok[t * 3 + 2]];
          const href = tok[t * 3 + 2] === K.val ? linkFor(t, tok[t * 3], tok[t * 3 + 1]) : null;
          if (href) { const link = document.createElement('a'); link.href = href; link.textContent = text.slice(ts, te); s.appendChild(link); }
          else s.textContent = text.slice(ts, te);
          frag.appendChild(s);
        }
        pos = Math.max(pos, te);
        if (tok[t * 3 + 1] <= b) t++; else break;
      }
      if (pos < b) frag.appendChild(document.createTextNode(text.slice(pos, b)));
      span.textContent = '';
      span.appendChild(frag);
    }
  }
  const near = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      if (!e.isIntersecting) return;
      if (!e.target._lit) highlight(e.target);
      near.unobserve(e.target);
    });
  }, { root: box, rootMargin: '1500px 0px' });

  function render() {
    const keep = box.scrollTop / Math.max(1, box.scrollHeight);
    prepare();
    near.disconnect();
    box.textContent = '';
    const lines = lineStarts.length;
    box.style.setProperty('--gutter', (String(lines).length + 1) + 'ch');
    const eager = lines <= EAGER_LINES;
    for (let first = 0; first < lines; first += CHUNK) {
      const chunk = document.createElement('div');
      chunk.className = 'chunk';
      chunk._first = first;
      const rows = Math.min(CHUNK, lines - first);
      chunk.style.containIntrinsicSize = 'auto ' + (rows * LINE_PX) + 'px';
      if (eager) fillPlain(chunk); else chunk.style.minHeight = (rows * LINE_PX) + 'px';
      box.appendChild(chunk);
      near.observe(chunk);
    }
    box.scrollTop = keep * box.scrollHeight;
    info();
  }
  function size(n) { return n < 1024 ? n + ' B' : n < 1048576 ? (n / 1024).toFixed(1) + ' KB' : (n / 1048576).toFixed(1) + ' MB'; }
  function info() {
    const from = { cache: 'from the cache', network: 'loaded again', page: "the tab's current document" }[meta.from] || '';
    const status = meta.status && (meta.status < 200 || meta.status >= 300) ? 'HTTP ' + meta.status + ' · ' : '';
    document.getElementById('src-info').textContent = status + lineStarts.length.toLocaleString() + ' lines · ' + size(meta.text.length) + (from ? ' · ' + from : '');
  }

  // ---------------------------------------------------------------- options
  function pref(name, fallback) { try { const v = localStorage.getItem('krunk.source.' + name); return v === null ? fallback : v === '1'; } catch (e) { return fallback; } }
  function savePref(name, on) { try { localStorage.setItem('krunk.source.' + name, on ? '1' : '0'); } catch (e) { /* not kept */ } }
  optWrap.checked = pref('wrap', false);
  optBeautify.checked = pref('beautify', false);
  box.classList.toggle('wrap', optWrap.checked);
  optWrap.addEventListener('change', function () { box.classList.toggle('wrap', optWrap.checked); savePref('wrap', optWrap.checked); });
  optBeautify.addEventListener('change', function () { savePref('beautify', optBeautify.checked); if (meta) render(); });

  LB.request('sourceGet').then(function (r) {
    meta = r;
    document.getElementById('src-url').textContent = r.url;
    document.getElementById('src-url').title = r.url;
    document.title = 'view-source:' + r.url;
    if (r.from === 'page') {
      const note = document.getElementById('src-note');
      note.textContent = "This page could not be loaded again (for example, the result of a form). Shown: the document as the tab has it now, after the page's scripts changed it.";
      note.hidden = false;
    }
    render();
    box.focus();
  }, function (e) {
    const err = document.getElementById('src-error');
    err.textContent = 'The source could not be loaded: ' + e.message;
    err.hidden = false;
  });
  LB.post('ready');
  // For the tests: the formatter and tokenizer on their own.
  window.__krunkSource = { beautifyHtml: beautifyHtml, beautifyCode: beautifyCode, tokHtml: tokHtml };
})();
