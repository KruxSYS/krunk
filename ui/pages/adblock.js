// krunk://adblock: what the ad blocker blocked on the tab this page was opened from (its
// shield panel's "Show all"), with the filter that matched each address.
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
  document.addEventListener('visibilitychange', function () { if (!document.hidden) load(); });
  LB.post('ready');
  load();
})();
