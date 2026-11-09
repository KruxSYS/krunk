// krunk://storage: cookies grouped by site (registrable domain), per-origin site data
// with sizes, and deleting a cookie, a site's cookies, all cookies or an origin's data.
// 0.6.0: sort by name, usage or last used; a usage breakdown and the last use per site;
// select sites and clear them, or clear everything shown; the totals at the top.
// 0.11.0: thousands of sites stay smooth: the list is shown a page at a time (more as it
// scrolls), sizes are measured for the rows in view first and patched into them (one update per
// frame, never a rebuild of an open row), and each site says how many subdomains it groups.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');
  const search = document.getElementById('search');
  const sortSel = document.getElementById('sort');
  const VALUE_PREVIEW = 48;
  let sites = [];
  const open = new Set();
  const selected = new Set();
  const usage = new Map(); // origin -> {usage, breakdown}
  // Rows are added a page at a time; the rows shown so far, by site.
  const PAGE = 150;
  let shownCount = PAGE;
  let shownList = [];
  const rowNodes = new Map();
  let measured = 0, toMeasure = 0;

  function size(n) {
    if (n === null || n === undefined) return '';
    const u = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i ? n.toFixed(1) : Math.round(n)) + ' ' + u[i];
  }
  const KIND = { local_storage: 'local storage', indexeddb: 'IndexedDB', cache_storage: 'Cache Storage',
    service_workers: 'service workers', file_systems: 'file systems', websql: 'Web SQL', shared_storage: 'shared storage',
    storage_buckets: 'storage buckets', interest_groups: 'interest groups', shader_cache: 'shader cache' };
  // The row's short breakdown: the engine's storage types folded into three groups.
  const GROUP = { local_storage: 'local', indexeddb: 'IndexedDB', cache_storage: 'cache', service_workers: 'cache' };
  function expiry(ms) { return ms === null || ms === undefined ? 'Session' : new Date(ms).toLocaleString(); }
  function sameSite(s) { return { unspecified: '–', none: 'None', lax: 'Lax', strict: 'Strict' }[s] || s; }
  function ago(ms) {
    if (!ms) return 'never used';
    const d = Date.now() - ms;
    if (d < 60000) return 'just now';
    if (d < 3600000) return Math.round(d / 60000) + ' min ago';
    if (d < 86400000) return Math.round(d / 3600000) + ' h ago';
    if (d < 172800000) return 'yesterday';
    if (d < 30 * 86400000) return Math.round(d / 86400000) + ' days ago';
    return new Date(ms).toLocaleDateString();
  }

  let loadSeq = 0;
  function load() {
    const seq = ++loadSeq;
    return LB.request('storageList').then(function (r) {
      sites = r.sites;
      render();
      measure(seq);
    }).catch(function (e) { Page.toast(e.message); });
  }
  // Sizes, 200 origins per request, one request at a time: the sites on screen first.
  function measure(seq) {
    const seen = new Set();
    const origins = [];
    const add = function (s) { s.origins.forEach(function (o) { if (!usage.has(o) && !seen.has(o)) { seen.add(o); origins.push(o); } }); };
    shownList.slice(0, shownCount).forEach(add);
    sites.forEach(add);
    measured = 0;
    toMeasure = origins.length;
    let i = 0;
    const next = function () {
      if (seq !== loadSeq || i >= origins.length) { toMeasure = 0; scheduleUpdate(); return; }
      const batch = origins.slice(i, i + 200);
      i += batch.length;
      LB.request('storageUsage', { origins: batch }).then(function (u) {
        u.usage.forEach(function (x) { usage.set(x.origin, x); });
        measured += batch.length;
        scheduleUpdate();
        next();
      }).catch(function () { toMeasure = 0; scheduleUpdate(); });
    };
    next();
  }
  // New sizes: the rows shown and the totals follow, at most once a frame. Sorted by size, the
  // order follows too (at most once a second while measuring).
  let updatePending = false, lastResort = 0;
  function scheduleUpdate() {
    if (updatePending) return;
    updatePending = true;
    requestAnimationFrame(function () {
      updatePending = false;
      if (sortSel.value === 'usage' && (toMeasure === 0 || Date.now() - lastResort > 1000)) { lastResort = Date.now(); render(true); return; }
      rowNodes.forEach(function (node, site) {
        const s = sites.find(function (x) { return x.site === site; });
        if (s) node.querySelector('.count').textContent = countText(s);
      });
      summary();
    });
  }

  function siteUsage(s) {
    let total = 0, known = false;
    s.origins.forEach(function (o) { const u = usage.get(o); if (u && u.usage) { total += u.usage; known = true; } });
    return known ? total : null;
  }
  // "local 2.0 KB · IndexedDB 50 KB · cache 3 KB" over the site's origins.
  function siteBreakdown(s) {
    const groups = {};
    s.origins.forEach(function (o) {
      const u = usage.get(o);
      if (!u) return;
      u.breakdown.forEach(function (b) { const g = GROUP[b[0]] || 'other'; groups[g] = (groups[g] || 0) + b[1]; });
    });
    return ['local', 'IndexedDB', 'cache', 'other'].filter(function (g) { return groups[g]; }).map(function (g) { return g + ' ' + size(groups[g]); });
  }

  function shownSites() {
    const q = search.value.trim().toLowerCase();
    const shown = sites.filter(function (s) { return !q || s.site.indexOf(q) >= 0 || s.origins.some(function (o) { return o.indexOf(q) >= 0; }); });
    const by = sortSel.value;
    if (by === 'usage') {
      shown.sort(function (a, b) { return (siteUsage(b) || 0) - (siteUsage(a) || 0) || b.cookies - a.cookies || a.site.localeCompare(b.site); });
    } else if (by === 'cookies') {
      shown.sort(function (a, b) { return b.cookies - a.cookies || a.site.localeCompare(b.site); });
    } else if (by === 'lastUsed') {
      shown.sort(function (a, b) { return (b.lastUsed || 0) - (a.lastUsed || 0) || a.site.localeCompare(b.site); });
    } else {
      shown.sort(function (a, b) { return a.site.localeCompare(b.site); });
    }
    return shown;
  }

  // The selection buttons (without re-rendering the list: a checkbox keeps its focus).
  function updateHeader(shown) {
    const selBtn = document.getElementById('clear-selected');
    selBtn.hidden = !selected.size;
    selBtn.textContent = 'Clear selected (' + selected.size + ')';
    const n = (shown || shownSites()).length;
    const all = document.getElementById('clear-shown');
    all.disabled = !n;
    all.textContent = search.value.trim() ? 'Clear all shown (' + n + ')' : 'Clear all shown';
  }

  function summary() {
    const q = search.value.trim();
    const cookieTotal = sites.reduce(function (n, s) { return n + s.cookies; }, 0);
    let bytes = 0;
    sites.forEach(function (s) { bytes += siteUsage(s) || 0; });
    document.getElementById('summary').textContent = sites.length + ' sites, ' + cookieTotal + ' cookies' +
      (bytes ? ', ' + size(bytes) + ' of site data' : '') + (q ? ' · ' + shownList.length + ' shown' : '') +
      (toMeasure ? ' · measuring sizes… ' + Math.min(measured, toMeasure) + ' of ' + toMeasure : '');
  }

  // `keep`: the same list again (new sizes, sorted by size): as many rows as before.
  function render(keep) {
    const q = search.value.trim().toLowerCase();
    shownList = shownSites();
    if (!keep) shownCount = PAGE;
    summary();
    updateHeader(shownList);
    list.textContent = '';
    rowNodes.clear();
    if (!shownList.length) {
      list.appendChild(el('div', { class: 'empty', text: q ? 'No site matches.' : 'No cookies or site data.' }));
      return;
    }
    appendRows(0, Math.min(shownCount, shownList.length));
  }
  // Rows `from`..`to` of the shown list, and the marker that brings the next page into view.
  const moreObserver = 'IntersectionObserver' in window ? new IntersectionObserver(function (entries) {
    if (entries.some(function (e) { return e.isIntersecting; })) showMore();
  }, { rootMargin: '600px' }) : null;
  function appendRows(from, to) {
    const old = list.querySelector('.more');
    if (old) { if (moreObserver) moreObserver.unobserve(old); old.remove(); }
    const frag = document.createDocumentFragment();
    for (let i = from; i < to; i++) {
      const node = siteNode(shownList[i]);
      rowNodes.set(shownList[i].site, node);
      frag.appendChild(node);
    }
    list.appendChild(frag);
    if (to < shownList.length) {
      const more = el('button', { class: 'more link-btn', type: 'button', text: 'Show more (' + (shownList.length - to) + ' more sites)', onclick: showMore });
      list.appendChild(more);
      if (moreObserver) moreObserver.observe(more);
    }
  }
  function showMore() {
    const from = Math.min(shownCount, shownList.length);
    if (from >= shownList.length) return;
    shownCount += PAGE;
    appendRows(from, Math.min(shownCount, shownList.length));
  }

  // "2 cookies · 3 subdomains · local 2.0 KB · 50.8 KB"
  function subdomains(s) {
    const hosts = new Set();
    s.origins.forEach(function (o) { try { const h = new URL(o).hostname; if (h !== s.site) hosts.add(h); } catch (e) { /* not a URL */ } });
    return hosts.size;
  }
  function countText(s) {
    const used = siteUsage(s);
    const sub = subdomains(s);
    const parts = [s.cookies + (s.cookies === 1 ? ' cookie' : ' cookies')]
      .concat(sub ? [sub + (sub === 1 ? ' subdomain' : ' subdomains')] : [])
      .concat(siteBreakdown(s));
    return parts.join(' · ') + (used ? ' · ' + size(used) : '');
  }

  function siteNode(s) {
    const check = el('input', { type: 'checkbox', class: 'pick', 'aria-label': 'Select ' + s.site });
    check.checked = selected.has(s.site);
    check.addEventListener('click', function (e) { e.stopPropagation(); });
    check.addEventListener('change', function () { if (check.checked) selected.add(s.site); else selected.delete(s.site); updateHeader(); });
    const node = el('div', { class: 'site' + (open.has(s.site) ? ' open' : ''), 'data-site': s.site }, [
      el('div', { class: 'row', tabindex: '0', role: 'button', 'aria-expanded': open.has(s.site) ? 'true' : 'false' }, [
        check,
        el('span', { class: 'chev', text: '›' }),
        Page.favicon('https://' + s.site + '/'),
        el('div', { class: 'main' }, [el('span', { class: 'name', text: s.site })]),
        el('span', { class: 'count', text: countText(s) }),
        el('span', { class: 'last', title: s.lastUsed ? new Date(s.lastUsed).toLocaleString() : '', text: ago(s.lastUsed) }),
        Page.iconButton('trash', 'Clear ' + s.site, function (e) { e.stopPropagation(); clearSites([s]); }),
      ]),
      el('div', { class: 'details' }),
    ]);
    const head = node.firstChild;
    const toggle = function () {
      if (open.has(s.site)) open.delete(s.site); else open.add(s.site);
      node.classList.toggle('open');
      head.setAttribute('aria-expanded', open.has(s.site) ? 'true' : 'false');
      if (open.has(s.site)) fillDetails(s, node.lastChild);
    };
    head.addEventListener('click', function (e) { if (!e.target.closest('.icon-btn, .pick')) toggle(); });
    // Only the row itself: Space on its checkbox and Enter on its button do their own thing.
    head.addEventListener('keydown', function (e) { if (e.target === head && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); toggle(); } });
    if (open.has(s.site)) fillDetails(s, node.lastChild);
    return node;
  }

  function fillDetails(s, box) {
    box.textContent = '';
    if (s.cookies) {
      box.appendChild(el('h3', { text: 'Cookies' }));
      const table = el('table', { class: 'cookies' }, [
        el('thead', {}, [el('tr', {}, ['Name', 'Value', 'Domain', 'Path', 'Expires', 'Flags', ''].map(function (h) { return el('th', { text: h }); }))]),
        el('tbody', {}, [el('tr', {}, [el('td', { colspan: '7', class: 'muted', text: 'Loading…' })])]),
      ]);
      box.appendChild(table);
      LB.request('storageCookies', { site: s.site }).then(function (r) {
        const body = table.lastChild;
        body.textContent = '';
        r.cookies.forEach(function (c) { body.appendChild(cookieRow(c, s)); });
      }).catch(function (e) { Page.toast(e.message); });
    }
    if (s.origins.length) {
      box.appendChild(el('h3', { text: 'Site data' }));
      s.origins.forEach(function (o) {
        const u = usage.get(o);
        let text = u ? (u.usage === null ? 'size unknown' : size(u.usage)) : 'measuring…';
        if (u && u.breakdown.length) text += ' (' + u.breakdown.map(function (b) { return (KIND[b[0]] || b[0]) + ' ' + size(b[1]); }).join(', ') + ')';
        box.appendChild(el('div', { class: 'origin' }, [
          el('span', { class: 'o-name', text: o }),
          el('span', { class: 'o-usage', text: text }),
          el('button', { class: 'link-btn danger', text: 'Clear site data', onclick: function () { clearOrigin(o); } }),
        ]));
      });
    }
    box.appendChild(el('div', { class: 'site-actions' }, [
      el('button', { class: 'link-btn danger clear-site', text: 'Clear ' + s.site + ' (cookies, data and permissions)', onclick: function () { clearSites([s]); } }),
      s.cookies ? el('button', { class: 'link-btn danger', text: 'Delete all cookies of ' + s.site, onclick: function () { deleteSite(s.site); } }) : null,
    ]));
  }

  function cookieRow(c, s) {
    const long = c.value.length > VALUE_PREVIEW;
    const valueCell = el('td', { class: 'value', title: long ? '' : c.value });
    const showValue = function (full) {
      valueCell.textContent = full || !long ? c.value : c.value.slice(0, VALUE_PREVIEW) + '…';
      if (long) {
        valueCell.appendChild(el('button', { class: 'show', text: full ? 'Hide' : 'Show', onclick: function () { showValue(!full); } }));
      }
    };
    showValue(false);
    const flags = [c.secure ? 'Secure' : null, c.httpOnly ? 'HttpOnly' : null, 'SameSite ' + sameSite(c.sameSite)].filter(Boolean).join(' · ');
    return el('tr', { 'data-name': c.name }, [
      el('td', { text: c.name, title: c.name }),
      valueCell,
      el('td', { text: c.domain, title: c.domain }),
      el('td', { text: c.path, title: c.path }),
      el('td', { text: expiry(c.expires) }),
      el('td', { class: 'flags', text: flags }),
      el('td', {}, [Page.iconButton('trash', 'Delete cookie ' + c.name, function () {
        LB.request('storageDeleteCookie', { cookie: { name: c.name, domain: c.domain, path: c.path } })
          .then(function () { Page.toast('Cookie deleted'); return load(); })
          .catch(function (e) { Page.toast(e.message); });
      })]),
    ]);
  }

  function deleteSite(site) {
    Page.confirm('Delete all cookies of ' + site + '? You may be signed out there.', { ok: 'Delete', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('storageDeleteSite', { site: site }).then(function () { Page.toast('Cookies deleted'); load(); })
        .catch(function (e) { Page.toast(e.message); });
    });
  }

  function clearOrigin(origin) {
    Page.confirm('Clear all data stored by ' + origin + ' (local storage, IndexedDB, caches, service workers, cookies)?', { ok: 'Clear', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('storageClearOrigin', { origin: origin }).then(function () { usage.delete(origin); Page.toast('Site data cleared'); load(); })
        .catch(function (e) { Page.toast(e.message); });
    });
  }

  // Clears whole sites: their cookies, then every origin's storage (which also forgets the
  // permission answers). Sequential, so the engine sees one request at a time.
  function clearSites(which) {
    if (!which.length) return;
    const what = which.length === 1 ? which[0].site : which.length + ' sites';
    Page.confirm('Clear all cookies, site data and permission answers of ' + what + '? You will be signed out there.', { ok: 'Clear', danger: true }).then(function (yes) {
      if (!yes) return;
      let chain = Promise.resolve();
      which.forEach(function (s) {
        if (s.cookies) chain = chain.then(function () { return LB.request('storageDeleteSite', { site: s.site }); });
        s.origins.forEach(function (o) { chain = chain.then(function () { return LB.request('storageClearOrigin', { origin: o }).then(function () { usage.delete(o); }); }); });
        chain = chain.then(function () { selected.delete(s.site); open.delete(s.site); });
      });
      chain.then(function () { Page.toast(which.length === 1 ? 'Site cleared' : which.length + ' sites cleared'); load(); })
        .catch(function (e) { Page.toast(e.message); load(); });
    });
  }
  document.getElementById('clear-selected').addEventListener('click', function () {
    clearSites(sites.filter(function (s) { return selected.has(s.site); }));
  });
  document.getElementById('clear-shown').addEventListener('click', function () { clearSites(shownSites()); });
  sortSel.addEventListener('change', function () { render(); });

  document.getElementById('delete-all').addEventListener('click', function () {
    Page.confirm('Delete all cookies? You will be signed out of most sites.', { ok: 'Delete all', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('storageDeleteAllCookies').then(function () { Page.toast('All cookies deleted'); load(); })
        .catch(function (e) { Page.toast(e.message); });
    });
  });
  search.addEventListener('input', Page.debounce(function () { render(); }, 100));

  LB.post('ready');
  load();
})();
