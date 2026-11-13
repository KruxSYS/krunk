// Krunk 0.18.0 UI tests: a middle-click in the Back/Forward menus (a new tab with the list
// copied) and the pending-requests column of krunk://tasks. Hooked into test-ui.cjs:
//   ONLY=NavMenuMiddleClick,TasksPending node ui/dev/test-ui.cjs [shots]
'use strict';

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot } = h;

  async function testNavMenuMiddleClick(browser) {
    console.log('navMenu: middle-click');
    const items = [3, 2, 1].map((i) => ({ entryId: 100 + i, label: 'Page ' + i, url: 'http://localhost:8094/p' + i + '.html', icon: null }));
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 340, height: 300 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(page, { event: 'popup', content: { kind: 'navMenu', tabId: 7, forward: false, items: items } });
    await page.waitForSelector('#root[data-kind="navMenu"] .item');
    const hides = (await sent(page)).filter((x) => x.cmd === 'hidePopup').length;
    await page.click('#root .item:nth-child(2)', { button: 'middle' });
    const m = await lastSent(page, 'navToEntryNewTab');
    check(m && m.tabId === 7 && m.entryId === 102, 'a middle-click posts navToEntryNewTab{tabId:7, entryId:102}');
    check((await lastSent(page, 'navToEntry')) === undefined, 'a middle-click does not jump in this tab');
    check((await sent(page)).filter((x) => x.cmd === 'hidePopup').length === hides && await page.$$eval('#root .item', (l) => l.length) === 4, 'the menu stays open');
    await page.click('#root .item:last-child', { button: 'middle' });
    check((await sent(page)).filter((x) => x.cmd === 'navToEntryNewTab').length === 1 && (await lastSent(page, 'command')) === undefined, 'a middle-click on "Show full history" does nothing');
    await page.click('#root .item:nth-child(2)', { button: 'right' });
    check((await sent(page)).filter((x) => x.cmd === 'navToEntryNewTab').length === 1, 'a right-click opens nothing');
    await page.click('#root .item:nth-child(2)');
    const j = await lastSent(page, 'navToEntry');
    check(j && j.entryId === 102 && (await sent(page)).filter((x) => x.cmd === 'hidePopup').length === hides + 1, 'a left-click still jumps in this tab and closes');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  }

  // krunk://tasks: the Pending column (requests under way, the oldest one's age).
  async function testTasksPending(browser) {
    console.log('tasks: pending requests');
    const row = (id, title, pending, oldest) => ({ id, kind: 'renderer', title: 'Tab: ' + title, pid: 1000 + id, cpu: 0, memory: 5e7, workingSet: 6e7, private: 5e7,
      network: 0, pending: pending, pendingOldestMs: oldest, killable: true, tabs: [{ id: id, title: title }] });
    const rows = [row(1, 'Idle page', 0, 0), row(2, 'Loading page', 3, 1200), row(3, 'Stuck page', 6, 45000), row(4, 'Stream', 1, 600000),
      { id: 9, kind: 'browser', title: 'Browser', pid: 1, cpu: 0, memory: 1e8, workingSet: 1e8, private: 1e8, network: 0, killable: false, tabs: [] }];
    const { page, errors } = await openPage(browser, 'https://krunk.invalid/tasks.html', { width: 1100, height: 500 }, 1,
      (r) => { window.__host.handlers.getTasks = () => r; }, rows);
    await page.waitForFunction(() => document.querySelectorAll('#rows tr').length === 5);
    const cells = await page.$$eval('#rows tr', (l) => Object.fromEntries(l.map((tr) => [tr.dataset.id, { text: tr.querySelector('td.pending').textContent, slow: tr.querySelector('td.pending').classList.contains('slow') }])));
    check(cells['1'].text === '–' && cells['9'].text === '–', 'no pending requests: a dash (also for a task without the field)');
    check(cells['2'].text === '3' && !cells['2'].slow, 'three young requests: "3"');
    check(cells['3'].text === '6 (45 s)' && cells['3'].slow, 'six with the oldest at 45 s: "6 (45 s)", marked');
    check(cells['4'].text === '1 (10 min)', 'a ten-minute stream: "1 (10 min)"');
    check(await page.$$eval('thead th', (l) => l.map((x) => x.textContent).join('|')) === 'Task|Type|Memory|Working set|Private|CPU|Network|Pending|PID', 'the column sits between Network and PID');
    await page.click('th[data-sort="pending"]');
    await page.waitForTimeout(50);
    check(await page.$eval('#rows tr:first-child', (tr) => tr.dataset.id) === '3', 'sorting by Pending puts the most first');
    const fits = await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
    check(fits, 'no horizontal scroll at 1100 px');
    await shot(page, 'tasks-pending.png');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  }

  return { testNavMenuMiddleClick, testTasksPending };
};
