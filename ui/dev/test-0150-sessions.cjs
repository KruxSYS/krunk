// Krunk 0.15.0 (WP-E2): krunk://sessions on the mock host: the list (running session marked),
// the detail (stats, bars, CPU chart, timeline), delete, clear all, retention, and the empty
// state with logging off. Hooked into test-ui.cjs (ONLY=SessionsPage,SessionsOff).
'use strict';

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot, acceptInPage } = h;
  const T = new Date(2026, 10, 9, 9, 0, 0).getTime();
  const MIN = 60000;

  function data() {
    const samples = [];
    for (let i = 0; i <= 120; i++) {
      const wave = (Math.sin(i / 9) + 1) * 3;
      samples.push({ ts: T + i * 30000, renderers: 2 + wave, browser: 1.2 + (i % 7) / 10, gpu: 0.8 + wave / 3, other: 0.3, memory: (600 + i) * 1048576 });
    }
    const list = {
      enabled: true, retentionDays: 30, current: 3,
      sessions: [
        { id: 3, started: T + 26 * 60 * MIN, ended: null, durationMs: 42 * MIN, current: true, activeMs: 30 * MIN, visits: 12, sites: 4,
          topSites: [{ host: 'github.com', ms: 18 * MIN, visits: 6 }, { host: 'docs.rs', ms: 9 * MIN, visits: 4 }, { host: 'news.ycombinator.com', ms: 3 * MIN, visits: 2 }] },
        { id: 2, started: T, ended: T + 60 * MIN, durationMs: 60 * MIN, current: false, activeMs: 47 * MIN, visits: 23, sites: 5,
          topSites: [{ host: 'example.org', ms: 25 * MIN, visits: 9 }, { host: 'rust-lang.org', ms: 14 * MIN, visits: 8 }, { host: 'wikipedia.org', ms: 8 * MIN, visits: 6 }] },
        { id: 1, started: T - 24 * 60 * MIN, ended: T - 23 * 60 * MIN + 5 * MIN, durationMs: 65 * MIN, current: false, activeMs: 0, visits: 0, sites: 0, topSites: [] },
      ],
    };
    const detail = {
      session: list.sessions[1],
      visits: [
        { ts: T + 1000, tab: 7, host: 'example.org', url: 'https://example.org/', title: 'Example Domain' },
        { ts: T + 26 * MIN, tab: 8, host: 'rust-lang.org', url: 'https://www.rust-lang.org/learn', title: 'Learn Rust' },
        { ts: T + 41 * MIN, tab: 9, host: 'wikipedia.org', url: 'https://en.wikipedia.org/wiki/Rust', title: 'Rust (programming language) – Wikipedia' },
      ],
      visitsTotal: 23,
      focus: [
        { tab: 7, host: 'example.org', started: T + 1000, ended: T + 25 * MIN },
        { tab: 8, host: 'rust-lang.org', started: T + 26 * MIN, ended: T + 40 * MIN },
        { tab: 9, host: 'wikipedia.org', started: T + 41 * MIN, ended: T + 49 * MIN },
      ],
      sites: list.sessions[1].topSites.concat([{ host: 'docs.rs', ms: 0, visits: 1 }]),
      tabs: [
        { tab: 7, title: 'Example Domain', host: 'example.org', ms: 25 * MIN },
        { tab: 8, title: 'Learn Rust', host: 'rust-lang.org', ms: 14 * MIN },
        { tab: 9, title: 'Rust (programming language) – Wikipedia', host: 'wikipedia.org', ms: 8 * MIN },
      ],
      samples: samples,
      cpu: { avgTotal: 7.4, peakTotal: 12.9, avgGpu: 1.8, peakMemory: 720 * 1048576 },
    };
    return { list, detail };
  }

  // The host: list and detail, delete, clear, settings changes.
  function mock(arg) {
    const st = JSON.parse(JSON.stringify(arg));
    window.__sess = st;
    const hd = window.__host.handlers;
    hd.sessionList = () => st.list;
    hd.sessionDetail = (m) => {
      if (m.id === st.detail.session.id) return st.detail;
      const s = st.list.sessions.find((x) => x.id === m.id);
      if (!s) throw new Error('This session is gone');
      return { session: s, visits: [], visitsTotal: 0, focus: [], sites: [], tabs: [], samples: [], cpu: { avgTotal: 0, peakTotal: 0, avgGpu: 0, peakMemory: 0 } };
    };
    hd.sessionDelete = (m) => {
      if (m.id === st.list.current) throw new Error("The running session can't be deleted (Clear all empties it)");
      st.list.sessions = st.list.sessions.filter((s) => s.id !== m.id);
      return { deleted: true };
    };
    hd.sessionClear = () => { st.list.sessions = st.list.sessions.filter((s) => s.current); return { deleted: 2 }; };
    hd.sessionLogSet = (m) => {
      if (m.enabled !== undefined) st.list.enabled = m.enabled;
      if (m.retentionDays !== undefined) st.list.retentionDays = m.retentionDays;
      return st.list;
    };
  }

  async function testSessionsPage(browser) {
    console.log('session logs page');
    for (const theme of ['light', 'dark']) {
      const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/sessions.html', { width: 1100, height: 900 }, 1, mock, data());
      await emit(page, { event: 'theme', theme: theme });
      await page.waitForSelector('#list .session');
      check(await page.locator('#list .session').count() === 3, theme + ': lists the 3 sessions');
      const first = page.locator('#list .session').first();
      check((await first.textContent()).includes('Running') && (await first.getAttribute('data-id')) === '3', theme + ': the running session is first and marked');
      check(await first.locator('.actions button').count() === 0, theme + ': the running session has no delete button');
      const second = page.locator('#list .session[data-id="2"]');
      const t2 = await second.textContent();
      check(t2.includes('1 h') && t2.includes('example.org') && t2.includes('25 min') && t2.includes('23 pages'), theme + ': a row shows duration, top sites with time, pages');
      check((await page.textContent('#list .session[data-id="1"]')).includes('No sites'), theme + ': a session without sites says so');
      check(await page.locator('#off').isHidden(), theme + ': no "logging is off" notice while on');
      check(await page.inputValue('#retention') === '30', theme + ': the retention select shows the setting');
      await shot(page, 'sessions-list-' + theme + '.png');

      // Detail.
      await second.click();
      await page.waitForSelector('#detail-view:not([hidden]) #chart svg path.area');
      check(await page.locator('#list-view').isHidden(), theme + ': the detail replaces the list');
      check(await page.locator('#chart svg path.area').count() === 4, theme + ': the CPU chart stacks 4 series');
      check(await page.locator('#legend .key').count() === 4, theme + ': with a legend');
      check(await page.locator('#sites .bar-row').count() === 3, theme + ': time per site: sites with time only');
      check((await page.textContent('#sites')).includes('example.org') && (await page.textContent('#tabs')).includes('Learn Rust'), theme + ': sites and tabs are named');
      check(await page.locator('#timeline li').count() === 6, theme + ': the timeline has visits and time in front');
      check((await page.textContent('#timeline-more')).includes('20 later pages'), theme + ': says how many pages are not listed');
      check((await page.textContent('#stats')).includes('12.9 %') && (await page.textContent('#stats')).includes('720 MB'), theme + ': stats show the CPU peak and memory');
      const box = await page.locator('#chart svg').boundingBox();
      await page.mouse.move(box.x + box.width * 0.6, box.y + 100);
      await page.waitForTimeout(50);
      check(await page.locator('#chart .tip').isVisible() && (await page.textContent('#chart .tip')).includes('Tabs'), theme + ': hovering the chart shows the values');
      await shot(page, 'sessions-detail-' + theme + '.png');
      await page.mouse.move(5, 5);

      if (theme === 'light') {
        // Delete from the detail, back to the list.
        await page.click('#delete');
        await acceptInPage(page);
        await page.waitForTimeout(80);
        check((await lastSent(page, 'sessionDelete')).id === 2, 'Delete sends the session id');
        check(await page.locator('#list-view').isVisible() && await page.locator('#list .session').count() === 2, 'back on the list without it');
        // Retention.
        await page.selectOption('#retention', '90');
        await page.waitForTimeout(60);
        check((await lastSent(page, 'sessionLogSet')).retentionDays === 90, 'the retention select changes the setting');
        // Clear all (confirmed).
        await page.click('#clear-all');
        await acceptInPage(page);
        await page.waitForTimeout(80);
        check((await sent(page)).some((m) => m.cmd === 'sessionClear'), 'Clear all asks first, then clears');
        check(await page.locator('#list .session').count() === 1, 'only the running session is left');
        // A session that is gone: back to the list.
        await page.evaluate(() => { location.hash = 's99'; });
        await page.waitForTimeout(80);
        check(await page.locator('#list-view').isVisible(), 'a deleted session id goes back to the list');
      }
      check(errors.length === 0, theme + ': no page errors ' + errors.join('; '));
      check(external.length === 0, theme + ': no external requests');
      await page.context().close();
    }
  }

  async function testSessionsOff(browser) {
    console.log('session logs page, logging off');
    for (const theme of ['light', 'dark']) {
      const d = data();
      d.list = { enabled: false, retentionDays: 30, current: null, sessions: [] };
      const { page, errors } = await openPage(browser, 'https://krunk.invalid/sessions.html', { width: 1000, height: 600 }, 1, mock, d);
      await emit(page, { event: 'theme', theme: theme });
      await page.waitForSelector('#off:not([hidden])');
      check((await page.textContent('#off')).includes('Session logging is off') && (await page.textContent('#list')).includes('Turn logging on'), theme + ': the empty state explains the toggle');
      check(await page.locator('#clear-all').isDisabled(), theme + ': nothing to clear');
      await shot(page, 'sessions-off-' + theme + '.png');
      if (theme === 'light') {
        await page.click('#open-settings');
        const open = await lastSent(page, 'openPage');
        check(open && open.page === 'settings' && open.section === 'privacy', 'the link opens Settings → Privacy');
        await page.click('#turn-on');
        await page.waitForTimeout(80);
        check((await lastSent(page, 'sessionLogSet')).enabled === true && await page.locator('#off').isHidden(), 'Turn on turns logging on');
      }
      check(errors.length === 0, theme + ': no page errors ' + errors.join('; '));
      await page.context().close();
    }
  }

  return { testSessionsPage, testSessionsOff };
};
