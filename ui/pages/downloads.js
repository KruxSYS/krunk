// Downloads page: live list with progress and actions.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');

  function size(n) {
    if (n === null || n === undefined) return '';
    const u = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i ? n.toFixed(1) : n) + ' ' + u[i];
  }

  function act(id, action) { LB.request('downloadAction', { id: id, action: action }).catch(function (e) { Page.toast(e.message); }); }

  function render(items) {
    list.textContent = '';
    if (!items.length) {
      list.appendChild(el('div', { class: 'empty', text: 'Files you download will appear here.' }));
      return;
    }
    items.forEach(function (d) {
      const running = d.state === 'inProgress' || d.state === 'paused';
      const failed = d.state === 'interrupted' || d.state === 'cancelled';
      const ext = (d.fileName.split('.').pop() || '').slice(0, 4).toUpperCase();
      let status;
      if (d.state === 'completed') status = size(d.total || d.received);
      else if (d.state === 'cancelled') status = 'Cancelled';
      else if (d.state === 'interrupted') status = 'Failed' + (d.reason ? ' – ' + d.reason : '');
      else status = size(d.received) + (d.total ? ' of ' + size(d.total) : '') + (d.state === 'paused' ? ' – paused' : '');
      const links = [];
      if (d.state === 'completed') {
        links.push(el('button', { text: 'Open file', onclick: function () { act(d.id, 'open'); } }));
        links.push(el('button', { text: 'Show in folder', onclick: function () { act(d.id, 'showInFolder'); } }));
      }
      if (d.state === 'inProgress') links.push(el('button', { text: 'Pause', onclick: function () { act(d.id, 'pause'); } }));
      if (d.state === 'paused') links.push(el('button', { text: 'Resume', onclick: function () { act(d.id, 'resume'); } }));
      if (running) links.push(el('button', { text: 'Cancel', onclick: function () { act(d.id, 'cancel'); } }));
      else links.push(el('button', { text: 'Remove from list', onclick: function () { act(d.id, 'remove'); } }));
      list.appendChild(el('div', { class: 'row dl' + (failed ? ' failed' : ''), 'data-id': d.id }, [
        el('span', { class: 'file-icon', text: ext || 'FILE' }),
        el('div', { class: 'info' }, [
          el('div', { class: 'name', title: d.path, text: d.fileName }),
          el('div', { class: 'sub', text: Page.host(d.url) + ' · ' + status }),
          running ? el('progress', d.total ? { max: d.total, value: d.received } : {}) : null,
          el('div', { class: 'links' }, links),
        ]),
      ]));
    });
  }

  LB.on('downloads', function (m) { render(m.items); });
  document.getElementById('clear').addEventListener('click', function () { LB.request('clearDownloads').then(load); });

  function load() { LB.request('getDownloads').then(render); }
  LB.post('ready');
  load();
})();
