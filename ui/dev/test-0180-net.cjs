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

  return { testNavMenuMiddleClick };
};
