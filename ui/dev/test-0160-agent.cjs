// Krunk 0.16.0 UI tests of the AI agent: the panel's scratchpad view (the agent's plan and notes,
// `agentScratch` events, Clear). Hooked into test-ui.cjs, which passes its helpers:
//   ONLY=AgentScratchpad node ui/dev/test-ui.cjs [shots]
'use strict';
const mockAi = require('./mock-ai.js');

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

  return { testAgentScratchpad };
};
