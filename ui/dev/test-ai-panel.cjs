// 0.12.0 UI tests: the AI agent panel (ui/shell/agent.*, markdown.js) and its shell parts (the
// #agent button, the running-macro indicator). Hooked into test-ui.cjs, which passes its helpers:
//   ONLY=AgentPanel,AgentShellButton node ui/dev/test-ui.cjs [shots]
'use strict';
const zlib = require('zlib');
const mockAi = require('./mock-ai.js');

// A small PNG "screenshot" (a page with a header bar and text lines) as a data: URL.
function shotPng() {
  const W = 240, H = 150;
  const rows = [];
  for (let y = 0; y < H; y++) {
    const row = [0];
    for (let x = 0; x < W; x++) {
      let c = [255, 255, 255];
      if (y < 22) c = [37, 99, 235];
      else if (y > 34 && (y - 34) % 14 < 6 && x > 14 && x < 14 + ((y * 37) % 160) + 40) c = [200, 204, 212];
      row.push(c[0], c[1], c[2]);
    }
    rows.push(Buffer.from(row));
  }
  const table = [];
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; table[n] = c >>> 0; }
  const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = table[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(W, 0); ihdr.writeUInt32BE(H, 4); ihdr[8] = 8; ihdr[9] = 2;
  const png = Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr),
    chunk('IDAT', zlib.deflateSync(Buffer.concat(rows))), chunk('IEND', Buffer.alloc(0))]);
  return 'data:image/png;base64,' + png.toString('base64');
}

const ANSWER = [
  '## Links on this page',
  '',
  'I found **12 links**. The *main* ones are below; the raw tag <img src=x onerror=alert(1)> stays text.',
  '',
  '- [Documentation](https://example.com/docs) - the `docs` section',
  '- [Bad link](javascript:alert(1)) is not a link',
  '- Plain address: https://example.org/about.',
  '',
  '1. First',
  '2. Second',
  '',
  '```js',
  'document.title = "<b>x</b>";',
  '```',
  '',
  'Line one',
  'line two',
].join('\n');

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot } = h;
  const count = async (page, cmd) => (await sent(page)).filter((m) => m.cmd === cmd).length;
  const settle = (page) => page.evaluate(() => new Promise((r) => setTimeout(() => requestAnimationFrame(() => setTimeout(r, 0)), 20)));
  const ai = (page, fn, ...args) => page.evaluate(([f, a]) => window.__ai[f](...a), [fn, args]);
  const png = shotPng();

  async function testAgentPanel(browser) {
    console.log('ai panel');
    const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/agent.html', { width: 380, height: 700 }, 1, mockAi);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    check((await lastSent(page, 'ready')) !== undefined, 'the panel posts ready');
    check(await page.evaluate(() => document.activeElement.id) === 'input', 'the message box has the focus on load');
    check(await page.isVisible('#empty'), 'an empty conversation shows a short hint');

    // Model indicator and the setup notice.
    await ai(page, 'config', { problem: 'No API key for OpenRouter.' });
    await settle(page);
    check(await page.isVisible('#problem') && (await page.textContent('#problem')).includes('No API key for OpenRouter.') &&
      (await page.textContent('#problem')).includes('Set up a provider in Settings → AI'), 'a configuration problem shows with "Set up a provider in Settings → AI"');
    await shot(page, 'ai-panel-problem.png');
    await ai(page, 'config');
    await settle(page);
    check(!(await page.isVisible('#problem')) && (await page.textContent('#model-name')) === 'OpenRouter · openai/gpt-4.1-mini', 'the header shows "provider · model"');

    // Input: Shift+Enter is a new line, Enter sends.
    await page.type('#input', 'List the links');
    await page.keyboard.press('Shift+Enter');
    await page.type('#input', 'on this page');
    check(await count(page, 'agentSend') === 0 && (await page.inputValue('#input')) === 'List the links\non this page', 'Shift+Enter adds a line');
    const h1 = await page.evaluate(() => document.getElementById('input').offsetHeight);
    for (let i = 0; i < 8; i++) await page.keyboard.press('Shift+Enter');
    const h2 = await page.evaluate(() => document.getElementById('input').offsetHeight);
    check(h2 > h1 && h2 <= 6 * 19 + 8, 'the box grows up to 6 lines (' + h1 + ' -> ' + h2 + ' px)');
    await page.fill('#input', 'List the links on this page');
    await page.keyboard.press('Enter');
    const s1 = await lastSent(page, 'agentSend');
    check(s1 && s1.text === 'List the links on this page' && (await page.inputValue('#input')) === '', 'Enter sends agentSend{text} and empties the box');

    // Running: Stop replaces Send, the box is disabled.
    await emit(page, { event: 'agentMessage', role: 'user', text: 'List the links on this page' });
    await ai(page, 'state', 'running', 0);
    await settle(page);
    check(await page.isVisible('.msg.user') && await page.isHidden('#empty'), 'the user message is shown');
    check(await page.isVisible('#stop') && await page.isHidden('#send') && await page.isDisabled('#input'), 'while running: big Stop instead of Send, the box disabled');
    check((await page.textContent('#status')).includes('Working…'), 'a "Working…" line');

    // Steps: running -> ok in place, details and image.
    await ai(page, 'step', 1, 'running');
    await settle(page);
    check(await page.locator('.step[data-n="1"].running .spinner').count() === 1, 'a running step shows a spinner');
    check((await page.textContent('#status-text')) === 'Working… (step 1)', 'status: "Working… (step 1)"');
    await ai(page, 'step', 1, 'ok', { summary: '12 links', detail: '{"limit":100}\n→ 12 links: https://example.com/docs, …', image: png, ms: 85 });
    await ai(page, 'step', 2, 'error', { tool: 'click', label: 'Click #missing', summary: 'No element matches #missing', detail: '{"selector":"#missing"}', ms: 12 });
    await ai(page, 'step', 3, 'refused', { tool: 'type_text', label: 'Type into #password', summary: 'refused: password/card field' });
    await settle(page);
    check(await page.locator('.step').count() === 3 && await page.locator('.step[data-n="1"].ok').count() === 1, 'steps update in place by n');
    check((await page.textContent('.step[data-n="1"] .step-sum')) === '12 links', 'the row shows the summary');
    check(await page.locator('.step[data-n="2"].error').count() === 1 && await page.locator('.step[data-n="3"].refused').count() === 1, 'error and refused rows');
    check(await page.isHidden('.step[data-n="1"] .step-body'), 'details are collapsed');
    await page.click('.step[data-n="1"] .step-head');
    check(await page.getAttribute('.step[data-n="1"] .step-head', 'aria-expanded') === 'true' &&
      (await page.textContent('.step[data-n="1"] pre')).includes('"limit":100'), 'a click shows the details');
    const img = await page.$('.step[data-n="1"] .thumb img');
    check(img && (await img.getAttribute('src')).startsWith('data:image/png') && await img.evaluate((i) => i.complete && i.naturalWidth === 240), 'the step image is shown as a thumbnail');
    await shot(page, 'ai-panel-steps.png');
    await page.click('.step[data-n="1"] .thumb');
    check(await page.isVisible('#viewer') && await page.evaluate(() => document.getElementById('viewer-img').naturalWidth === 240), 'a click enlarges the image');
    await shot(page, 'ai-panel-image.png');
    const stops0 = await count(page, 'agentStop');
    await page.keyboard.press('Escape');
    check(await page.isHidden('#viewer') && await count(page, 'agentStop') === stops0, 'Esc closes the image first (no stop)');

    // A streamed Markdown answer.
    await page.evaluate(() => {
      const log = document.getElementById('log');
      window.__added = 0;
      new MutationObserver((ms) => ms.forEach((m) => { window.__added += m.addedNodes.length; })).observe(log, { childList: true, subtree: true });
    });
    const half = ANSWER.indexOf('1. First');
    await ai(page, 'stream', ANSWER.slice(0, half), 6);
    await settle(page);
    await page.waitForFunction(() => document.querySelector('.msg.assistant.live h2'));
    const firstP = await page.evaluate(() => { const p = document.querySelector('.msg.assistant.live p'); p.__mark = 1; return p.textContent; });
    check(firstP.includes('12 links'), 'deltas render progressively (heading and paragraph before the end)');
    await ai(page, 'stream', ANSWER.slice(half), 6);
    await settle(page);
    check(await page.evaluate(() => document.querySelector('.msg.assistant.live p').__mark === 1), 'the settled part is not re-rendered on each delta');
    await emit(page, { event: 'agentMessage', role: 'assistant', text: ANSWER });
    await settle(page);
    const md = await page.evaluate(() => {
      const m = document.querySelectorAll('.msg.assistant');
      const a = m[m.length - 1];
      const q = (s) => a.querySelectorAll(s).length;
      return {
        count: m.length, live: a.classList.contains('live'), text: a.textContent, h2: q('h2'), strong: q('strong'), em: q('em'), code: q('p code, li code'),
        pre: a.querySelector('pre code') && a.querySelector('pre code').textContent, ul: q('ul > li'), ol: q('ol > li'), br: q('p br'),
        links: Array.from(a.querySelectorAll('a')).map((x) => x.getAttribute('href')),
        imgs: document.querySelectorAll('#log img').length,
      };
    });
    check(md.count === 1 && !md.live, 'the final answer replaces the streamed text (no duplicate)');
    check(md.h2 === 1 && md.strong === 1 && md.em === 1 && md.code === 1 && md.ul === 3 && md.ol === 2 && md.br === 1, 'Markdown: heading, bold, italic, code, lists, line break (' + JSON.stringify(md) + ')');
    check(md.pre === 'document.title = "<b>x</b>";', 'a fenced code block keeps its text');
    check(md.text.includes('<img src=x onerror=alert(1)>') && md.imgs === 1, 'raw HTML is shown as text (no img element besides the step thumbnail)');
    check(md.links.length === 2 && md.links[0] === 'https://example.com/docs' && md.links[1] === 'https://example.org/about', 'only http(s) links; trailing dot not part of the address (' + md.links.join(' ') + ')');
    check(md.text.includes('Bad link') && !md.links.some((l) => /^javascript/i.test(l)), 'a javascript: link is plain text');
    await page.click('.msg.assistant a[href="https://example.com/docs"]');
    const ou = await lastSent(page, 'openUrl');
    check(ou && ou.url === 'https://example.com/docs' && page.url() === 'https://shell.krunk.invalid/agent.html', 'a link click posts openUrl{url}, the panel stays');
    await shot(page, 'ai-panel-stream.png');

    // Stop while running, then "Stopped".
    check(await page.isVisible('#stop'), 'Stop is still visible while running');
    await page.click('#stop');
    check(await count(page, 'agentStop') === stops0 + 1, 'Stop sends agentStop');
    await ai(page, 'state', 'stopped', 3);
    await settle(page);
    check(await page.locator('.msg.stopped').count() === 1 && (await page.textContent('.msg.stopped')) === 'Stopped', '"Stopped" is shown');
    check(await page.isHidden('#stop') && await page.isVisible('#send') && !(await page.isDisabled('#input')) && await page.isHidden('#status'), 'stopped: Send back, the box enabled');
    check(await page.evaluate(() => document.activeElement.id) === 'input', 'the box gets the focus back');
    await page.keyboard.press('Escape');
    check(await count(page, 'agentStop') === stops0 + 1, 'Esc while idle sends nothing');

    // Esc stops a running task from anywhere in the panel.
    await ai(page, 'state', 'running', 3);
    await settle(page);
    await page.click('#log', { position: { x: 5, y: 5 } });
    await page.keyboard.press('Escape');
    check(await count(page, 'agentStop') === stops0 + 2, 'Esc while running sends agentStop');
    await shot(page, 'ai-panel-running.png');

    // ask_user: single choice.
    await ai(page, 'state', 'asking', 3);
    await emit(page, { event: 'agentAsk', ask: { id: 7, question: 'Which format do you want?', options: ['CSV', 'JSON', 'Markdown table'], multi: false, allowOther: true } });
    await settle(page);
    check((await page.textContent('#status-text')).includes('Waiting for your answer'), 'asking: "Waiting for your answer…"');
    check(await page.locator('.card.ask button.opt').count() === 3 && await page.isVisible('.card.ask .other input'), 'single choice: one button per option and an Other field');
    await shot(page, 'ai-panel-ask-single.png');
    await page.click('.card.ask button.opt:has-text("JSON")');
    let ans = await lastSent(page, 'agentAnswer');
    check(ans && ans.ask === 7 && JSON.stringify(ans.choices) === '[1]' && ans.other === undefined, 'a click answers agentAnswer{ask:7, choices:[1]}');
    check(await page.locator('.card.ask.done button').count() === 0 && (await page.textContent('.card.ask.done .answer')).includes('JSON'), 'the card turns read-only with the answer');
    // Other in single mode.
    await emit(page, { event: 'agentAsk', ask: { id: 8, question: 'Name of the file?', options: ['links.csv', 'out.csv'], multi: false, allowOther: true } });
    await settle(page);
    const c8 = page.locator('.card.ask:not(.done)');
    check(await c8.locator('.other button').isDisabled(), 'Other: Send is disabled while empty');
    await c8.locator('.other input').fill('my-links.csv');
    await c8.locator('.other input').press('Enter');
    ans = await lastSent(page, 'agentAnswer');
    check(ans.ask === 8 && ans.choices.length === 0 && ans.other === 'my-links.csv', 'Other text answers {choices:[], other}');
    // Multiple choice with Other.
    await emit(page, { event: 'agentAsk', ask: { id: 9, question: 'What should I collect?', options: ['Links', 'Images', 'Forms'], multi: true, allowOther: true } });
    await settle(page);
    const c9 = page.locator('.card.ask:not(.done)');
    check(await c9.locator('input[type=checkbox]').count() === 3 && await c9.locator('.acts .btn.primary').isDisabled(), 'multi: checkboxes, Send disabled until a choice');
    await c9.locator('label.opt:has-text("Links")').click();
    await c9.locator('label.opt:has-text("Forms")').click();
    await c9.locator('.other input').fill('Scripts');
    await shot(page, 'ai-panel-ask-multi.png');
    await c9.locator('.acts .btn.primary').click();
    ans = await lastSent(page, 'agentAnswer');
    check(ans.ask === 9 && JSON.stringify(ans.choices) === '[0,2]' && ans.other === 'Scripts', 'multi: Send posts {choices:[0,2], other:"Scripts"}');
    check((await page.locator('.card.ask.done').last().textContent()).includes('Links, Forms, Other: Scripts'), 'the multi card shows the answers');

    // Consent.
    await emit(page, { event: 'agentConsentAsk', ask: 10, category: 'history', label: 'your browsing history' });
    await settle(page);
    check((await page.locator('.card.consent').last().textContent()).includes('The agent wants to read your browsing history. Allow?'), 'consent: "The agent wants to read … Allow?"');
    await shot(page, 'ai-panel-consent.png');
    await page.locator('.card.consent:not(.done) button:has-text("Allow")').click();
    let cs = await lastSent(page, 'agentConsent');
    check(cs && cs.ask === 10 && cs.allow === true, 'Allow posts agentConsent{ask, allow:true}');
    await emit(page, { event: 'agentConsentAsk', ask: 11, category: 'cookies', label: 'cookies of a site' });
    await settle(page);
    await page.locator('.card.consent:not(.done) button:has-text("Deny")').click();
    cs = await lastSent(page, 'agentConsent');
    check(cs.ask === 11 && cs.allow === false && (await page.locator('.card.consent.done').last().textContent()).includes('Denied'), 'Deny posts allow:false and shows "Denied"');

    // Continue after the step limit.
    await emit(page, { event: 'agentContinueAsk', ask: 12, steps: 30 });
    await settle(page);
    check((await page.locator('.card.continue').last().textContent()).includes('The agent used 30 steps. Continue?'), 'continue: "The agent used 30 steps. Continue?"');
    await page.locator('.card.continue:not(.done) button:has-text("Continue")').click();
    let ct = await lastSent(page, 'agentContinue');
    check(ct && ct.ask === 12 && ct.go === true, 'Continue posts agentContinue{ask, go:true}');
    await emit(page, { event: 'agentContinueAsk', ask: 13, steps: 60 });
    await settle(page);
    await page.locator('.card.continue:not(.done) button:has-text("Stop")').click();
    ct = await lastSent(page, 'agentContinue');
    check(ct.ask === 13 && ct.go === false, 'Stop posts go:false');
    // An open question closes when the task ends.
    await emit(page, { event: 'agentAsk', ask: { id: 14, question: 'Unanswered?', options: ['A', 'B'], multi: false, allowOther: false } });
    await ai(page, 'state', 'stopped', 3);
    await settle(page);
    check(await page.locator('.card.ask:not(.done)').count() === 0 && (await page.locator('.card.ask.done').last().textContent()).includes('Not answered'), 'an open question shows "Not answered" after Stop');

    // Script cards.
    await emit(page, { event: 'agentScript', card: { id: 'u1', kind: 'userscript', name: 'Hide the cookie banner', enabled: true } });
    await emit(page, { event: 'agentScript', card: { id: 'm1', kind: 'macro', name: 'Hold A', enabled: true } });
    await settle(page);
    const sc = page.locator('.card.script').first();
    check((await sc.textContent()).includes('Created userscript: Hide the cookie banner'), 'script card: "Created userscript: <name>"');
    await sc.locator('button.toggle').click();
    let tg = await lastSent(page, 'agentToggleScript');
    check(tg && tg.id === 'u1' && tg.kind === 'userscript' && tg.enabled === false && (await sc.locator('button.toggle').textContent()) === 'Enable', 'Disable posts agentToggleScript{enabled:false}, then shows Enable');
    await sc.locator('button.toggle').click();
    tg = await lastSent(page, 'agentToggleScript');
    check(tg.enabled === true, 'Enable posts enabled:true');
    await page.locator('.card.script').nth(1).locator('button.open').click();
    const os = await lastSent(page, 'agentOpenScript');
    check(os && os.id === 'm1' && os.kind === 'macro', 'Open in editor posts agentOpenScript{id, kind}');
    await emit(page, { event: 'agentScript', card: { id: 'u1', kind: 'userscript', name: 'Hide the cookie banner v2', enabled: false } });
    await settle(page);
    check(await page.locator('.card.script').count() === 2 && (await sc.textContent()).includes('Updated userscript: Hide the cookie banner v2'), 'a changed script updates its card in place');

    // Error and notice.
    await emit(page, { event: 'agentError', message: 'The provider answered 401: invalid key.' });
    await emit(page, { event: 'agentMessage', role: 'notice', text: 'Switched to llama3.1:8b.' });
    await settle(page);
    check(await page.locator('.msg.error[role=alert]').count() === 1 && (await page.textContent('.msg.error')).includes('401'), 'agentError shows an error box');
    check(await page.locator('.msg.notice').count() === 1, 'a notice is shown muted');
    await shot(page, 'ai-panel-cards.png');

    // Model quick switch (mouse and keyboard).
    await page.click('#model');
    check(await page.isVisible('#models') && await page.locator('#models .mopt').count() === 3 && await page.locator('#models .mgroup').count() === 2, 'the model list shows every model by provider');
    check(await page.locator('#models .mopt[aria-selected="true"]').textContent() === 'openai/gpt-4.1-mini', 'the current model is marked');
    await shot(page, 'ai-panel-models.png');
    await page.click('#models .mopt:has-text("llama3.1:8b")');
    let sm = await lastSent(page, 'agentSelectModel');
    check(sm && sm.provider === 'p2' && sm.model === 'llama3.1:8b' && await page.isHidden('#models'), 'a click posts agentSelectModel{provider, model}');
    check((await page.textContent('#model-name')) === 'Local · llama3.1:8b', 'the header shows the new model');
    await page.click('#model');
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Enter');
    sm = await lastSent(page, 'agentSelectModel');
    check(sm.provider === 'p1' && sm.model === 'anthropic/claude-sonnet-4', 'the keyboard picks a model too');
    await page.click('#model');
    const stops1 = await count(page, 'agentStop');
    await page.keyboard.press('Escape');
    check(await page.isHidden('#models') && await count(page, 'agentStop') === stops1, 'Esc closes the list');

    // Narrow and wide panels.
    for (const w of [280, 900]) {
      await page.setViewportSize({ width: w, height: 700 });
      await settle(page);
      const over = await page.evaluate(() => document.getElementById('log').scrollWidth - document.getElementById('log').clientWidth);
      check(over <= 0, w + ' px: no horizontal scrolling (' + over + ')');
      await shot(page, 'ai-panel-' + w + '.png');
    }
    await page.setViewportSize({ width: 380, height: 700 });

    // Dark theme.
    await emit(page, { event: 'theme', theme: 'dark' });
    await page.evaluate(() => { document.getElementById('log').scrollTop = 0; });
    await settle(page);
    const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    check(bg === 'rgb(43, 45, 49)', 'dark theme (' + bg + ')');
    await shot(page, 'ai-panel-dark.png');
    await page.evaluate(() => { document.getElementById('log').scrollTop = 1e6; });
    await ai(page, 'state', 'running', 4);
    await ai(page, 'step', 4, 'running', { tool: 'screenshot', label: 'Take a screenshot' });
    await settle(page);
    await shot(page, 'ai-panel-dark-running.png');
    await ai(page, 'state', 'done', 4);

    // A long conversation: deltas stay cheap.
    await page.evaluate(() => {
      for (let i = 0; i < 150; i++) window.__host.emit({ event: 'agentMessage', role: i % 2 ? 'assistant' : 'user', text: 'Message **' + i + '** with a [link](https://example.com/' + i + ')' });
    });
    await settle(page);
    const t = await page.evaluate(async () => {
      const text = 'A longer paragraph with some **bold** words and `code`, repeated. '.repeat(4) + '\n\n';
      window.__added = 0;
      const t0 = performance.now();
      for (let i = 0; i < 400; i++) window.__host.emit({ event: 'agentDelta', text: text.slice((i * 13) % text.length, (i * 13) % text.length + 13) || 'x' });
      await new Promise((r) => setTimeout(r, 50));
      await new Promise((r) => requestAnimationFrame(() => r()));
      return { ms: performance.now() - t0, added: window.__added };
    });
    check(t.ms < 1500, 'a long conversation: 400 deltas after 150 messages in ' + Math.round(t.ms) + ' ms (' + t.added + ' nodes added)');
    await ai(page, 'state', 'done', 4);

    // New conversation.
    await page.click('#new');
    check(await count(page, 'agentNew') === 1, '"New conversation" posts agentNew');
    await emit(page, { event: 'agentClear' });
    await settle(page);
    check(await page.locator('#log .msg, #log .step, #log .card').count() === 0 && await page.isVisible('#empty'), 'agentClear empties the conversation');
    await page.click('#close');
    check(await count(page, 'agentClose') === 1, 'the close button posts agentClose');
    await emit(page, { event: 'theme', theme: 'light' });
    await settle(page);
    await shot(page, 'ai-panel-empty.png');

    check(external.length === 0, 'no network requests (' + external.join(' ') + ')');
    check(errors.length === 0, 'no console errors and no dialogs (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  async function testAgentShellButton(browser) {
    console.log('ai shell button');
    for (const theme of ['light', 'dark']) {
      const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 76 }, 1);
      await page.evaluate(() => {
        const h = window.__host;
        h.state.tabs = [{ id: 1, title: 'Example Domain', url: 'https://example.com/', favicon: null, loading: false, private: false, sleeping: false }];
        h.state.active = 1;
        h.tabsEvent();
        h.navEvent();
      });
      await emit(page, { event: 'theme', theme: theme });
      await page.waitForSelector('.tab');
      await emit(page, { event: 'agentButton', visible: false, open: false });
      await page.waitForTimeout(40);
      check(await page.isHidden('#agent'), theme + ': agentButton{visible:false} hides #agent');
      await emit(page, { event: 'agentButton', visible: true, open: true });
      await page.waitForTimeout(40);
      const pos = await page.evaluate(() => {
        const s = document.getElementById('shield'), a = document.getElementById('agent');
        const sb = s.getBoundingClientRect(), ab = a.getBoundingClientRect();
        return { next: s.nextElementSibling === a, left: ab.left, right: sb.right, dy: Math.abs((ab.top + ab.bottom) / 2 - (sb.top + sb.bottom) / 2), pressed: a.getAttribute('aria-pressed'), on: a.classList.contains('on') };
      });
      check(await page.isVisible('#agent') && pos.next, theme + ': visible, right after #shield in the DOM');
      check(pos.left >= pos.right - 1 && pos.dy < 2, theme + ': its box is right of the shield, on the same line');
      check(pos.pressed === 'true' && pos.on, theme + ': open -> pressed');
      await shot(page, 'ai-shell-button-' + theme + '.png');
      await page.click('#agent');
      check((await lastSent(page, 'toggleAgent')) !== undefined, theme + ': a click posts toggleAgent');
      await emit(page, { event: 'agentButton', visible: true, open: false });
      await page.waitForTimeout(40);
      check(await page.getAttribute('#agent', 'aria-pressed') === 'false', theme + ': closed -> not pressed');

      await emit(page, { event: 'macroRunning', id: 'm1', name: 'Hold A' });
      await page.waitForTimeout(40);
      check(await page.isVisible('#macro-run') && (await page.textContent('#macro-run-name')) === 'Hold A', theme + ': macroRunning shows the indicator with the name');
      await shot(page, 'ai-shell-macro-' + theme + '.png');
      await page.click('#macro-stop');
      check((await lastSent(page, 'stopMacros')) !== undefined, theme + ': Stop posts stopMacros');
      await emit(page, { event: 'macroRunning', id: null, name: null });
      await page.waitForTimeout(40);
      check(await page.isHidden('#macro-run'), theme + ': no macro -> no indicator');
      // A userscript's GM_notification / a macro's toast() reach the shell as `toast`.
      await emit(page, { event: 'toast', message: 'GM test ran 1 times <b>x</b>' });
      await page.waitForTimeout(40);
      check(await page.isVisible('#shell-toast') && (await page.textContent('#shell-toast')) === 'GM test ran 1 times <b>x</b>'
        && await page.$('#shell-toast b') === null, theme + ': toast shows the message as text');
      await shot(page, 'ai-shell-toast-' + theme + '.png');
      check(external.length === 0, theme + ': no network requests');
      check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  return { testAgentPanel, testAgentShellButton };
};
