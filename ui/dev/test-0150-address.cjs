// Krunk 0.15.0 (WP-D): search suggestions under the address bar. Hooked into test-ui.cjs
// (ONLY=AddressSearchSuggestions). The host side is mocked (mock-host.js `host.suggest`): the
// local rows come at once, the engine's suggestions a little later in addressSearchSuggestions;
// the shell must keep its selection and inline completion, and the popup shows the rows with
// the search glyph.
'use strict';

const DDG = (q) => 'https://duckduckgo.com/?q=' + encodeURIComponent(q).replace(/%20/g, '+');
const SUGG = ['why do we yawn', 'why do cats purr', "why don't we"];

// Runs in the page: the mocked host's answers for `why do` (the search row, one history page,
// then the engine's three suggestions).
function suggestMock(arg) {
  const ddg = (q) => 'https://duckduckgo.com/?q=' + encodeURIComponent(q).replace(/%20/g, '+');
  window.__host.suggest = {
    delayMs: arg.delayMs,
    rows: (text) => [
      { url: ddg(text), title: text, kind: 'search', fill: text },
      { url: 'https://whydo.example/', title: 'Why do things happen', kind: 'history', fill: 'https://whydo.example/' },
    ],
    more: (text) => text === 'why do' ? arg.sugg.map((s) => ({ url: ddg(s), title: s, kind: 'searchSuggestion', fill: s })) : [],
  };
}

module.exports = (h) => {
  const { openPage, check, lastSent, emit, shot } = h;

  async function testAddressSearchSuggestions(browser) {
    console.log('address bar: search suggestions');
    // ---- the shell: rows arrive in two steps ----
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1, suggestMock, { delayMs: 400, sugg: SUGG });
    await page.waitForSelector('#address');
    await page.click('#address');
    await page.keyboard.press('Control+A');
    await page.keyboard.type('why do');
    await page.waitForTimeout(150);
    check((await lastSent(page, 'addressSuggest')).text === 'why do', 'typing asks for suggestions');
    // The local rows are there; the user moves down once before the engine answers.
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(30);
    check((await lastSent(page, 'addressSuggestSelect')).selected === 0, 'Down selects the first local row');
    await page.waitForTimeout(450);
    // The engine's rows came: the selection stayed, Down goes on into them.
    check(await page.locator('#address').inputValue() === 'why do', 'the bar still shows the selected row (' + await page.locator('#address').inputValue() + ')');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(30);
    check((await lastSent(page, 'addressSuggestSelect')).selected === 2, 'Down reaches the first suggestion (selection kept when they arrived)');
    check(await page.locator('#address').inputValue() === SUGG[0], 'the bar shows the suggestion\'s words, not its URL (' + await page.locator('#address').inputValue() + ')');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(30);
    check((await lastSent(page, 'addressSuggestSelect')).selected === null && await page.locator('#address').inputValue() === 'why do', 'past the last suggestion: back to the typed text');
    await page.keyboard.press('ArrowUp');
    await page.waitForTimeout(30);
    check((await lastSent(page, 'addressSuggestSelect')).selected === 4, 'Up from the text selects the last suggestion');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(40);
    const pick = await lastSent(page, 'addressSuggestPick');
    check(pick && pick.url === DDG(SUGG[2]), 'Enter on a suggestion searches it (' + (pick && pick.url) + ')');

    // An answer for older text is ignored; so is one after Esc closed the list.
    await page.click('#address');
    await page.keyboard.press('Control+A');
    await page.keyboard.type('why do');
    await page.waitForTimeout(150);
    await emit(page, { event: 'addressSearchSuggestions', text: 'why', entries: [{ url: DDG('stale'), title: 'stale', kind: 'searchSuggestion', fill: 'stale' }] });
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(30);
    check((await lastSent(page, 'addressSuggestSelect')).selected === null, 'a stale answer adds nothing (two local rows only)');
    await page.keyboard.press('Escape');
    await page.waitForTimeout(450);
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(30);
    check(await page.locator('#address').inputValue() === 'why do', 'after Esc the late answer does not reopen the list');
    check(errors.length === 0, 'shell: no errors (' + errors.join('; ') + ')');

    // ---- the popup: the rows as shown ----
    const rowsAll = [
      { url: DDG('why do'), title: 'why do', kind: 'search', fill: 'why do' },
      { url: 'https://whydo.example/', title: 'Why do things happen', kind: 'history', fill: 'https://whydo.example/' },
      { url: 'https://www.rust-lang.org/', title: 'Rust', kind: 'bookmark', fill: 'https://www.rust-lang.org/' },
    ].concat(SUGG.map((s) => ({ url: DDG(s), title: s, kind: 'searchSuggestion', fill: s })));
    const height = 8 + 38 * rowsAll.length; // core::omnibox::height_css
    for (const theme of ['light', 'dark']) {
      const pp = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 560, height: height }, 1);
      const p = pp.page;
      await p.evaluate(() => { window.__host.autoSimulate = false; });
      await emit(p, { event: 'theme', theme: theme });
      await emit(p, { event: 'popup', content: { kind: 'addressSuggest', selected: 3, entries: rowsAll } });
      await p.waitForSelector('.suggest.addr');
      await p.waitForTimeout(60);
      await shot(p, 'address-search-suggestions-' + theme + '.png');
      const sugg = p.locator('.suggest.addr.search-suggestion');
      check(await sugg.count() === 3 && await p.locator('.suggest.addr').count() === 6, theme + ': three suggestion rows under the local ones');
      check(await sugg.nth(0).locator('.glyph').textContent() === '🔍' && await sugg.nth(0).locator('.url').textContent() === '', theme + ': search glyph, no address');
      check(await sugg.nth(0).locator('.title').textContent() === SUGG[0] && await sugg.nth(0).locator('.title b').textContent() === ' we yawn', theme + ': the typed words plain, the rest bold');
      check(await sugg.nth(2).locator('.title b').textContent() === "n't we", theme + ': a suggestion continuing the last word');
      check((await p.getAttribute('.suggest.addr.selected', 'data-index')) === '3', theme + ': the shell\'s selection is highlighted on a suggestion');
      const last = await p.locator('.suggest.addr').nth(5).boundingBox();
      check(last && last.y + last.height <= height, theme + ': all rows fit the height the host gives (' + (last && Math.round(last.y + last.height)) + ' <= ' + height + ')');
      await sugg.nth(1).click();
      const ap = await lastSent(p, 'addressSuggestPick');
      check(ap && ap.url === DDG(SUGG[1]), theme + ': a click on a suggestion searches it');
      check(pp.errors.length === 0, theme + ': popup: no errors (' + pp.errors.join('; ') + ')');
      await pp.context.close();
    }
    await page.context().close();
  }

  return { testAddressSearchSuggestions };
};
