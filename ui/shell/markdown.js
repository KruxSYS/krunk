// A small Markdown renderer for the agent panel (0.12.0). The model's text becomes DOM nodes
// built with createElement + textContent: never innerHTML, so raw HTML in an answer
// (`<img src=x onerror=…>`) is shown as text. Links work only for http(s) addresses and go
// through `opts.onLink(url)`; images are never loaded (shown as links).
//
// Supported: paragraphs with line breaks, # headings, **bold**, *italic*, ~~strike~~, `code`,
// fenced code blocks, bullet and numbered lists (nested by indent), > quotes, --- rules, simple
// tables, [text](url) links, <https://…> and bare https:// addresses.
//
// KrunkMarkdown.render(text, opts) -> DocumentFragment
// KrunkMarkdown.stableCut(text, from) -> index: while a message streams in, the text before the
//   last blank line outside a code block will not change its rendering much; the panel renders
//   that part once and re-renders only the tail.
(function () {
  'use strict';
  const MAX_DEPTH = 6;
  const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^`\s]*)/;
  const HEADING = /^ {0,3}(#{1,6})(?:\s+(.*?))?\s*#*\s*$/;
  const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
  const QUOTE = /^ {0,3}> ?/;
  const ITEM = /^( *)([-*+]|\d{1,9}[.)])(?:\s+(.*))?$/;
  const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

  function el(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text !== undefined) n.textContent = text;
    return n;
  }

  function isHttp(url) {
    return /^https?:\/\/[^\s/?#]+/i.test(url);
  }

  function link(url, children, opts) {
    const a = el('a');
    a.href = url;
    a.title = url;
    a.rel = 'noopener noreferrer';
    children.forEach(function (c) { a.appendChild(c); });
    a.addEventListener('click', function (e) {
      e.preventDefault();
      if (opts.onLink) opts.onLink(url);
    });
    // A middle click would open a window of the panel itself: route it the same way.
    a.addEventListener('auxclick', function (e) {
      if (e.button !== 1) return;
      e.preventDefault();
      if (opts.onLink) opts.onLink(url);
    });
    return a;
  }

  // ---------- inline ----------

  // Inline text -> nodes. `memo` remembers delimiters with no closer further on (keeps long
  // unbalanced text linear).
  function inline(text, opts, depth, noLinks) {
    const out = [];
    let buf = '';
    const failed = Object.create(null);
    function flush() {
      if (!buf) return;
      const parts = buf.split('\n');
      parts.forEach(function (p, i) {
        if (i) out.push(el('br'));
        if (p) out.push(document.createTextNode(p));
      });
      buf = '';
    }
    function push(node) { flush(); out.push(node); }
    function closer(delim, from) {
      if (failed[delim] !== undefined && from >= failed[delim]) return -1;
      const j = text.indexOf(delim, from);
      if (j < 0) failed[delim] = from;
      return j;
    }
    let i = 0;
    while (i < text.length) {
      const c = text[i];
      // Backslash escapes.
      if (c === '\\' && i + 1 < text.length && /[!-/:-@[-`{-~]/.test(text[i + 1])) {
        buf += text[i + 1];
        i += 2;
        continue;
      }
      // Code spans: a run of n backticks up to the next run of exactly n.
      if (c === '`') {
        let n = 1;
        while (text[i + n] === '`') n++;
        const fence = '`'.repeat(n);
        let j = closer(fence, i + n);
        while (j >= 0 && (text[j + n] === '`' || text[j - 1] === '`')) j = text.indexOf(fence, j + 1);
        if (j >= 0) {
          let code = text.slice(i + n, j).replace(/\n/g, ' ');
          if (code.length > 2 && code[0] === ' ' && code[code.length - 1] === ' ') code = code.slice(1, -1);
          push(el('code', null, code));
          i = j + n;
          continue;
        }
        buf += fence;
        i += n;
        continue;
      }
      // Links [text](url) and images ![alt](url) (shown as links: nothing is loaded).
      if (!noLinks && (c === '[' || (c === '!' && text[i + 1] === '['))) {
        const img = c === '!';
        const m = parseLink(text, img ? i + 1 : i);
        if (m) {
          const label = m.label || (img ? 'image' : m.url);
          if (isHttp(m.url) && depth < MAX_DEPTH) push(link(m.url, inline(label, opts, depth + 1, true), opts));
          else inline(label, opts, depth + 1, true).forEach(push);
          i = m.end;
          continue;
        }
      }
      // <https://…>
      if (!noLinks && c === '<') {
        const m = /^<(https?:\/\/[^\s<>]+)>/i.exec(text.slice(i, i + 2048));
        if (m) {
          push(link(m[1], [document.createTextNode(m[1])], opts));
          i += m[0].length;
          continue;
        }
      }
      // Bare http(s) addresses at a word start.
      if (!noLinks && (c === 'h' || c === 'H') && (i === 0 || /[\s(\[{"'*_]/.test(text[i - 1]))) {
        const m = /^https?:\/\/[^\s<>"]+/i.exec(text.slice(i, i + 2048));
        if (m) {
          let url = m[0];
          // Trailing punctuation belongs to the sentence; a ")" only if unbalanced.
          for (;;) {
            const last = url[url.length - 1];
            if (/[.,;:!?'"*_]/.test(last)) url = url.slice(0, -1);
            else if (last === ')' && (url.split('(').length < url.split(')').length)) url = url.slice(0, -1);
            else break;
          }
          if (isHttp(url)) {
            push(link(url, [document.createTextNode(url)], opts));
            i += url.length;
            continue;
          }
        }
      }
      // Emphasis: ** __ (bold), * _ (italic), ~~ (strike).
      // ***both*** (bold and italic).
      if (c === '*' && text.startsWith('***', i) && /\S/.test(text[i + 3] || ' ') && text[i + 3] !== '*' && depth < MAX_DEPTH) {
        const j = closer('***', i + 4);
        if (j >= 0 && !/\s/.test(text[j - 1])) {
          const b = el('strong');
          const e = el('em');
          inline(text.slice(i + 3, j), opts, depth + 1, noLinks).forEach(function (x) { e.appendChild(x); });
          b.appendChild(e);
          push(b);
          i = j + 3;
          continue;
        }
      }
      if ((c === '*' || c === '_' || c === '~') && depth < MAX_DEPTH) {
        const two = text[i + 1] === c;
        if (c === '~' && !two) { buf += c; i++; continue; }
        const delim = two ? c + c : c;
        const after = text[i + delim.length];
        const before = i > 0 ? text[i - 1] : ' ';
        // "_" inside a word (snake_case) is text; an opener must touch its content.
        const opens = after !== undefined && !/\s/.test(after) && after !== c && !(c === '_' && /\w/.test(before));
        if (opens) {
          let j = closer(delim, i + delim.length + 1);
          // A closer must touch its content; "_" must end a word.
          while (j >= 0 && (/\s/.test(text[j - 1]) || text[j - 1] === c || text[j + delim.length] === c || (c === '_' && /\w/.test(text[j + delim.length] || '')))) {
            j = text.indexOf(delim, j + 1);
          }
          if (j >= 0) {
            const tag = c === '~' ? 's' : two ? 'strong' : 'em';
            const n = el(tag);
            inline(text.slice(i + delim.length, j), opts, depth + 1, noLinks).forEach(function (x) { n.appendChild(x); });
            push(n);
            i = j + delim.length;
            continue;
          }
        }
        buf += delim;
        i += delim.length;
        continue;
      }
      buf += c;
      i++;
    }
    flush();
    return out;
  }

  // "[label](url "title")" at `i` -> {label, url, end} or null.
  function parseLink(text, i) {
    if (text[i] !== '[') return null;
    let depth = 0;
    let j = i;
    const limit = Math.min(text.length, i + 1000);
    for (; j < limit; j++) {
      const ch = text[j];
      if (ch === '\\') { j++; continue; }
      if (ch === '[') depth++;
      else if (ch === ']') { depth--; if (depth === 0) break; }
      else if (ch === '\n' && text[j + 1] === '\n') return null;
    }
    if (j >= limit || text[j + 1] !== '(') return null;
    const m = /^\(\s*<?([^\s()<>]*(?:\([^\s()<>]*\)[^\s()<>]*)*)>?(?:\s+("[^"]*"|'[^']*'))?\s*\)/.exec(text.slice(j + 1, j + 2049));
    if (!m) return null;
    return { label: text.slice(i + 1, j), url: m[1], end: j + 1 + m[0].length };
  }

  // ---------- blocks ----------

  function splitRow(line) {
    let s = line.trim();
    if (s.startsWith('|')) s = s.slice(1);
    if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
    const cells = [];
    let cur = '';
    for (let k = 0; k < s.length; k++) {
      if (s[k] === '\\' && s[k + 1] === '|') { cur += '|'; k++; } else if (s[k] === '|') { cells.push(cur.trim()); cur = ''; } else cur += s[k];
    }
    cells.push(cur.trim());
    return cells;
  }

  function startsBlock(line, next) {
    return FENCE.test(line) || HEADING.test(line) || RULE.test(line) || QUOTE.test(line) || ITEM.test(line) ||
      (line.includes('|') && next !== undefined && TABLE_SEP.test(next) && next.includes('-'));
  }

  function blocks(lines, opts, depth, into) {
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      if (!line.trim()) { i++; continue; }
      let m;
      // Fenced code (an unclosed fence runs to the end: the stream may still be in it).
      if ((m = FENCE.exec(line))) {
        const mark = m[1];
        const code = [];
        i++;
        while (i < lines.length) {
          const t = lines[i].trim();
          if (t.length >= mark.length && t[0] === mark[0] && /^(`+|~+)$/.test(t)) { i++; break; }
          code.push(lines[i]);
          i++;
        }
        const pre = el('pre', 'md-code');
        const c = el('code', null, code.join('\n'));
        if (m[2]) pre.dataset.lang = m[2].slice(0, 20);
        pre.appendChild(c);
        into.appendChild(pre);
        continue;
      }
      if ((m = HEADING.exec(line))) {
        const h = el('h' + m[1].length, 'md-h');
        inline(m[2] || '', opts, depth, false).forEach(function (x) { h.appendChild(x); });
        into.appendChild(h);
        i++;
        continue;
      }
      if (RULE.test(line)) {
        into.appendChild(el('hr'));
        i++;
        continue;
      }
      if (QUOTE.test(line)) {
        const inner = [];
        while (i < lines.length && lines[i].trim() && (QUOTE.test(lines[i]) || !startsBlock(lines[i], lines[i + 1]))) {
          inner.push(lines[i].replace(QUOTE, ''));
          i++;
        }
        const q = el('blockquote');
        if (depth < MAX_DEPTH) blocks(inner, opts, depth + 1, q);
        else q.textContent = inner.join('\n');
        into.appendChild(q);
        continue;
      }
      if ((m = ITEM.exec(line))) {
        i = list(lines, i, opts, depth, into);
        continue;
      }
      if (line.includes('|') && i + 1 < lines.length && TABLE_SEP.test(lines[i + 1]) && lines[i + 1].includes('-')) {
        i = table(lines, i, opts, depth, into);
        continue;
      }
      // A paragraph: up to a blank line or the start of another block.
      const para = [line.trim()];
      i++;
      while (i < lines.length && lines[i].trim() && !startsBlock(lines[i], lines[i + 1])) {
        para.push(lines[i].trim());
        i++;
      }
      const p = el('p');
      inline(para.join('\n'), opts, depth, false).forEach(function (x) { p.appendChild(x); });
      into.appendChild(p);
    }
  }

  function list(lines, i, opts, depth, into) {
    const first = ITEM.exec(lines[i]);
    const indent = first[1].length;
    const ordered = /\d/.test(first[2]);
    const kind = ordered ? first[2].slice(-1) : first[2];
    const ul = el(ordered ? 'ol' : 'ul');
    if (ordered) {
      const start = parseInt(first[2], 10);
      if (start !== 1) ul.start = start;
    }
    while (i < lines.length) {
      const m = ITEM.exec(lines[i]);
      if (!m || m[1].length !== indent) break;
      const isOrdered = /\d/.test(m[2]);
      if (isOrdered !== ordered || (isOrdered ? m[2].slice(-1) : m[2]) !== kind) break;
      const content = [m[3] || ''];
      const contentIndent = indent + m[2].length + 1;
      i++;
      // The item's further lines: indented ones, lazy paragraph lines, blank lines inside it.
      while (i < lines.length) {
        const l = lines[i];
        if (!l.trim()) {
          // A blank line ends the item unless more indented content follows.
          let k = i + 1;
          while (k < lines.length && !lines[k].trim()) k++;
          if (k < lines.length && lines[k].length - lines[k].trimStart().length > indent) {
            for (; i < k; i++) content.push('');
            continue;
          }
          break;
        }
        const lead = l.length - l.trimStart().length;
        if (lead > indent) { content.push(l.slice(Math.min(lead, contentIndent))); i++; continue; }
        if (ITEM.test(l) || startsBlock(l, lines[i + 1])) break;
        content.push(l.trim());
        i++;
      }
      const li = el('li');
      if (depth < MAX_DEPTH) blocks(content, opts, depth + 1, li);
      else li.textContent = content.join('\n');
      ul.appendChild(li);
      // Blank lines between items keep the list going.
      let k = i;
      while (k < lines.length && !lines[k].trim()) k++;
      if (k > i && k < lines.length) {
        const n = ITEM.exec(lines[k]);
        if (n && n[1].length === indent) i = k;
      }
    }
    into.appendChild(ul);
    return i;
  }

  function table(lines, i, opts, depth, into) {
    const head = splitRow(lines[i]);
    const align = splitRow(lines[i + 1]).map(function (s) {
      return s.startsWith(':') && s.endsWith(':') ? 'center' : s.endsWith(':') ? 'right' : '';
    });
    const wrap = el('div', 'md-table');
    const t = el('table');
    const thead = el('thead');
    const tr = el('tr');
    head.forEach(function (h, k) {
      const th = el('th');
      if (align[k]) th.style.textAlign = align[k];
      inline(h, opts, depth, false).forEach(function (x) { th.appendChild(x); });
      tr.appendChild(th);
    });
    thead.appendChild(tr);
    t.appendChild(thead);
    const tbody = el('tbody');
    i += 2;
    while (i < lines.length && lines[i].trim() && lines[i].includes('|')) {
      const row = el('tr');
      const cells = splitRow(lines[i]);
      for (let k = 0; k < head.length; k++) {
        const td = el('td');
        if (align[k]) td.style.textAlign = align[k];
        inline(cells[k] || '', opts, depth, false).forEach(function (x) { td.appendChild(x); });
        row.appendChild(td);
      }
      tbody.appendChild(row);
      i++;
    }
    t.appendChild(tbody);
    wrap.appendChild(t);
    into.appendChild(wrap);
    return i;
  }

  function render(text, opts) {
    const frag = document.createDocumentFragment();
    blocks(String(text || '').replace(/\r\n?/g, '\n').replace(/\t/g, '    ').split('\n'), opts || {}, 0, frag);
    return frag;
  }

  // Where the settled part of streamed text ends: after the last blank line outside a code
  // block that is followed by an unindented line. `from` is a previous cut (outside a block).
  function stableCut(text, from) {
    let cut = from;
    let fence = null;
    let pos = from;
    let blank = false;
    let inList = false;
    for (;;) {
      const nl = text.indexOf('\n', pos);
      if (nl < 0) break; // the last line is still being written
      const line = text.slice(pos, nl);
      if (fence) {
        const t = line.trim();
        if (t.length >= fence.length && t[0] === fence[0] && /^(`+|~+)$/.test(t)) fence = null;
      } else if (!line.trim()) {
        blank = true;
      } else {
        const item = ITEM.test(line);
        const indented = /^\s/.test(line);
        // Items of one list (also with blank lines between them) stay together.
        if (blank && !indented && !(item && inList)) cut = pos;
        inList = item || (inList && (indented || !blank));
        const m = FENCE.exec(line);
        if (m) fence = m[1];
        blank = false;
      }
      pos = nl + 1;
    }
    return cut;
  }

  window.KrunkMarkdown = { render: render, stableCut: stableCut, inline: inline };
})();
