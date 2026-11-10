// Krunk 0.15.0 (WP-E1): krunk://downloads as a windowed list over the host's pages
// (`queryDownloads`). Hooked into test-ui.cjs: ONLY=Downloads (the rows, actions, menus and live
// updates; was testDownloads in test-ui.cjs) and ONLY=DownloadsPaging (2,000 downloads: scrolling,
// groups, search, filters, a bounded DOM, light and dark screenshots). The host side is the mock
// in mock-host.js (`__host.mockDownloads`), which pages like core/src/downloads.rs.
'use strict';

module.exports = (h) => {
  const { openPage, check, lastSent, emit, shot } = h;
  const URL_ = 'https://krunk.invalid/downloads.html';
  const rows = (page) => page.locator('.dl:not(.placeholder)');
  const settle = (page) => page.waitForTimeout(260);

  async function testDownloads(browser) {
    console.log('downloads page (0.15.0)');
    const now = Date.now();
    const items = [
      { id: 3, url: 'https://releases.example.org/tool-2.4.1-x64.msi', fileName: 'tool-2.4.1-x64.msi', path: 'C:\\Users\\me\\Downloads\\tool-2.4.1-x64.msi', state: 'inProgress', received: 23500000, total: 61200000, started: now, reason: null },
      { id: 2, url: 'https://arxiv.org/pdf/1234.5678?download=1&token=abc', fileName: 'paper.pdf', path: 'C:\\Users\\me\\Downloads\\paper.pdf', state: 'completed', received: 2400000, total: 2400000, started: Date.UTC(2026, 10, 5, 9, 15), finished: Date.UTC(2026, 10, 5, 9, 16), pageUrl: 'https://arxiv.org/abs/1234.5678', reason: null },
      { id: 1, url: 'https://cdn.example.com/video.mp4', fileName: 'video.mp4', path: 'C:\\Users\\me\\Downloads\\video.mp4', state: 'interrupted', received: 1000, total: null, started: now - 120000, reason: 'Network error' },
    ];
    const { page, errors, external } = await openPage(browser, URL_, { width: 1000, height: 520 }, 1,
      (d) => { window.__host.mockDownloads(d); }, items);
    await page.waitForSelector('.dl:not(.placeholder)');
    check(await rows(page).count() === 3, 'lists 3 downloads');
    const order = await rows(page).evaluateAll((ns) => ns.sort((a, b) => a.dataset.index - b.dataset.index).map((n) => n.dataset.id).join(','));
    check(order === '3,1,2', 'newest first (' + order + ')');
    check(await page.locator('.dl-bar').count() === 1, 'progress bar for the running download');
    const kinds = await page.$$eval('.dl[data-id="3"] .dl-icon, .dl[data-id="2"] .dl-icon, .dl[data-id="1"] .dl-icon', (ns) => ns.map((n) => n.dataset.kind).sort());
    check(JSON.stringify(kinds) === JSON.stringify(['app', 'pdf', 'video']), 'file-type icons by extension (' + kinds.join(', ') + ')');
    check(await page.locator('.dl[data-id="2"] .site').textContent() === 'arxiv.org' &&
      await page.locator('.dl[data-id="2"] .site').getAttribute('title') === 'https://arxiv.org/pdf/1234.5678?download=1&token=abc', 'the site, with the full address as its tooltip');
    check(await page.locator('.dl[data-id="2"] .kind').textContent() === 'PDF' && (await page.locator('.dl[data-id="2"] .sub').textContent()).includes('2.3 MB'), 'type and size');
    const when2 = await page.locator('.dl[data-id="2"] .when').getAttribute('title');
    check(/2026/.test(when2) && /finished/.test(when2), 'date and time it started and finished (' + when2 + ')');
    check(!/finished/.test(await page.locator('.dl[data-id="1"] .when').getAttribute('title')), 'an entry without a finish time shows the start only');
    check(await page.locator('.dl[data-id="1"] .pill').textContent() === 'Failed' && (await page.locator('.dl[data-id="1"] .sub').textContent()).includes('Network error'), 'a failed download says so and why');
    check(await page.locator('.dl[data-id="1"] .act[aria-label="Retry"]').count() === 1, 'a failed download offers Retry');
    check(await page.locator('.groups, .dl-group').first().textContent() === 'Today', 'a "Today" group');
    const groups = await page.$$eval('.dl-group', (ns) => ns.map((n) => n.textContent));
    check(groups.includes('Today') && groups.includes('Earlier'), 'groups: ' + groups.join(', '));
    await shot(page, 'downloads.png');
    // Right-click: the row's menu (every action the page had).
    await page.locator('.dl[data-id="2"] .name').click({ button: 'right' });
    await page.waitForSelector('.ctx-menu');
    const mlabels = await page.$$eval('.ctx-menu .menu-item', (ns) => ns.map((n) => n.textContent + (n.disabled ? '(off)' : '')));
    check(JSON.stringify(mlabels) === JSON.stringify(['Open file', 'Show in folder', 'Copy download link', 'Copy page address', 'Download again', 'Remove from list', 'Delete file…']), 'context menu of a finished download (' + mlabels.join(', ') + ')');
    await shot(page, 'downloads-context-menu.png');
    await page.locator('.ctx-menu .menu-item', { hasText: 'Copy download link' }).click();
    check((await lastSent(page, 'downloadAction')).action === 'copyLink' && await page.locator('.ctx-menu').count() === 0, 'Copy download link sends downloadAction{copyLink} and closes the menu');
    await page.locator('.dl[data-id="2"] .name').click({ button: 'right' });
    await page.locator('.ctx-menu .menu-item', { hasText: 'Delete file' }).click();
    await page.waitForSelector('#lb-dialog-ok');
    check((await page.textContent('.lb-dialog-text')).includes('paper.pdf'), 'Delete file asks first');
    await page.click('#lb-dialog-ok');
    await page.waitForTimeout(50);
    check((await lastSent(page, 'downloadAction')).action === 'deleteFile', 'confirmed: downloadAction{deleteFile}');
    // The ⋯ button opens the same menu.
    await page.locator('.dl[data-id="2"]').hover();
    await page.locator('.dl[data-id="2"] .act-more').click();
    check(await page.locator('.ctx-menu .menu-item').count() === 7, 'the ⋯ button opens the menu');
    await page.keyboard.press('Escape');
    // A failed one: no file actions, no page address; the keyboard opens the menu too.
    await page.locator('.dl[data-id="1"]').focus();
    await page.waitForTimeout(100);
    await page.keyboard.press('Shift+F10');
    await page.waitForSelector('.ctx-menu');
    const flabels = await page.$$eval('.ctx-menu .menu-item', (ns) => ns.map((n) => n.textContent + (n.disabled ? '(off)' : '')));
    check(JSON.stringify(flabels) === JSON.stringify(['Open file(off)', 'Show in folder(off)', 'Copy download link', 'Copy page address(off)', 'Retry', 'Remove from list', 'Delete file…(off)']), 'context menu of a failed download (' + flabels.join(', ') + ')');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Escape');
    check(await page.locator('.ctx-menu').count() === 0, 'Esc closes the menu');
    // Arrow keys move between rows.
    await page.locator('.dl[data-id="3"]').focus();
    await page.keyboard.press('ArrowDown');
    check(await page.evaluate(() => document.activeElement.dataset.id) === '1', 'ArrowDown focuses the next row');
    // Live progress: only the running ones, updated in place.
    const rowBefore = await page.$('.dl[data-id="3"]');
    await emit(page, { event: 'downloads', items: [Object.assign({}, items[0], { received: 30000000, speed: 2048, eta: 75 })] });
    await page.waitForTimeout(40);
    const sub = await page.locator('.dl[data-id="3"] .sub').textContent();
    check(sub.includes('2.0 KB/s') && sub.includes('1 min left'), 'progress shows speed and time left (' + sub + ')');
    check(await rowBefore.evaluate((n) => n.isConnected), 'a row that only progresses is kept');
    await emit(page, { event: 'downloads', items: [Object.assign({}, items[0], { received: 100, total: 1000, connections: 4, speed: 2048, eta: 75, partProgress: [{ done: 100, len: 250, start: 0, conn: 0 }, { done: 0, len: 250, start: 250, conn: 1 }, { done: 0, len: 250, start: 500, conn: 2 }, { done: 0, len: 250, start: 750, conn: 3 }] })] });
    await page.waitForTimeout(60);
    check((await page.textContent('.dl[data-id="3"] .sub')).includes('4 connections'), 'a download in parts shows its connections');
    const segs = await page.$$eval('.dl[data-id="3"] .dl-segments .seg', (ns) => ns.map((n) => n.style.left + '/' + n.style.width + '/' + n.className + '/' + Number(/scaleX\(([\d.]+)\)/.exec(n.firstChild.style.transform)[1])));
    check(segs.length === 4 && segs[0] === '0%/25%/seg c0/0.4' && segs[2] === '50%/25%/seg c2/0', 'each part at its offset, coloured by connection (' + segs.join(' ; ') + ')');
    await page.locator('.dl[data-id="3"] .name').click({ button: 'right' });
    const rlabels = await page.$$eval('.ctx-menu .menu-item', (ns) => ns.map((n) => n.textContent + (n.disabled ? '(off)' : '')));
    check(JSON.stringify(rlabels) === JSON.stringify(['Pause', 'Cancel', 'Copy download link', 'Copy page address(off)']), 'context menu of a running download (' + rlabels.join(', ') + ')');
    await page.locator('.ctx-menu .menu-item', { hasText: 'Pause' }).click();
    check((await lastSent(page, 'downloadAction')).action === 'pause', 'Pause from the menu');
    await page.mouse.click(5, 5);
    await page.locator('.dl[data-id="2"]').hover();
    await page.locator('.dl[data-id="2"] .act[aria-label="Show in folder"]').click();
    const a = await lastSent(page, 'downloadAction');
    check(a && a.id === 2 && a.action === 'showInFolder', 'Show in folder sends downloadAction');
    await page.locator('.dl[data-id="2"] .name').click();
    check((await lastSent(page, 'downloadAction')).action === 'open', 'a click on a finished name opens the file');
    await page.locator('.dl[data-id="3"] .act[aria-label="Cancel"]').click();
    check((await lastSent(page, 'downloadAction')).action === 'cancel', 'Cancel sends downloadAction{cancel}');
    // The list changed (the host says so): the page fetches its rows again.
    await page.evaluate(() => { const d = window.__host.dl.items.find((x) => x.id === 3); d.state = 'completed'; d.received = d.total; });
    await emit(page, { event: 'downloadsChanged', active: 0 });
    await settle(page);
    check(await page.locator('.dl-bar, .dl-segments').count() === 0, 'downloadsChanged: the rows are fetched again');
    check(await rowBefore.evaluate((n) => !n.isConnected), 'a row that changes state is rebuilt');
    const keep = await page.$('.dl[data-id="2"]');
    await emit(page, { event: 'downloadsChanged', active: 0 });
    await settle(page);
    check(await keep.evaluate((n) => n.isConnected), 'an unchanged row is kept in place');
    // Remove, then clear.
    await page.locator('.dl[data-id="1"]').hover();
    await page.locator('.dl[data-id="1"] .act[aria-label="Remove from list"]').click();
    await emit(page, { event: 'downloadsChanged', active: 0 });
    await settle(page);
    check(await rows(page).count() === 2, 'Remove from list');
    await page.click('#clear');
    await settle(page);
    check((await lastSent(page, 'clearDownloads')) !== undefined, 'Clear completed sends clearDownloads');
    check((await page.textContent('.empty strong')) === 'No downloads yet' && await page.locator('.dl').count() === 0, 'an empty list says so');
    check(await page.locator('#clear').isDisabled(), 'nothing to clear: the button is off');
    await shot(page, 'downloads-empty.png');
    check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  async function testDownloadsPaging(browser) {
    console.log('downloads page: 2,000 downloads');
    // Today at 13:00 local: a few downloads today, a few yesterday, the rest earlier.
    const noon = new Date(); noon.setHours(13, 0, 0, 0);
    for (const theme of ['light', 'dark']) {
      const { page, errors, external } = await openPage(browser, URL_ + '#perf', { width: 1100, height: 760 }, 1,
        (now) => { window.__host.mockDownloads(2000, now); }, noon.getTime());
      if (theme === 'dark') await page.emulateMedia({ colorScheme: 'dark' });
      await page.waitForSelector('.dl:not(.placeholder)');
      await page.waitForTimeout(150);
      const firstRows = Number(await page.evaluate(() => document.body.dataset.firstRows));
      check(firstRows > 0, theme + ': first rows after ' + firstRows + ' ms (page load to rows)');
      const q0 = await page.evaluate(() => window.__host.dl.queries);
      check(q0 <= 2, theme + ': the first screen takes ' + q0 + ' request(s) of 100 rows, not the whole list');
      const chips = await page.$$eval('#chips .chip', (ns) => ns.map((n) => n.textContent.replace(/\s+/g, ' ').trim()));
      check(chips[0] === 'All 2,000' && /^Failed \d/.test(chips[3]) && chips[1] === 'In progress 0', theme + ': chip counts (' + chips.join(' | ') + ')');
      const groups = await page.$$eval('.dl-group', (ns) => ns.map((n) => n.textContent));
      check(JSON.stringify(groups) === JSON.stringify(['Today', 'Yesterday', 'Earlier']), theme + ': groups Today / Yesterday / Earlier (' + groups.join(', ') + ')');
      const height = await page.evaluate(() => document.getElementById('list').offsetHeight);
      check(height === 2000 * 72 + 3 * 40, theme + ': the list is as tall as all rows (' + height + ')');
      const dom = () => page.evaluate(() => document.querySelectorAll('#list .dl').length);
      const n0 = await dom();
      check(n0 > 5 && n0 <= 40, theme + ': only the rows near the view are in the DOM (' + n0 + ')');
      await shot(page, 'downloads-0150-' + theme + '.png');
      // Scroll through in steps: the DOM stays bounded, the rows follow.
      let most = 0;
      for (let y = 0; y < height; y += 9000) {
        await page.evaluate((y) => window.scrollTo(0, y), y);
        await page.waitForTimeout(30);
        most = Math.max(most, await dom());
      }
      check(most <= 40, theme + ': at most ' + most + ' rows in the DOM while scrolling 2,000');
      await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
      await settle(page);
      const lastIdx = await page.$$eval('.dl:not(.placeholder)', (ns) => Math.max.apply(null, ns.map((n) => Number(n.dataset.index))));
      check(lastIdx === 1999, theme + ': the end of the list is reachable (' + lastIdx + ')');
      check(await page.locator('.dl.placeholder').count() === 0, theme + ': no placeholders left once the pages came');
      const queries = await page.evaluate(() => window.__host.dl.queries);
      check(queries < 40, theme + ': ' + queries + ' page requests for the whole scroll');
      // The middle: the row at index 1000 is the 1001st newest.
      await page.evaluate(() => window.scrollTo(0, 1000 * 72 + 3 * 40 + document.getElementById('list').offsetTop - 200));
      await settle(page);
      const mid = await page.evaluate(() => {
        const n = document.querySelector('.dl[data-index="1000"]');
        const want = window.__host.dl.items.slice().sort((a, b) => b.started - a.started || b.id - a.id)[1000];
        return n && n.dataset.id === String(want.id);
      });
      check(mid, theme + ': row 1000 shows the 1001st newest download');
      await shot(page, 'downloads-0150-scrolled-' + theme + '.png');
      if (theme === 'light') {
        const perf = await page.textContent('#perf');
        check(/first rows \d+ ms/.test(perf) && /rows in DOM/.test(perf), '#perf shows the timing (' + perf + ')');
        // Search (debounced): only matches, back at the top, counted.
        await page.fill('#q', 'ubuntu');
        await page.waitForTimeout(450);
        const names = await page.$$eval('.dl:not(.placeholder) .name', (ns) => ns.map((n) => n.textContent));
        check(names.length > 0 && names.every((n) => n.startsWith('ubuntu')), 'search: only matching rows (' + names.length + ')');
        check(await page.textContent('#summary') === '200 matches' && await page.evaluate(() => window.scrollY) === 0, 'search: "200 matches", from the top');
        await shot(page, 'downloads-0150-search.png');
        // Filters.
        await page.click('.chip[data-filter="failed"]');
        await settle(page);
        const failed = await page.$$eval('.dl:not(.placeholder)', (ns) => ns.every((n) => n.classList.contains('failed')));
        check(failed && await page.getAttribute('.chip[data-filter="failed"]', 'aria-pressed') === 'true', 'filter: failed only');
        const listed = await page.evaluate(() => document.getElementById('list').offsetHeight);
        const nFailed = Number((await page.textContent('.chip[data-filter="failed"] .n')).replace(/\D/g, ''));
        check(nFailed > 0 && nFailed < 200, 'failed matches counted (' + nFailed + ', list ' + listed + ' px)');
        await page.click('.chip[data-filter="active"]');
        await settle(page);
        check(await page.textContent('.empty strong') === 'No matching downloads', 'a filter without matches says so');
        await page.click('.chip[data-filter="done"]');
        await page.fill('#q', '');
        await page.waitForTimeout(450);
        const done = await page.$$eval('.dl:not(.placeholder)', (ns) => ns.every((n) => n.classList.contains('state-completed')));
        check(done, 'filter: done only');
        await page.click('.chip[data-filter="all"]');
        await settle(page);
        // A new running download arrives at the top; its progress moves in place.
        await page.evaluate(() => window.__host.dl.items.push({ id: 5000, url: 'https://new.example/big.iso', fileName: 'big.iso', path: 'C:\\dl\\big.iso', state: 'inProgress', received: 10, total: 1000, started: Date.now() + 1000, reason: null }));
        await emit(page, { event: 'downloadsChanged', active: 1 });
        await settle(page);
        check(await page.locator('.dl[data-id="5000"][data-index="0"] .dl-bar').count() === 1, 'a new download appears at the top with its bar');
        const row = await page.$('.dl[data-id="5000"]');
        await emit(page, { event: 'downloads', items: [{ id: 5000, url: 'https://new.example/big.iso', fileName: 'big.iso', path: 'C:\\dl\\big.iso', state: 'inProgress', received: 500, total: 1000, started: Date.now() + 1000, reason: null, speed: 100 }] });
        await page.waitForTimeout(40);
        check((await page.textContent('.dl[data-id="5000"] .sub')).startsWith('500 B of 1000 B') && await row.evaluate((n) => n.isConnected), 'its progress updates the row in place');
        check(await page.textContent('.chip[data-filter="active"] .n') === '1', 'In progress counts it');
      }
      check(external.length === 0, theme + ': no network requests (' + external.join(', ') + ')');
      check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  return { testDownloads, testDownloadsPaging };
};
