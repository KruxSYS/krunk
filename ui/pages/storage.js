// krunk://storage: cookies grouped by site (registrable domain), per-origin site data
// with sizes, and deleting a cookie, a site's cookies, all cookies or an origin's data.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');
  const search = document.getElementById('search');
  const VALUE_PREVIEW = 48;
  let sites = [];
  const open = new Set();
  const usage = new Map(); // origin -> {usage, breakdown}

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
  function expiry(ms) { return ms === null || ms === undefined ? 'Session' : new Date(ms).toLocaleString(); }
  function sameSite(s) { return { unspecified: '–', none: 'None', lax: 'Lax', strict: 'Strict' }[s] || s; }

  function load() {
    return LB.request('storageList').then(function (r) {
      sites = r.sites;
      render();
      const origins = [];
      sites.forEach(function (s) { s.origins.forEach(function (o) { if (!usage.has(o)) origins.push(o); }); });
      for (let i = 0; i < origins.length; i += 200) {
        LB.request('storageUsage', { origins: origins.slice(i, i + 200) }).then(function (u) {
          u.usage.forEach(function (x) { usage.set(x.origin, x); });
          render();
        });
      }
    }).catch(function (e) { Page.toast(e.message); });
  }

  function siteUsage(s) {
    let total = 0, known = false;
    s.origins.forEach(function (o) { const u = usage.get(o); if (u && u.usage) { total += u.usage; known = true; } });
    return known ? total : null;
  }

  function render() {
    const q = search.value.trim().toLowerCase();
    const shown = sites.filter(function (s) { return !q || s.site.indexOf(q) >= 0 || s.origins.some(function (o) { return o.indexOf(q) >= 0; }); });
    const cookieTotal = sites.reduce(function (n, s) { return n + s.cookies; }, 0);
    document.getElementById('summary').textContent = sites.length + ' sites, ' + cookieTotal + ' cookies' + (q ? ' · ' + shown.length + ' shown' : '');
    list.textContent = '';
    if (!shown.length) {
      list.appendChild(el('div', { class: 'empty', text: q ? 'No site matches.' : 'No cookies or site data.' }));
      return;
    }
    shown.forEach(function (s) { list.appendChild(siteNode(s)); });
  }

  function siteNode(s) {
    const used = siteUsage(s);
    const node = el('div', { class: 'site' + (open.has(s.site) ? ' open' : ''), 'data-site': s.site }, [
      el('div', { class: 'row', tabindex: '0', role: 'button', 'aria-expanded': open.has(s.site) ? 'true' : 'false' }, [
        el('span', { class: 'chev', text: '›' }),
        Page.favicon('https://' + s.site + '/'),
        el('div', { class: 'main' }, [el('span', { class: 'name', text: s.site })]),
        el('span', { class: 'count', text: s.cookies + (s.cookies === 1 ? ' cookie' : ' cookies') + (used ? ' · ' + size(used) : '') }),
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
    head.addEventListener('click', toggle);
    head.addEventListener('keydown', function (e) { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); } });
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

  document.getElementById('delete-all').addEventListener('click', function () {
    Page.confirm('Delete all cookies? You will be signed out of most sites.', { ok: 'Delete all', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('storageDeleteAllCookies').then(function () { Page.toast('All cookies deleted'); load(); })
        .catch(function (e) { Page.toast(e.message); });
    });
  });
  search.addEventListener('input', Page.debounce(render, 100));

  LB.post('ready');
  load();
})();
