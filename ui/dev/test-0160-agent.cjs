// Krunk 0.16.0 UI tests of the AI agent: the panel's scratchpad view (the agent's plan and notes,
// `agentScratch` events, Clear); passwords on request: the consent that names the site, the card
// of a generated password (masked, Show, Copy) and the probe's `fillPassword` (the one function
// that writes password fields). Hooked into test-ui.cjs, which passes its helpers:
//   ONLY=AgentScratchpad,AgentPasswordCard,AgentPasswordProbe node ui/dev/test-ui.cjs [shots]
'use strict';
const fs = require('fs');
const path = require('path');
const mockAi = require('./mock-ai.js');

// core's rules (agent::redact::probe_rules), as in test-agent-probe.cjs.
const RULES = {
  selector: 'input[type="password" i], input[autocomplete^="cc-" i], input[autocomplete*=" cc-" i], textarea[autocomplete^="cc-" i], textarea[autocomplete*=" cc-" i], select[autocomplete^="cc-" i], select[autocomplete*=" cc-" i]',
  namePattern: 'card.?num|cc.?(num|no)|credit.?card|cvv|cvc|csc|security.?code|card.?code|cc.?exp',
  nameAttributes: ['name', 'id', 'aria-label', 'placeholder'],
  luhn: true, minDigits: 13, maxDigits: 19, redacted: '[redacted]',
};

// A page with a sign-up form, a change-password form, a login form and a payment form.
const PW_PAGE = `<!doctype html><html><head><title>Password fixture</title>
<script>
  window.__ev = [];
  ['input', 'change', 'focus'].forEach((t) => document.addEventListener(t, (e) => { window.__ev.push(t + '#' + e.target.id); }, true));
</script></head><body>
<form id="signup" action="/log">
  <label>Username <input id="user" name="username" autocomplete="username" value="ann"></label>
  <label>Password <input id="pw" name="password" type="password" autocomplete="new-password"></label>
  <label>Confirm password <input id="pw2" name="password2" type="password" autocomplete="new-password"></label>
  <input id="hid" type="password" style="display:none">
  <button type="submit">Create account</button>
</form>
<form id="change" action="/log">
  <label for="cur">Current password</label><input id="cur" name="current" type="password" value="old-SECRET">
  <label for="new1">New password</label><input id="new1" name="new1" type="password">
  <label for="new2">Repeat it</label><input id="new2" name="new2" type="password">
  <input id="ro" name="token" type="password" readonly value="fixed">
  <button>Change password</button>
</form>
<form id="login" action="/log">
  <input id="luser" name="login" autocomplete="username">
  <input id="lpw" name="pass" type="password" autocomplete="current-password">
  <button>Sign in</button>
</form>
<form id="pay" action="/log">
  <input id="cc" name="ccnum" autocomplete="cc-number" value="4111 1111 1111 1111">
  <input id="cvv" name="cvv" type="password" placeholder="CVV">
  <input id="pin" name="x" type="password" autocomplete="cc-csc">
  <button>Pay</button>
</form>
</body></html>`;

// Model output is untrusted: every string here must end up as text.
const EVIL = '<img src=x onerror="window.__xss=1"><b>bold</b>';

const SCRATCH = {
  plan: [
    { id: '1', text: 'Open the shop', state: 'done' },
    { id: 'prices', text: 'Collect the prices ' + EVIL, state: 'doing' },
    { id: '3', text: 'Write the summary', state: 'todo' },
    { id: '4', text: 'Ask for a coupon', state: 'dropped' },
  ],
  notes: [
    { name: 'prices', bytes: 29, content: 'apple 1.20 EUR\npear 2.00 EUR\n' + EVIL },
    { name: 'open-questions', bytes: 0, content: '' },
    { name: 'big.txt', bytes: 20480, content: 'x'.repeat(20480) },
  ],
  bytes: 20509,
};

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot } = h;
  const count = async (page, cmd) => (await sent(page)).filter((m) => m.cmd === cmd).length;
  const settle = (page) => page.evaluate(() => new Promise((r) => setTimeout(() => requestAnimationFrame(() => setTimeout(r, 0)), 20)));
  const ai = (page, fn, ...args) => page.evaluate(([f, a]) => window.__ai[f](...a), [fn, args]);

  async function testAgentScratchpad(browser) {
    console.log('ai panel: scratchpad');
    const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/agent.html', { width: 380, height: 700 }, 1, mockAi);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await ai(page, 'config');
    await settle(page);

    // Closed by default; the header button opens it.
    check(await page.isHidden('#scratch') && await page.isVisible('#scratch-btn'), 'the scratchpad view is closed; the header has its button');
    check(await page.getAttribute('#scratch-btn', 'aria-expanded') === 'false' && !(await page.evaluate(() => document.getElementById('scratch-btn').classList.contains('has'))),
      'the button is not marked while the scratchpad is empty');
    await page.click('#scratch-btn');
    check(await page.isVisible('#scratch') && await page.getAttribute('#scratch-btn', 'aria-expanded') === 'true', 'a click opens the view');
    check(await page.isVisible('#sp-empty') && await page.isHidden('#sp-plan-box') && await page.isHidden('#sp-notes-box') && await page.isDisabled('#sp-clear'),
      'empty: a hint, no plan, no notes, Clear disabled');
    check((await page.textContent('#sp-empty')).includes('memory only'), 'the hint says it is kept in memory only');
    await shot(page, 'ai-scratch-empty.png');

    // The plan and the notes from an event.
    await emit(page, { event: 'agentScratch', scratch: SCRATCH });
    await settle(page);
    const plan = await page.evaluate(() => Array.from(document.querySelectorAll('#sp-plan .sp-item')).map((li) => ({
      cls: li.className, state: li.querySelector('.sp-state').textContent, text: li.querySelector('.sp-text').textContent,
    })));
    check(plan.length === 4 && plan.map((p) => p.state).join('|') === 'Done|Doing|To do|Dropped', 'the plan: one row per item with its state (' + plan.map((p) => p.state).join('|') + ')');
    check(plan[0].cls.includes('done') && plan[1].cls.includes('doing') && plan[2].cls.includes('todo') && plan[3].cls.includes('dropped'), 'each row carries its state as a class');
    check(plan[1].text === 'Collect the prices ' + EVIL, 'an item\'s text is shown as it is');
    const struck = await page.evaluate(() => getComputedStyle(document.querySelector('#sp-plan .sp-item.dropped .sp-text')).textDecorationLine);
    check(struck === 'line-through', 'a dropped item is struck through');
    const notes = await page.evaluate(() => Array.from(document.querySelectorAll('#sp-notes .sp-note')).map((n) => ({
      name: n.querySelector('.sp-note-name').textContent, size: n.querySelector('.sp-note-size').textContent,
      open: n.querySelector('.sp-note-head').getAttribute('aria-expanded'), hidden: n.querySelector('.sp-note-body').hidden,
    })));
    check(notes.length === 3 && notes[0].name === 'prices' && notes[0].size === '29 bytes' && notes[1].size === '0 bytes' && notes[2].size === '20 KiB',
      'the notes: name and size (' + notes.map((n) => n.name + ' ' + n.size).join(', ') + ')');
    check(notes.every((n) => n.open === 'false' && n.hidden), 'note bodies are collapsed');
    check((await page.textContent('#sp-size')) === '4 plan items · 3 notes, 20 KiB', 'the head line sums it up (' + (await page.textContent('#sp-size')) + ')');
    check(await page.isHidden('#sp-empty') && !(await page.isDisabled('#sp-clear')), 'the hint is gone, Clear is enabled');
    check(await page.evaluate(() => document.getElementById('scratch-btn').classList.contains('has')) &&
      (await page.getAttribute('#scratch-btn', 'aria-label')).includes('4 plan items, 3 notes'), 'the header button is marked and names what is there');

    // A note opens on a click; its content is text.
    await page.click('.sp-note-head[data-name="prices"]');
    const body = await page.evaluate(() => {
      const b = document.querySelector('.sp-note-head[data-name="prices"]').nextElementSibling;
      return { hidden: b.hidden, text: b.textContent, tag: b.localName, kids: b.children.length };
    });
    check(!body.hidden && body.tag === 'pre' && body.text === SCRATCH.notes[0].content && body.kids === 0, 'a click shows the note\'s content, line breaks kept');
    await page.click('.sp-note-head[data-name="open-questions"]');
    check((await page.textContent('.sp-note-head[data-name="open-questions"] + .sp-note-body')) === '(empty)', 'an empty note says so');
    const xss = await page.evaluate(() => ({
      flag: window.__xss, imgs: document.querySelectorAll('#scratch img').length, bold: document.querySelectorAll('#scratch b').length,
    }));
    check(xss.flag === undefined && xss.imgs === 0 && xss.bold === 0, 'HTML in the plan and in notes stays text (no element is made of it)');
    await shot(page, 'ai-scratch-open.png');

    // Live updates: a state change and new note text arrive while the view is open.
    const next = JSON.parse(JSON.stringify(SCRATCH));
    next.plan[1].state = 'done';
    next.plan[2].state = 'doing';
    next.notes[0].content += '\nplum 3.00 EUR';
    next.notes[0].bytes = 43;
    next.notes.splice(1, 1); // the empty note was deleted
    next.notes.push({ name: 'weird <i>name</i>', bytes: 3, content: 'abc' });
    await page.focus('.sp-note-head[data-name="prices"]');
    await emit(page, { event: 'agentScratch', scratch: next });
    await settle(page);
    const live = await page.evaluate(() => ({
      states: Array.from(document.querySelectorAll('#sp-plan .sp-state')).map((s) => s.textContent).join('|'),
      names: Array.from(document.querySelectorAll('.sp-note-name')).map((s) => s.textContent),
      pricesOpen: !document.querySelector('.sp-note-head[data-name="prices"]').nextElementSibling.hidden,
      prices: document.querySelector('.sp-note-head[data-name="prices"]').nextElementSibling.textContent,
      focus: document.activeElement && document.activeElement.dataset.name,
      italics: document.querySelectorAll('#scratch i').length,
    }));
    check(live.states === 'Done|Done|Doing|Dropped', 'a new event updates the plan in place (' + live.states + ')');
    check(live.names.join(',') === 'prices,big.txt,weird <i>name</i>' && live.italics === 0, 'notes are added and removed; an odd name is text');
    check(live.pricesOpen && live.prices.endsWith('plum 3.00 EUR'), 'an open note stays open and shows the new text');
    check(live.focus === 'prices', 'the keyboard focus stays on its note');
    await page.keyboard.press('Enter');
    check(await page.getAttribute('.sp-note-head[data-name="prices"]', 'aria-expanded') === 'false', 'Enter on a note\'s row closes it again');

    // The view sits above the conversation, which stays usable; no sideways scrolling.
    await emit(page, { event: 'agentMessage', role: 'user', text: 'Compare the prices' });
    await ai(page, 'state', 'running', 2);
    await ai(page, 'step', 1, 'ok', { tool: 'note_write', label: 'Write the note prices', summary: 'Wrote note prices (43 bytes)' });
    await settle(page);
    const lay = await page.evaluate(() => {
      const s = document.getElementById('scratch').getBoundingClientRect(), l = document.getElementById('log').getBoundingClientRect();
      return { above: s.bottom <= l.top + 1, share: s.height / innerHeight, logH: l.height, over: document.getElementById('scratch').scrollWidth - document.getElementById('scratch').clientWidth };
    });
    check(lay.above && lay.share <= 0.56 && lay.logH > 100, 'the view is above the conversation and leaves it room (' + JSON.stringify(lay) + ')');
    check(lay.over <= 0, 'no horizontal scrolling in the view');
    check(await page.isVisible('.step[data-n="1"]') && await page.isVisible('#stop'), 'the conversation goes on below it');
    await page.setViewportSize({ width: 280, height: 700 });
    await settle(page);
    check(await page.evaluate(() => document.getElementById('scratch').scrollWidth - document.getElementById('scratch').clientWidth) <= 0, '280 px wide: no horizontal scrolling');
    await shot(page, 'ai-scratch-280.png');
    await page.setViewportSize({ width: 380, height: 700 });
    await emit(page, { event: 'theme', theme: 'dark' });
    await settle(page);
    await shot(page, 'ai-scratch-dark.png');
    await emit(page, { event: 'theme', theme: 'light' });

    // Clear: posts the request at once (no question), the view is empty.
    const stops = await count(page, 'agentStop');
    await page.click('#sp-clear');
    await settle(page);
    const cl = await lastSent(page, 'agentScratchClear');
    check(cl !== undefined && Object.keys(cl).join(',') === 'cmd' && await count(page, 'agentScratchClear') === 1, 'Clear posts agentScratchClear');
    check(await page.locator('#sp-plan .sp-item').count() === 0 && await page.locator('.sp-note').count() === 0 && await page.isVisible('#sp-empty'), 'the view is empty at once');
    check(await page.locator('#lb-dialog-ok').count() === 0 && await count(page, 'agentStop') === stops && await page.isVisible('#stop'), 'Clear asks nothing and does not stop the task');
    check(await page.isVisible('#scratch') && await page.isDisabled('#sp-clear'), 'the view stays open; Clear is disabled again');
    // The host confirms with an empty scratchpad; a later change shows again.
    await emit(page, { event: 'agentScratch', scratch: { plan: [], notes: [], bytes: 0 } });
    await emit(page, { event: 'agentScratch', scratch: { plan: [{ id: '1', text: 'Start over', state: 'todo' }], notes: [], bytes: 0 } });
    await settle(page);
    check(await page.locator('#sp-plan .sp-item').count() === 1 && await page.isHidden('#sp-notes-box'), 'the agent\'s next plan shows up after Clear');

    // A new conversation empties it; a malformed event does no harm.
    await ai(page, 'state', 'done', 2);
    await emit(page, { event: 'agentClear' });
    await settle(page);
    check(await page.locator('#sp-plan .sp-item').count() === 0 && await page.isVisible('#sp-empty'), 'agentClear (New conversation) empties the scratchpad view');
    await emit(page, { event: 'agentScratch', scratch: { plan: 'nope', notes: [null, { name: 5 }, { name: 'ok', content: 7 }], bytes: 'x' } });
    await emit(page, { event: 'agentScratch' });
    await emit(page, { event: 'agentScratch', scratch: { plan: [{ text: 'odd state', state: '<script>' }, { state: 'done' }], notes: [] } });
    await settle(page);
    const odd = await page.evaluate(() => Array.from(document.querySelectorAll('#sp-plan .sp-item')).map((li) => li.className + '|' + li.textContent));
    check(odd.length === 1 && odd[0] === 'sp-item todo|To doodd state', 'a malformed event is ignored or shown safely (' + odd.join(' ; ') + ')');
    await page.click('#scratch-btn');
    check(await page.isHidden('#scratch') && await page.getAttribute('#scratch-btn', 'aria-expanded') === 'false', 'the button closes the view again');

    check(external.length === 0, 'no network requests (' + external.join(' ') + ')');
    check(errors.length === 0, 'no console errors and no dialogs (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  // ---- passwords on request: the consent and the card ----

  async function testAgentPasswordCard(browser) {
    console.log('ai panel: password consent and card');
    const SECRET = 'Gen-7Kq!x&<b>9~pw';
    const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/agent.html', { width: 380, height: 700 }, 1, mockAi);
    await page.evaluate((secret) => {
      const host = window.__host;
      host.autoSimulate = false;
      host.handlers = host.handlers || {};
      host.copies = [];
      // The panel must not write the clipboard itself (the host does, privately).
      window.__clipWrites = 0;
      if (navigator.clipboard) navigator.clipboard.writeText = function () { window.__clipWrites++; return Promise.resolve(); };
      const exec = document.execCommand.bind(document);
      document.execCommand = function (c) { if (/copy|cut/i.test(c)) window.__clipWrites++; return exec.apply(null, arguments); };
      host.handlers.agentPasswordShow = (m) => { if (m.id === 3) return { password: secret }; throw new Error('This password is no longer kept.'); };
      host.handlers.agentPasswordCopy = (m) => { if (m.id !== 3) throw new Error('This password is no longer kept.'); host.copies.push(m.id); return null; };
    }, SECRET);
    await ai(page, 'config');
    await ai(page, 'state', 'asking', 0);

    // The consent names the site; the answer is for that site.
    const SITE = 'https://shop.example';
    await emit(page, { event: 'agentConsentAsk', ask: 21, category: 'passwords', label: 'entering a password on a site', site: SITE });
    await settle(page);
    const q = await page.locator('.card.consent').last().textContent();
    check(q.includes('The agent wants to enter a password on https://shop.example. Allow?') && !q.includes('wants to read'), 'the consent names the site: "' + q.slice(0, 80) + '"');
    await shot(page, 'ai-password-consent.png');
    await page.locator('.card.consent:not(.done) button:has-text("Allow")').click();
    let cs = await lastSent(page, 'agentConsent');
    check(cs && cs.ask === 21 && cs.allow === true, 'Allow posts agentConsent{ask, allow:true}');
    check((await page.locator('.card.consent.done').last().textContent()).includes('Allowed on this site for this conversation'), 'the answer says it holds for this site');
    await emit(page, { event: 'agentConsentAsk', ask: 22, category: 'passwords', label: 'x', site: 'https://evil.example/<img src=x onerror="window.__xss=1">' });
    await settle(page);
    check(await page.locator('.card.consent img').count() === 0 && (await page.locator('.card.consent').last().textContent()).includes('<img src=x'), 'a site name is text');
    await page.locator('.card.consent:not(.done) button:has-text("Deny")').click();
    cs = await lastSent(page, 'agentConsent');
    check(cs.ask === 22 && cs.allow === false, 'Deny posts allow:false');
    await emit(page, { event: 'agentConsentAsk', ask: 23, category: 'passwords', label: 'x' });
    await emit(page, { event: 'agentConsentAsk', ask: 24, category: 'history', label: 'your browsing history' });
    await settle(page);
    const two = await page.locator('.card.consent:not(.done) .card-q').allTextContents();
    check(two[0].includes('enter a password on this site') && two[1] === 'The agent wants to read your browsing history. Allow?', 'no site: "this site"; the other consents read as before');
    await ai(page, 'state', 'running', 1);

    // The card of a generated password: masked, the value is not in the page.
    await emit(page, { event: 'agentPassword', id: 3, site: SITE + '<i>x</i>', length: 17, fields: 2 });
    await settle(page);
    const card = page.locator('.card.password').first();
    check(await page.locator('.card.password').count() === 1 && (await card.locator('.card-q').textContent()) === 'Krunk created a password for ' + SITE + '<i>x</i>' && await card.locator('i').count() === 0,
      'a card names the site (as text)');
    const masked = await card.locator('.pw-value').textContent();
    check(masked === '\u2022'.repeat(17) && await card.locator('.pw-value.masked').count() === 1, 'the password is masked by default (' + masked.length + ' dots)');
    check((await card.locator('.pw-note').textContent()).includes('Entered into 2 fields') && (await card.locator('.pw-note').textContent()).includes('saves it with your passwords'),
      'the card says where it went and that sending the form saves it');
    check(!(await page.content()).includes('Gen-7Kq') && await count(page, 'agentPasswordShow') === 0, 'the value is not in the panel before Show');
    await shot(page, 'ai-password-card.png');

    // Show asks the host; the value is text; Hide drops it again.
    await card.locator('.pw-show').click();
    await page.waitForFunction(() => !document.querySelector('.pw-value').classList.contains('masked'));
    const sh = await lastSent(page, 'agentPasswordShow');
    check(sh && sh.id === 3 && typeof sh.reqId === 'number', 'Show requests agentPasswordShow{id}');
    check((await card.locator('.pw-value').textContent()) === SECRET && await card.locator('.pw-value b').count() === 0 && await card.locator('.pw-show').textContent() === 'Hide',
      'the reply\'s password is shown as text; the button says Hide');
    await shot(page, 'ai-password-shown.png');
    await card.locator('.pw-show').click();
    check((await card.locator('.pw-value').textContent()) === '\u2022'.repeat(17) && !(await page.content()).includes('Gen-7Kq') && await card.locator('.pw-show').textContent() === 'Show',
      'Hide masks it again and the value is gone from the page');

    // Copy is the host's job (its clipboard, cleared after 30 s).
    await card.locator('.pw-copy').click();
    await page.waitForFunction(() => document.querySelector('.pw-status').textContent !== '');
    const cp = await lastSent(page, 'agentPasswordCopy');
    check(cp && cp.id === 3 && (await page.evaluate(() => window.__host.copies.length)) === 1, 'Copy requests agentPasswordCopy{id}');
    check((await card.locator('.pw-status').textContent()).includes('Copied') && !(await page.content()).includes('Gen-7Kq'), 'the card says it was copied; the value did not come to the panel for it');
    check((await page.evaluate(() => window.__clipWrites)) === 0, 'the panel itself writes nothing to the clipboard');

    // A password the host no longer has.
    await emit(page, { event: 'agentPassword', id: 9, site: SITE, length: 3, fields: 1 });
    await settle(page);
    const gone = page.locator('.card.password').nth(1);
    check((await gone.locator('.pw-value').textContent()).length === 8 && (await gone.locator('.pw-note').textContent()).includes('Entered into 1 field.'), 'the dots don\'t give a short length away; "1 field"');
    await gone.locator('.pw-show').click();
    await page.waitForFunction(() => document.querySelectorAll('.pw-status')[1].textContent !== '');
    check((await gone.locator('.pw-status').textContent()) === 'This password is no longer kept.' && await gone.locator('.pw-value.masked').count() === 1, 'a failed Show says why and stays masked');
    await gone.locator('.pw-copy').click();
    await page.waitForTimeout(60);
    check((await gone.locator('.pw-status').textContent()) === 'This password is no longer kept.', 'a failed Copy says why');

    // Narrow, dark, and a new conversation.
    await page.setViewportSize({ width: 280, height: 700 });
    await card.locator('.pw-show').click();
    await page.waitForFunction(() => !document.querySelector('.pw-value').classList.contains('masked'));
    const over = await page.evaluate(() => document.getElementById('log').scrollWidth - document.getElementById('log').clientWidth);
    check(over <= 0, '280 px wide with the password shown: no horizontal scrolling (' + over + ')');
    await emit(page, { event: 'theme', theme: 'dark' });
    await settle(page);
    await shot(page, 'ai-password-dark-280.png');
    await emit(page, { event: 'theme', theme: 'light' });
    await page.setViewportSize({ width: 380, height: 700 });
    await ai(page, 'state', 'done', 1);
    await emit(page, { event: 'agentClear' });
    await settle(page);
    check(await page.locator('.card.password').count() === 0 && !(await page.content()).includes('Gen-7Kq'), 'a new conversation removes the cards and a shown password');
    check((await page.evaluate(() => window.__xss)) === undefined, 'nothing from an event ran as script');

    check(external.length === 0, 'no network requests (' + external.join(' ') + ')');
    check(errors.length === 0, 'no console errors and no dialogs (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  // ---- passwords on request: the probe ----

  async function testAgentPasswordProbe(browser) {
    console.log('agent probe: fillPassword');
    const src = fs.readFileSync(path.join(__dirname, '..', 'inject', 'agent-probe.js'), 'utf8');
    const context = await browser.newContext({ viewport: { width: 900, height: 800 } });
    await context.route('**/*', (r) => r.abort());
    await context.route('https://shop.example/', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: PW_PAGE }));
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.goto('https://shop.example/');
    const cdp = await context.newCDPSession(page);
    const tree = await cdp.send('Page.getFrameTree');
    const world = await cdp.send('Page.createIsolatedWorld', { frameId: tree.frameTree.frame.id, worldName: 'krunk-agent', grantUniveralAccess: false });
    const ctx = world.executionContextId;
    await cdp.send('Runtime.evaluate', { contextId: ctx, expression: '(' + src + ')(' + JSON.stringify(RULES) + ') && true', returnByValue: true });
    const call = async (fn, ...args) => {
      const r = await cdp.send('Runtime.evaluate', { contextId: ctx, expression: '__krunkProbe.' + fn + '(...' + JSON.stringify(args) + ')', returnByValue: true, awaitPromise: true });
      if (r.exceptionDetails) return { exception: r.exceptionDetails.exception ? r.exceptionDetails.exception.description : r.exceptionDetails.text };
      return r.result.value;
    };
    const vals = () => page.evaluate(() => {
      const o = {};
      document.querySelectorAll('input').forEach((i) => { o[i.id] = i.value; });
      return o;
    });
    const events = async () => { const e = await page.evaluate(() => window.__ev.splice(0)); return e.join(' '); };
    const GEN = 'Gen-4Tz!q#Xw9~mB2$';
    const before = await vals();

    // A dry run: the fields and what the browser needs to judge the form; nothing is written.
    const dry = await call('fillPassword', null, '#signup', true);
    check(JSON.stringify(dry.fields) === '["#pw","#pw2"]' && dry.autocomplete === 'new-password' && dry.passwords === 2 && /Create account/.test(dry.formText),
      'dry run on the sign-up form: both new-password fields, the autocomplete hint, the count and the form\'s text (' + JSON.stringify(dry) + ')');
    check(JSON.stringify(await vals()) === JSON.stringify(before) && (await events()) === '', 'a dry run writes nothing and fires nothing');
    const dryField = await call('fillPassword', null, '#pw2', true);
    check(JSON.stringify(dryField.fields) === '["#pw","#pw2"]', 'a field as the selector means its form (the confirmation too)');

    // A generated password: the new-password field and its confirmation.
    const g = await call('fillPassword', GEN, '#signup', true);
    let v = await vals();
    check(g.filled === 2 && JSON.stringify(g.fields) === '["#pw","#pw2"]' && v.pw === GEN && v.pw2 === GEN, 'generated: the password and its confirmation are filled');
    check(v.user === 'ann' && v.hid === '' && v.lpw === '' && v.new1 === '', 'the username, a hidden password field and the other forms are untouched');
    const ev = await events();
    check(ev === 'input#pw change#pw input#pw2 change#pw2', 'input and change fire on each field, and no focus (' + ev + ')');
    check(!JSON.stringify(g).includes('Gen-4Tz'), 'the result names the fields, never the value');

    // A change-password form: not the current password, not a read-only field.
    const dc = await call('fillPassword', null, '#change', true);
    check(JSON.stringify(dc.fields) === '["#new1","#new2"]' && dc.passwords === 4 && /Change password/.test(dc.formText), 'change form: only the new password and its repeat are candidates (' + JSON.stringify(dc.fields) + ')');
    const gc = await call('fillPassword', GEN, '#new1', true);
    v = await vals();
    check(gc.filled === 2 && v.new1 === GEN && v.new2 === GEN && v.cur === 'old-SECRET' && v.ro === 'fixed', 'the current password and the read-only field keep their values');
    await events();

    // A login form has no field for a new password (autocomplete=current-password).
    const dl = await call('fillPassword', null, '#login', true);
    check(dl.fields.length === 0 && dl.passwords === 1 && dl.autocomplete === '', 'login form: no new-password candidate (the browser refuses to generate there)');
    const gl = await call('fillPassword', GEN, '#login', true);
    check(!!gl.error && (await vals()).lpw === '', 'generated on a login form: an error, nothing written');

    // A password the user gave: exactly the field named.
    const pr = await call('fillPassword', 'user-GAVE-this', '#lpw', false);
    v = await vals();
    check(pr.filled === 1 && JSON.stringify(pr.fields) === '["#lpw"]' && v.lpw === 'user-GAVE-this' && v.luser === '', 'provided: the named password field is filled, the username is not');
    check((await events()) === 'input#lpw change#lpw' && !JSON.stringify(pr).includes('GAVE'), 'its events fire; the result has no value');
    const pf = await call('fillPassword', 'second', '#login', false);
    check(pf.filled === 1 && (await vals()).lpw === 'second', 'provided with a form as the selector: its password field');
    await events();

    // Never a card field, never anything that is not a password field.
    const snap = await vals();
    const r1 = await call('fillPassword', 'x1', '#cvv', false);
    const r2 = await call('fillPassword', 'x2', '#pin', false);
    const r3 = await call('fillPassword', 'x3', '#cc', false);
    const r4 = await call('fillPassword', 'x4', '#user', false);
    const r5 = await call('fillPassword', 'x5', '#pay', false);
    const r6 = await call('fillPassword', 'x6', '#lpw, #user', false);
    check(/card field/.test(r1.refused) && /card field/.test(r2.refused), 'a security code typed as a password is refused (by its name, by its autocomplete)');
    check(/only writes password fields/.test(r3.refused) && /only writes password fields/.test(r4.refused), 'a card number field and a text field are refused');
    check(/card field/.test(r5.refused), 'a payment form as the selector is refused');
    check(/only writes password fields/.test(r6.refused) && (await vals()).lpw === 'second', 'one wrong field refuses the whole call: nothing is written');
    const r7 = await call('fillPassword', GEN, '#cvv', true);
    const r8 = await call('fillPassword', GEN, '#user', true);
    const r9 = await call('fillPassword', GEN, '#pay', true);
    check(/card field/.test(r7.refused) && /only writes password fields/.test(r8.refused) && !!r9.error, 'generated: card and text fields refused; a payment form has no candidate');
    check(JSON.stringify(await vals()) === JSON.stringify(snap) && (await events()) === '', 'none of the refused calls wrote or fired anything');
    const ro = await call('fillPassword', 'x', '#ro', false);
    check(!!ro.error && (await vals()).ro === 'fixed', 'a read-only password field is not written');

    // Without a selector: several forms need one; the focused password field is taken.
    const amb = await call('fillPassword', null, null, true);
    check(/several forms/.test(amb.error || ''), 'no selector and several forms: an error asks for one');
    await page.evaluate(() => { document.getElementById('lpw').focus(); window.__ev.splice(0); });
    const fo = await call('fillPassword', 'focused-PW', null, false);
    check(fo.filled === 1 && (await vals()).lpw === 'focused-PW', 'no selector: the focused password field');
    await page.evaluate(() => { document.getElementById('new1').focus(); });
    const fg = await call('fillPassword', null, null, true);
    check(JSON.stringify(fg.fields) === '["#new1","#new2"]', 'generated without a selector: the focused field\'s form');
    const bad = await call('fillPassword', 'x', 'input[', false);
    const none = await call('fillPassword', 'x', '#nothing', false);
    check(bad.exception && /valid selector/.test(bad.exception) && /no element/.test(none.error), 'a bad selector and no match are errors');

    // What was filled stays redacted for every reading tool.
    const sec = await call('secrets');
    check(sec.includes(GEN) && sec.includes('focused-PW'), 'secrets() has the filled values (the browser scrubs them from every result)');
    const forms = JSON.stringify(await call('forms'));
    const html = (await call('html', null, false)).html;
    const q = JSON.stringify(await call('query', 'input[type=password]', 20, 0));
    const ctxt = JSON.stringify(await call('context'));
    check(![forms, html, q, ctxt].some((t) => t.includes('Gen-4Tz') || t.includes('focused-PW')), 'forms, html, query and context show no filled password');
    check(forms.includes('[redacted]') && html.includes('data-krunk-redacted'), 'they show the fields as redacted');
    const fill = await call('fill', { '#pw': 'via-fill', '#lpw': 'via-fill' });
    check(/^refused/.test(fill.results['#pw']) && /^refused/.test(fill.results['#lpw']) && (await vals()).pw === GEN, 'the normal fill still refuses password fields');

    // One form on the page: no selector needed; a given password needs the field when there are two.
    await page.evaluate(() => { ['change', 'login', 'pay'].forEach((id) => document.getElementById(id).remove()); document.activeElement.blur(); window.__ev.splice(0); });
    const one = await call('fillPassword', 'Gen-second-1!', null, true);
    v = await vals();
    check(one.filled === 2 && v.pw === 'Gen-second-1!' && v.pw2 === 'Gen-second-1!', 'the page\'s only password form is found without a selector');
    const two = await call('fillPassword', 'x', null, false);
    check(/several password fields/.test(two.error || '') && (await vals()).pw === 'Gen-second-1!', 'a given password is not guessed into one of two fields');
    await page.evaluate(() => document.getElementById('pw2').remove());
    const only = await call('fillPassword', 'the-only-one', null, false);
    check(only.filled === 1 && (await vals()).pw === 'the-only-one', 'the page\'s only password field takes a given password');
    await page.evaluate(() => document.getElementById('pw').remove());
    const nopw = await call('fillPassword', 'x', null, false);
    check(/no visible password field/.test(nopw.error || ''), 'a page without a password field: an error');

    check(errors.length === 0, 'no page errors (' + errors.join('; ') + ')');
    await context.close();
  }

  return { testAgentScratchpad, testAgentPasswordCard, testAgentPasswordProbe };
};
