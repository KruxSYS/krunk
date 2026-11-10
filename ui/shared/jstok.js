// The JavaScript tokenizer shared by the source viewer (krunk://source) and the code editor
// (ui/shared/editor.js). Moved out of source.js in 0.12.0.
//
// KrunkJsTok.js(text, start, end, out, kinds) pushes [start, end, kind] triples (sorted, not
// overlapping) into `out`. `kinds` names the number to push for each kind of token:
//   com (comments), str (strings), kw (keywords), num (numbers): always;
//   re (regular expression literals) and tpl (template literals, with the code inside ${…}
//   tokenized too): only when given. Without them a `/` is plain text and a template literal
//   is one string, as the source viewer always had it.
(function () {
  'use strict';
  function isWs(c) { return c === 32 || c === 9 || c === 10 || c === 13 || c === 12; }
  function isIdStart(c) { return (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 36 || c === 95 || c > 127; }
  function isId(c) { return isIdStart(c) || (c >= 48 && c <= 57); }
  function isDigit(c) { return c >= 48 && c <= 57; }

  const KEYWORDS = new Set(('break case catch class const continue debugger default delete do else export extends false finally for function if import in instanceof let new null return super switch this throw true try typeof undefined var void while with yield async await of static get set').split(' '));
  // After these words an expression starts, so a `/` begins a regular expression.
  const BEFORE_EXPR = new Set('return typeof instanceof in of new delete void throw case do else yield await'.split(' '));

  // The end of the quoted string starting at `i` (a line break ends an unclosed '…' or "…").
  function string(s, i, b) {
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

  // The end of the regular expression literal starting at `i`, or -1 when it isn't one (a line
  // break comes first).
  function regex(s, i, b) {
    let j = i + 1, cls = false;
    while (j < b) {
      const c = s.charCodeAt(j);
      if (c === 10 || c === 13) return -1;
      if (c === 92) { j += 2; continue; }
      if (c === 91) cls = true;
      else if (c === 93) cls = false;
      else if (c === 47 && !cls) {
        j++;
        while (j < b && isId(s.charCodeAt(j))) j++;
        return j;
      }
      j++;
    }
    return -1;
  }

  // A template literal starting at `i` (kinds.tpl given): its text parts are `tpl` tokens, the
  // code inside ${…} is tokenized as code.
  function template(s, i, b, out, k) {
    let start = i, j = i + 1;
    while (j < b) {
      const c = s.charCodeAt(j);
      if (c === 92) { j += 2; continue; }
      if (c === 96) { out.push(start, j + 1, k.tpl); return j + 1; }
      if (c === 36 && s.charCodeAt(j + 1) === 123) {
        out.push(start, j + 2, k.tpl);
        const e = scan(s, j + 2, b, out, k, true);
        if (e >= b) return b;
        start = e; // the closing brace starts the next text part
        j = e + 1;
        continue;
      }
      j++;
    }
    if (b > start) out.push(start, b, k.tpl);
    return b;
  }

  // Tokenizes s[a, b). `nested`: inside ${…}; returns at its closing brace.
  function scan(s, a, b, out, k, nested) {
    let i = a, depth = 0;
    let value = false; // the last token was a value (so a `/` divides)
    while (i < b) {
      const c = s.charCodeAt(i);
      if (c === 47 && s.charCodeAt(i + 1) === 47) {
        let e = s.indexOf('\n', i);
        if (e < 0 || e > b) e = b;
        out.push(i, e, k.com); i = e; continue;
      }
      if (c === 47 && s.charCodeAt(i + 1) === 42) {
        let e = s.indexOf('*/', i + 2);
        e = e < 0 || e > b ? b : e + 2;
        out.push(i, e, k.com); i = e; continue;
      }
      if (c === 96 && k.tpl) { i = template(s, i, b, out, k); value = true; continue; }
      if (c === 34 || c === 39 || c === 96) { const e = string(s, i, b); out.push(i, e, k.str); i = e; value = true; continue; }
      if (c === 47 && k.re && !value) {
        const e = regex(s, i, b);
        if (e > 0) { out.push(i, e, k.re); i = e; value = true; continue; }
      }
      if (isDigit(c) && !(i > a && isId(s.charCodeAt(i - 1)))) {
        let e = i + 1;
        while (e < b && (isId(s.charCodeAt(e)) || s.charCodeAt(e) === 46)) e++;
        out.push(i, e, k.num); i = e; value = true; continue;
      }
      if (isIdStart(c)) {
        let e = i + 1;
        while (e < b && isId(s.charCodeAt(e))) e++;
        const word = s.slice(i, e);
        if (KEYWORDS.has(word) && s.charCodeAt(i - 1) !== 46) out.push(i, e, k.kw);
        value = !(BEFORE_EXPR.has(word) && s.charCodeAt(i - 1) !== 46);
        i = e; continue;
      }
      if (nested) {
        if (c === 123) depth++;
        else if (c === 125) { if (depth === 0) return i; depth--; }
      }
      if (!isWs(c)) value = c === 41 || c === 93 || c === 125; // ) ] }
      i++;
    }
    return b;
  }

  function js(s, a, b, out, kinds) { scan(s, a, b, out, kinds, false); }

  window.KrunkJsTok = { js: js, string: string, KEYWORDS: KEYWORDS };
})();
