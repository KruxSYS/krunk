#!/usr/bin/env node
// A mock OpenAI-compatible model provider for Krunk's agent tests (0.12.0). Not shipped.
//
//   node ui/dev/mock-provider.cjs [--port 18111] [--log /tmp/mock-provider.jsonl]
//
// GET  /v1/models            two models (one with tools and vision)
// POST /v1/chat/completions  streamed (SSE) answers scripted by the last user message:
//   "title … to WORD"        set_title {title: WORD}, then finish
//   "links"                   list_links, then finish with the links it got
//   "screenshot"              save_screenshot, then finish
//   "banner"                  query #promo-banner, save_userscript removing it, then finish
//   "holds the A key"         save_macro (holds A until Stop), run_macro, then finish
//   "fill in the form"        list_forms, fill_fields with every field it got (the password
//                             and card too: Krunk must refuse those), then finish
//   "ask"                     ask_user (3 colours), then finish with the answer
//   "slow"                    text, one word every 200 ms for 60 s (for Stop)
//   "history" / "bookmarks"   get_history / get_bookmarks (the gate and consent), then finish
//   "passwords page"          navigate to krunk://passwords and krunk://sync (both refused)
//   "read this settings page" get_html and screenshot on a built-in page (refused)
//   "read the password"       eval_js that fills (if empty) and reads the password field
//   "loop"                    get_page_context forever (the step limit's question)
//   anything else             a short text answer
// Every request is appended to the log (JSON lines: time, body), and so is every connection
// that closes before its answer ended: the VM smoke test checks both (Stop closes the
// connection; the password value never appears in a request).
'use strict';
const http = require('http');
const fs = require('fs');

const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : def; };
const PORT = Number(opt('--port', 18111));
const LOG = opt('--log', '/tmp/mock-provider.jsonl');

function log(entry) {
  fs.appendFileSync(LOG, JSON.stringify(Object.assign({ t: new Date().toISOString() }, entry)) + '\n');
}

// ---- reading the conversation ----
function lastUser(messages) {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'user') return { index: i, text: textOf(messages[i].content) };
  }
  return { index: -1, text: '' };
}
function textOf(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) return content.filter((p) => p.type === 'text').map((p) => p.text).join('\n');
  return '';
}
// Tool results after the last user message, in order.
function toolResults(messages, from) {
  return messages.slice(from + 1).filter((m) => m.role === 'tool').map((m) => textOf(m.content));
}
function parse(text) {
  try { return JSON.parse(text); } catch (_) {
    // A capped result has a JSON tail line; try the first line too.
    try { return JSON.parse(String(text).split('\n')[0]); } catch (_) { return null; }
  }
}
// The user's own words: the context block comes before them (the line after the last marker).
function userWords(text) {
  const m = /(?:^|\n)User message:\s*([\s\S]*)$/i.exec(text);
  return (m ? m[1] : text).trim();
}

// ---- scenarios: each returns the answer for step `n` (tool results so far in `results`) ----
const call = (name, a) => ({ tool: { name, args: a || {} } });
const finish = (answer) => call('finish', { answer });
const SCENARIOS = [
  // ---- 0.12.0 M5 ----
  {
    match: /\bhistory\b/i,
    steps: [() => call('get_history', { days: 1, limit: 20 }), (m, r) => finish('History result: ' + (r[0] || '').slice(0, 300))],
  },
  {
    match: /\bbookmarks\b/i,
    steps: [() => call('get_bookmarks', {}), (m, r) => finish('Bookmarks result: ' + (r[0] || '').slice(0, 300))],
  },
  {
    match: /passwords page/i,
    steps: [
      () => call('navigate', { url: 'krunk://passwords' }),
      () => call('navigate', { url: 'krunk://sync' }),
      (m, r) => finish('passwords: ' + (r[0] || '').slice(0, 150) + ' | sync: ' + (r[1] || '').slice(0, 150)),
    ],
  },
  {
    match: /read this settings page/i,
    steps: [
      () => call('get_html', {}),
      () => call('screenshot', {}),
      (m, r) => finish('get_html: ' + (r[0] || '').slice(0, 150) + ' | screenshot: ' + (r[1] || '').slice(0, 150)),
    ],
  },
  {
    match: /read the password/i,
    steps: [
      () => call('eval_js', { code: "(() => { const p = document.getElementById('pw'); if (!p.value) p.value = 'typed-SECRET-777'; return 'the field holds: ' + p.value; })()" }),
      (m, r) => finish('eval_js: ' + (r[0] || '').slice(0, 200)),
    ],
  },
  {
    // Never finishes: for the step limit (a continue question after max steps).
    match: /\bloop\b/i,
    steps: [() => call('get_page_context', {})],
  },
  {
    match: /title of this page to\s+(\S+)/i,
    steps: [
      (m) => call('set_title', { title: m[1].replace(/["'.]+$/, '') }),
      (m) => finish('The page title is now **' + m[1].replace(/["'.]+$/, '') + '**.'),
    ],
  },
  {
    match: /links/i,
    steps: [
      () => call('list_links', {}),
      (m, r) => {
        const v = parse(r[0]);
        const links = (v && (v.links || v)) || [];
        const lines = Array.isArray(links) ? links.map((l) => '- [' + (l.text || l.href) + '](' + l.href + ')') : [];
        return finish('The page has ' + lines.length + ' links:\n\n' + lines.join('\n'));
      },
    ],
  },
  {
    match: /screenshot/i,
    steps: [() => call('save_screenshot', { full_page: false }), (m, r) => finish('Saved: ' + (r[0] || '').slice(0, 200))],
  },
  {
    match: /banner/i,
    steps: [
      () => call('query', { selector: '#promo-banner' }),
      () => call('save_userscript', {
        source: [
          '// ==UserScript==',
          '// @name        Remove the promo banner',
          '// @namespace   krunk-mock',
          '// @version     1.0',
          '// @match       *://*/*agent*.html*',
          '// @grant       GM_addStyle',
          '// @run-at      document-start',
          '// ==/UserScript==',
          "GM_addStyle('#promo-banner { display: none !important; }');",
          "document.addEventListener('DOMContentLoaded', () => document.getElementById('promo-banner')?.remove());",
        ].join('\n'),
      }),
      (m, r) => finish('I created the userscript "Remove the promo banner". ' + (r[1] || '').slice(0, 200)),
    ],
  },
  {
    match: /holds? the a key/i,
    steps: [
      () => call('save_macro', {
        name: 'Hold A',
        source: "await key.down('a');\nawait repeat('forever', () => wait(1000));",
      }),
      (m, r) => {
        const v = parse(r[0]) || {};
        return call('run_macro', { id: v.id || '' });
      },
      (m, r) => finish('The macro "Hold A" is running and holds the A key. Stop it with the toolbar button or Ctrl+Shift+. — ' + (r[1] || '').slice(0, 120)),
    ],
  },
  {
    match: /fill in the form/i,
    steps: [
      () => call('list_forms', {}),
      (m, r) => {
        const v = parse(r[0]);
        const forms = (v && (v.forms || v)) || [];
        const fields = {};
        const sample = { email: 'ann@example.com', tel: '+1 555 0100', password: 'mock-typed-pw', number: '4111 1111 1111 1111' };
        (Array.isArray(forms) ? forms : []).forEach((f) => (f.fields || []).forEach((x) => {
          if (!x.selector || x.type === 'submit' || x.type === 'hidden') return;
          fields[x.selector] = sample[x.type] || (/card|cc/i.test(x.name || '') ? '4111 1111 1111 1111' : 'Ann Example');
        }));
        return call('fill_fields', { fields });
      },
      (m, r) => finish('Filled the form. Result: ' + (r[1] || '').slice(0, 600)),
    ],
  },
  {
    match: /\bask\b/i,
    steps: [
      () => call('ask_user', { question: 'Which colour should I use?', options: ['Red', 'Green', 'Blue'] }),
      (m, r) => finish('You answered: ' + (r[0] || '')),
    ],
  },
];

// ---- SSE ----
function sse(res, obj) { res.write('data: ' + JSON.stringify(obj) + '\n\n'); }
function chunk(model, delta, finishReason) {
  return { id: 'mock', object: 'chat.completion.chunk', created: 0, model, choices: [{ index: 0, delta, finish_reason: finishReason || null }] };
}
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function answer(req, res, body) {
  const model = body.model || 'mock';
  const messages = body.messages || [];
  const user = lastUser(messages);
  const words = userWords(user.text);
  const results = toolResults(messages, user.index);
  let closed = false;
  res.on('close', () => { if (!res.writableEnded) { closed = true; log({ closed: 'early', words: words.slice(0, 80) }); } });
  res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', Connection: 'keep-alive' });
  sse(res, chunk(model, { role: 'assistant', content: '' }));
  if (/\bslow\b/i.test(words)) {
    for (let i = 0; i < 300 && !closed; i++) {
      sse(res, chunk(model, { content: 'word' + i + ' ' }));
      await wait(200);
    }
    if (!closed) { sse(res, chunk(model, {}, 'stop')); res.end('data: [DONE]\n\n'); }
    return;
  }
  const sc = SCENARIOS.find((s) => s.match.test(words));
  const step = sc ? sc.steps[Math.min(results.length, sc.steps.length - 1)] : null;
  const out = step ? step(words.match(sc.match), results) : { text: 'Mock provider: I got "' + words.slice(0, 100) + '".' };
  await wait(150);
  if (out.text) {
    for (const piece of out.text.match(/.{1,12}/gs) || []) {
      if (closed) return;
      sse(res, chunk(model, { content: piece }));
      await wait(30);
    }
    sse(res, chunk(model, {}, 'stop'));
  } else {
    const a = JSON.stringify(out.tool.args);
    const id = 'call_' + Math.random().toString(36).slice(2, 10);
    sse(res, chunk(model, { tool_calls: [{ index: 0, id, type: 'function', function: { name: out.tool.name, arguments: '' } }] }));
    // The arguments in pieces, as real providers send them.
    for (const piece of a.match(/.{1,16}/gs) || []) {
      if (closed) return;
      sse(res, chunk(model, { tool_calls: [{ index: 0, function: { arguments: piece } }] }));
      await wait(10);
    }
    sse(res, chunk(model, {}, 'tool_calls'));
  }
  res.end('data: [DONE]\n\n');
}

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url.replace(/\/$/, '') === '/v1/models') {
    log({ get: '/v1/models', auth: req.headers.authorization ? 'present' : 'none' });
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ object: 'list', data: [
      { id: 'mock-agent', object: 'model', owned_by: 'krunk-mock', name: 'Mock agent (tools)' },
      { id: 'mock-small', object: 'model', owned_by: 'krunk-mock', name: 'Mock small' },
    ] }));
    return;
  }
  if (req.method === 'POST' && req.url === '/v1/chat/completions') {
    let data = '';
    req.on('data', (c) => { data += c; });
    req.on('end', () => {
      let body = {};
      try { body = JSON.parse(data); } catch (_) { res.writeHead(400); res.end('bad json'); return; }
      log({ post: '/v1/chat/completions', auth: req.headers.authorization ? 'present' : 'none', body });
      answer(req, res, body).catch((e) => { log({ error: String(e) }); try { res.end(); } catch (_) { /* gone */ } });
    });
    return;
  }
  res.writeHead(404, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ error: { message: 'not found' } }));
});
server.listen(PORT, '127.0.0.1', () => console.log('mock provider on http://127.0.0.1:' + PORT + '/v1, log ' + LOG));
