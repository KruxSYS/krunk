// krunk://adblock: what the ad blocker blocked on the tab this page was opened from (its
// shield panel's "Show all"), with the filter that matched each address; the filter lists
// (on/off, add by address, remove, update) and "My filters".
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');
  const summary = document.getElementById('summary');
  const search = document.getElementById('search');
  let data = null;
  // Short names for request types.
  const KIND_LABELS = { xmlhttprequest: 'xhr', sub_frame: 'frame', main_frame: 'page', stylesheet: 'css', websocket: 'socket', image: 'image', script: 'script', ping: 'ping', media: 'media', font: 'font', object: 'object', other: 'other', document: 'page' };

  function render() {
    list.textContent = '';
    if (!data) return;
    if (!data.enabled) {
      summary.textContent = 'Ad blocking is turned off (Settings → Privacy).';
    } else if (!data.source) {
      summary.textContent = 'Open this page from the shield in the toolbar to see what was blocked on a page.';
    } else {
      const total = data.items.reduce(function (n, i) { return n + i.count; }, 0);
      summary.textContent = total + ' requests to ' + data.items.length + ' addresses blocked on ' + (data.host || 'this page');
    }
    const q = search.value.trim().toLowerCase();
    const items = data.items.filter(function (i) {
      return !q || i.url.toLowerCase().includes(q) || (i.rule || '').toLowerCase().includes(q) || (i.list || '').toLowerCase().includes(q);
    });
    if (!items.length) {
      list.appendChild(el('div', { class: 'empty', text: data.items.length ? 'Nothing matches.' : 'Nothing was blocked on this page.' }));
      return;
    }
    // By host, hosts in order of their newest request.
    const hosts = [];
    const byHost = new Map();
    items.forEach(function (i) {
      if (!byHost.has(i.host)) { byHost.set(i.host, []); hosts.push(i.host); }
      byHost.get(i.host).push(i);
    });
    hosts.forEach(function (h) {
      const reqs = byHost.get(h);
      const n = reqs.reduce(function (s, i) { return s + i.count; }, 0);
      list.appendChild(el('div', { class: 'host' }, [
        el('h3', {}, [el('span', { text: h || '(no host)' }), el('span', { class: 'n', text: n + (n === 1 ? ' request' : ' requests') })]),
      ].concat(reqs.map(function (i) {
        const path = i.url.replace(/^https?:\/\/[^/]+/, '') || '/';
        return el('div', { class: 'req' }, [
          el('span', { class: 'kind', title: i.kind, text: KIND_LABELS[i.kind] || i.kind }),
          el('span', { class: 'addr', title: i.url, text: path }),
          el('span', { class: 'count', text: i.count > 1 ? '×' + i.count : '' }),
          el('div', { class: 'why' }, i.rule ? [
            el('span', { class: 'rule', text: i.rule }),
            el('span', { text: ' · ' + (i.list || 'filter list') }),
            i.redirected ? el('span', { class: 'tag', title: 'Answered with a harmless stand-in instead of being cancelled', text: 'stand-in' }) : null,
          ] : [el('span', { text: 'filter list' })]),
        ]);
      }))));
    });
  }

  function load() {
    LB.request('getBlocked').then(function (d) { data = d; render(); }, function (e) { Page.toast(e.message); });
  }
  search.addEventListener('input', render);
  document.getElementById('refresh').addEventListener('click', load);

  // ---------- tabs ----------
  const $ = function (id) { return document.getElementById(id); };
  let current = 'blocked';
  function showPanel(name) {
    if (!$(name) || !$(name).classList.contains('panel')) name = 'blocked';
    current = name;
    document.querySelectorAll('.tab-btn').forEach(function (b) { b.setAttribute('aria-selected', String(b.dataset.panel === name)); });
    document.querySelectorAll('.panel').forEach(function (p) { p.hidden = p.id !== name; });
    if (location.hash.slice(1) !== name) history.replaceState(null, '', name === 'blocked' ? location.pathname : '#' + name);
    refresh();
  }
  function refresh() {
    if (current === 'blocked') load();
    else if (current === 'lists') loadLists();
    else if (current === 'filters' && !userDirty()) loadUser();
  }
  document.querySelectorAll('.tab-btn').forEach(function (b) {
    b.addEventListener('click', function () { showPanel(b.dataset.panel); });
  });

  // ---------- filter lists ----------
  let lists = null;
  let working = false;
  function fmtTime(ms) { return new Date(ms).toLocaleString(); }
  // `working`: a change of this page waits for its reply; the controls also wait while the
  // host rebuilds for another reason (lists.busy).
  function setWorking(on, text) {
    working = on;
    disable(on || !!(lists && lists.busy));
    if (on) $('lists-status').textContent = text || 'Applying…';
  }
  function disable(on) {
    ['update-lists', 'restore-lists', 'add-btn', 'add-url'].forEach(function (id) { $(id).disabled = on; });
    $('list-rows').querySelectorAll('input, button').forEach(function (c) { c.disabled = on; });
  }
  function renderLists() {
    const box = $('list-rows');
    box.textContent = '';
    if (!lists) return;
    $('lists-off').hidden = lists.enabled;
    let status = lists.lastCheck ? 'Last checked ' + fmtTime(lists.lastCheck) : 'Not updated yet (using the built-in copies)';
    if (lists.lastError) status += ' · last problem: ' + lists.lastError;
    if (lists.busy) status = 'Updating…';
    $('lists-status').textContent = status;
    const on = lists.lists.filter(function (l) { return l.enabled; }).length;
    $('tab-lists').textContent = 'Filter lists (' + on + ')';
    lists.lists.forEach(function (l) {
      const cb = el('input', { type: 'checkbox', 'aria-label': l.title, 'data-id': l.id });
      cb.checked = l.enabled;
      cb.addEventListener('change', function () {
        change('setFilterList', { id: l.id, enabled: cb.checked }, (cb.checked ? 'Turning on ' : 'Turning off ') + l.title + '…');
      });
      let meta;
      if (l.source === 'downloaded') meta = 'updated ' + fmtTime(l.fetched);
      else if (l.source === 'embedded') meta = 'built-in copy (from this version of Krunk)';
      else meta = 'not downloaded yet';
      if (l.rules != null) meta = l.rules.toLocaleString() + (l.rules === 1 ? ' filter · ' : ' filters · ') + meta;
      const row = el('div', { class: 'list-row' + (l.enabled ? '' : ' off'), 'data-id': l.id }, [
        cb,
        el('div', { class: 'title' }, [el('span', { text: l.title }), el('span', { class: 'tag', text: l.builtin ? 'built-in' : 'added' })]),
        l.builtin ? null : Page.iconButton('trash', 'Remove ' + l.title, function () {
          Page.confirm('Remove the list “' + l.title + '”?', { ok: 'Remove', danger: true }).then(function (yes) {
            if (yes) change('removeFilterList', { id: l.id }, 'Removing ' + l.title + '…');
          });
        }),
        el('div', { class: 'url', text: l.url }),
        el('div', { class: 'meta', text: meta }),
        l.error ? el('div', { class: 'err', text: 'Last download failed: ' + l.error }) : null,
      ]);
      box.appendChild(row);
    });
    disable(working || lists.busy);
    // A rebuild runs (the daily update, a change elsewhere): look again until it is done.
    clearTimeout(renderLists.poll);
    if (lists.busy) renderLists.poll = setTimeout(function () { if (current === 'lists') loadLists(); }, 1000);
  }
  function loadLists() {
    return LB.request('getFilterLists').then(function (d) { lists = d; renderLists(); }, function (e) { Page.toast(e.message); });
  }
  // A change: the reply comes once the new filters are in use.
  function change(cmd, args, text) {
    setWorking(true, text);
    return LB.request(cmd, args).then(function () {
      setWorking(false);
      return loadLists();
    }, function (e) {
      setWorking(false);
      loadLists();
      throw e;
    });
  }
  $('update-lists').addEventListener('click', function () {
    change('updateFilterLists', undefined, 'Updating…').catch(function (e) { Page.toast(e.message); });
  });
  $('restore-lists').addEventListener('click', function () {
    change('restoreDefaultLists', undefined).then(function () { Page.toast('Default lists restored'); },
      function (e) { Page.toast(e.message); });
  });
  $('add-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const url = $('add-url').value.trim();
    $('add-error').textContent = '';
    if (!/^https:\/\/[^\s/]+/i.test(url)) { $('add-error').textContent = 'Enter an https:// address'; return; }
    change('addFilterList', { url: url }, 'Downloading ' + url + '…').then(function () {
      $('add-url').value = '';
      Page.toast('List added');
    }, function (err) { $('add-error').textContent = err.message; });
  });

  // ---------- my filters ----------
  let saved = '';
  function userDirty() { return $('user-text').value !== saved; }
  function renderReport(r, note) {
    const parts = [];
    parts.push(r.network + (r.network === 1 ? ' blocking filter' : ' blocking filters'));
    parts.push(r.cosmetic + (r.cosmetic === 1 ? ' element filter' : ' element filters'));
    if (r.errors.length) parts.push(r.errors.length + (r.errors.length === 1 ? ' line not understood' : ' lines not understood'));
    $('user-status').textContent = (note ? note + ' · ' : '') + parts.join(' · ');
    const box = $('user-errors');
    box.textContent = '';
    r.errors.forEach(function (e) {
      box.appendChild(el('div', { class: 'line-err' }, [
        el('strong', { text: 'Line ' + e.line + ': ' }),
        el('span', { text: e.message + ' — ' }),
        el('code', { text: e.text }),
      ]));
    });
  }
  function loadUser() {
    return LB.request('getUserFilters').then(function (d) {
      saved = d.text;
      $('user-text').value = d.text;
      renderReport(d.report);
      $('user-save').disabled = true;
      $('user-revert').disabled = true;
    }, function (e) { Page.toast(e.message); });
  }
  $('user-text').addEventListener('input', function () {
    $('user-save').disabled = !userDirty();
    $('user-revert').disabled = !userDirty();
  });
  $('user-revert').addEventListener('click', loadUser);
  $('user-save').addEventListener('click', function () {
    const text = $('user-text').value;
    $('user-save').disabled = true;
    $('user-status').textContent = 'Saving…';
    LB.request('saveUserFilters', { text: text }).then(function (r) {
      saved = text;
      $('user-revert').disabled = true;
      $('user-save').disabled = userDirty();
      renderReport(r, 'Saved and in use');
    }, function (e) {
      $('user-save').disabled = false;
      $('user-status').textContent = e.message;
    });
  });
  // Ctrl+S saves.
  $('user-text').addEventListener('keydown', function (e) {
    if (e.key === 's' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      if (!$('user-save').disabled) $('user-save').click();
    }
  });

  document.addEventListener('visibilitychange', function () { if (!document.hidden) refresh(); });
  window.addEventListener('hashchange', function () { showPanel(location.hash.slice(1) || 'blocked'); });
  LB.post('ready');
  showPanel(location.hash.slice(1) || 'blocked');
  // The tab title counts the lists that are on.
  if (current !== 'lists') loadLists();
})();
