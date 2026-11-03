// Bookmarks manager: folder tree, list, search, rename/move/delete, drag
// and drop, and Netscape HTML import/export (native file dialogs).
(function () {
  'use strict';
  const el = LB.el;
  const BAR = 1;
  let roots = [];
  let selected = BAR;
  let query = '';
  let dragId = null;

  function walk(nodes, fn, depth) {
    (nodes || []).forEach(function (n) { fn(n, depth || 0); walk(n.children, fn, (depth || 0) + 1); });
  }
  function find(id) {
    let hit = null;
    walk(roots, function (n) { if (n.id === id) hit = n; });
    return hit;
  }
  function folders() {
    const out = [];
    walk(roots, function (n, d) { if (n.isFolder) out.push({ node: n, depth: d }); });
    return out;
  }

  function renderTree() {
    const tree = document.getElementById('tree');
    tree.textContent = '';
    folders().forEach(function (f) {
      const b = el('button', {
        class: 'folder' + (f.node.id === selected && !query ? ' selected' : ''), 'data-id': f.node.id,
        onclick: function () { selected = f.node.id; query = ''; document.getElementById('q').value = ''; render(); },
      }, [Page.svg(Page.ICONS.folder), el('span', { text: f.node.title })]);
      b.style.paddingLeft = (8 + f.depth * 16) + 'px';
      b.addEventListener('dragover', function (e) { if (dragId !== null && dragId !== f.node.id) { e.preventDefault(); b.classList.add('drop'); } });
      b.addEventListener('dragleave', function () { b.classList.remove('drop'); });
      b.addEventListener('drop', function (e) {
        e.preventDefault();
        b.classList.remove('drop');
        if (dragId !== null) move(dragId, f.node.id, 1e6);
      });
      tree.appendChild(b);
    });
  }

  function row(n, inSearch) {
    const r = el('div', { class: 'row', draggable: 'true', 'data-id': n.id }, [
      n.isFolder ? el('span', { class: 'folder-icon' }, [Page.svg(Page.ICONS.folder)]) : Page.favicon(n.url),
      el('span', { class: 'main' }, [
        n.isFolder
          ? el('span', { class: 'name', text: n.title, onclick: function () { selected = n.id; render(); } })
          : el('a', { href: n.url, title: n.url, text: n.title }),
        n.isFolder ? null : el('span', { class: 'sub', text: n.url }),
      ]),
      el('span', { class: 'actions' }, [
        Page.iconButton('edit', 'Edit', function () { edit(n); }),
        Page.iconButton('trash', 'Delete', function () { remove(n); }),
      ]),
    ]);
    r.addEventListener('dragstart', function (e) { dragId = n.id; r.classList.add('dragging'); e.dataTransfer.setData('text/plain', String(n.id)); });
    r.addEventListener('dragend', function () { dragId = null; r.classList.remove('dragging'); });
    if (!inSearch) {
      r.addEventListener('dragover', function (e) { if (dragId !== null && dragId !== n.id) { e.preventDefault(); r.classList.add('drop-before'); } });
      r.addEventListener('dragleave', function () { r.classList.remove('drop-before'); });
      r.addEventListener('drop', function (e) {
        e.preventDefault();
        r.classList.remove('drop-before');
        const siblings = find(selected).children || [];
        const from = siblings.findIndex(function (s) { return s.id === dragId; });
        let to = siblings.findIndex(function (s) { return s.id === n.id; });
        if (from >= 0 && from < to) to -= 1;
        if (dragId !== null) move(dragId, selected, to);
      });
    }
    return r;
  }

  function renderItems(items, title, inSearch) {
    document.getElementById('folder-title').textContent = title;
    const box = document.getElementById('items');
    box.textContent = '';
    if (!items.length) box.appendChild(el('div', { class: 'empty', text: inSearch ? 'No matches.' : 'This folder is empty.' }));
    items.forEach(function (n) { box.appendChild(row(n, inSearch)); });
  }

  function render() {
    renderTree();
    if (query) {
      LB.request('searchBookmarks', { text: query }).then(function (items) {
        renderItems(items, 'Search results for “' + query + '”', true);
      });
      return;
    }
    const f = find(selected) || find(BAR);
    if (f) { selected = f.id; renderItems(f.children || [], f.title, false); }
  }

  function load() {
    return LB.request('getBookmarks').then(function (tree) { roots = tree; render(); });
  }

  function move(id, parent, index) {
    LB.request('moveBookmark', { id: id, parent: parent, index: Math.max(0, Math.min(index, 1e6)) })
      .then(load, function (e) { Page.toast(e.message); });
  }

  function remove(n) {
    const what = n.isFolder ? 'the folder “' + n.title + '” and everything in it' : '“' + n.title + '”';
    Page.confirm('Delete ' + what + '?', { ok: 'Delete', danger: true }).then(function (yes) {
      if (yes) LB.request('deleteBookmark', { id: n.id }).then(function () { Page.toast('Deleted'); load(); });
    });
  }

  const dialog = document.getElementById('edit-dialog');
  let editing = null;
  function edit(n) {
    editing = n;
    document.getElementById('edit-title').textContent = n.isFolder ? 'Rename folder' : 'Edit bookmark';
    document.getElementById('edit-name').value = n.title;
    document.getElementById('edit-url').value = n.url || '';
    document.getElementById('edit-url-row').hidden = n.isFolder;
    const sel = document.getElementById('edit-folder');
    sel.textContent = '';
    folders().forEach(function (f) {
      // A folder cannot be moved into itself or below itself.
      let inside = false;
      walk([n], function (x) { if (x.id === f.node.id) inside = true; });
      if (inside) return;
      sel.appendChild(el('option', { value: f.node.id, text: ' '.repeat(f.depth * 3) + f.node.title }));
    });
    sel.value = String(n.parent);
    dialog.showModal();
  }
  dialog.addEventListener('close', function () {
    if (dialog.returnValue !== 'ok' || !editing) return;
    const n = editing;
    const name = document.getElementById('edit-name').value.trim();
    const url = document.getElementById('edit-url').value.trim();
    const parent = Number(document.getElementById('edit-folder').value);
    let p = LB.request('editBookmark', { id: n.id, title: name, url: n.isFolder ? null : url });
    if (parent !== n.parent) p = p.then(function () { return LB.request('moveBookmark', { id: n.id, parent: parent, index: 1e6 }); });
    p.then(load, function (e) { Page.toast(e.message); });
  });

  document.getElementById('add-folder').addEventListener('click', function () {
    Page.prompt('Folder name', 'New folder', { ok: 'Create' }).then(function (name) {
      if (name !== null) LB.request('addFolder', { parent: selected, title: name }).then(load);
    });
  });
  document.getElementById('import').addEventListener('click', function () {
    LB.request('importBookmarks').then(function (r) {
      if (r) Page.toast('Imported ' + r.bookmarks + ' bookmarks, ' + r.folders + ' folders' + (r.skipped ? ' (' + r.skipped + ' skipped)' : ''));
      load();
    }, function (e) { Page.toast(e.message); });
  });
  document.getElementById('export').addEventListener('click', function () {
    LB.request('exportBookmarks').then(function (r) { if (r) Page.toast('Exported to ' + r); }, function (e) { Page.toast(e.message); });
  });
  document.getElementById('q').addEventListener('input', Page.debounce(function (e) {
    query = e.target.value.trim();
    render();
  }, 150));

  LB.post('ready');
  load();
})();
