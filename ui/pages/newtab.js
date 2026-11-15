// New tab page: search box, shortcut tiles (most visited by frecency from local history, or
// the user's own list), a keyboard-shortcut list and a customize panel (0.6.0): background
// none/colour/image with dim and blur, tile source and grid, drag-to-reorder, add/edit/pin/
// remove tiles. Nothing here touches the network.
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);
  const box = $('tiles');
  let config = null;
  let tiles = [];

  // ---------- options ----------
  function applyConfig(c) {
    config = c;
    const n = c.ntp || {};
    $('q').placeholder = c.searchName ? 'Search with ' + c.searchName + ' or enter address' : 'Search the web or type an address';
    // Background (styles through the CSSOM: the page's CSP has no inline styles).
    const bg = $('bg'), dim = $('dim');
    // A built-in wallpaper (0.7.0) or the user's own image; a light wallpaper gets dark text.
    const imageUrl = n.background === 'wallpaper' ? c.wallpaperUrl : n.background === 'image' ? c.imageUrl : null;
    document.body.classList.toggle('has-image', !!imageUrl);
    document.body.classList.toggle('image-light', !!imageUrl && n.background === 'wallpaper' && c.wallpaperLook === 'light');
    if (imageUrl) {
      bg.style.backgroundImage = 'url("' + imageUrl + '")';
      bg.style.backgroundColor = '';
      bg.style.filter = n.imageBlur ? 'blur(' + n.imageBlur + 'px)' : '';
      dim.style.opacity = String((n.imageDim || 0) / 100);
    } else if (n.background === 'color' && n.backgroundColor) {
      bg.style.backgroundImage = '';
      bg.style.backgroundColor = n.backgroundColor;
      bg.style.filter = '';
      dim.style.opacity = '0';
    } else {
      bg.style.backgroundImage = '';
      bg.style.backgroundColor = '';
      bg.style.filter = '';
      dim.style.opacity = '0';
    }
    $('search').hidden = n.showSearch === false;
    box.hidden = n.showTiles === false;
    box.style.setProperty('--cols', String(n.columns || 6));
    renderKeys(c.shortcuts || [], n.showShortcuts !== false);
    renderWallpapers(c.wallpapers || [], n);
    fillPanel(n);
  }

  // The built-in wallpapers as thumbnails; a click chooses one (with its usual darkening).
  function renderWallpapers(list, n) {
    const box = $('wallpapers');
    box.textContent = '';
    list.forEach(function (w) {
      const on = n.background === 'wallpaper' && n.wallpaper === w.id;
      const b = el('button', { type: 'button', class: 'wallpaper' + (on ? ' on' : ''), title: w.title, 'aria-label': w.title, 'aria-pressed': on ? 'true' : 'false', 'data-id': w.id },
        [el('img', { src: w.thumb, alt: '' })]);
      b.addEventListener('click', function () { saveOptions({ background: 'wallpaper', wallpaper: w.id, imageDim: w.dim, imageBlur: 0 }); });
      box.appendChild(b);
    });
  }

  function renderKeys(list, show) {
    const ul = $('keys-list');
    ul.textContent = '';
    list.forEach(function (pair) {
      ul.appendChild(el('li', {}, [el('span', { text: pair[0] }), el('kbd', { text: pair[1] })]));
    });
    $('keys').hidden = !show || !list.length;
  }
  $('keys-hide').addEventListener('click', function () { saveOptions({ showShortcuts: false }); });

  function currentNtp() { return Object.assign({}, (config && config.ntp) || {}); }
  function saveOptions(changes) {
    const ntp = Object.assign(currentNtp(), changes);
    return LB.request('setNtpOptions', { ntp: ntp }).then(function (c) { applyConfig(c); load(); $('panel-error').textContent = ''; },
      function (e) { $('panel-error').textContent = e.message; });
  }

  // ---------- customize panel ----------
  const panel = $('panel');
  // A field being edited keeps its text (the host pushes the config while the user types).
  function setField(id, v) { const e = $(id); if (document.activeElement !== e) e.value = v; }
  function fillPanel(n) {
    document.querySelectorAll('input[name=bg]').forEach((r) => { r.checked = r.value === (n.background || 'none'); });
    setField('bg-color', n.backgroundColor || '');
    if (typeof colorPicker !== 'undefined') colorPicker.sync();
    $('dim-range').value = n.imageDim || 0; $('dim-val').textContent = (n.imageDim || 0) + ' %';
    $('blur-range').value = n.imageBlur || 0; $('blur-val').textContent = (n.imageBlur || 0) + ' px';
    document.querySelectorAll('input[name=tiles]').forEach((r) => { r.checked = r.value === (n.tiles || 'mostVisited'); });
    setField('cols', n.columns || 6); setField('rows', n.rows || 2);
    $('show-search').checked = n.showSearch !== false;
    $('show-tiles').checked = n.showTiles !== false;
    $('show-keys').checked = n.showShortcuts !== false;
  }
  $('customize').addEventListener('click', function () { fillPanel(currentNtp()); panel.showModal(); });
  $('panel-close').addEventListener('click', function () { panel.close(); });
  document.querySelectorAll('input[name=bg]').forEach((r) => r.addEventListener('change', function () {
    if (r.value === 'image' && !(config && config.imageUrl)) { pickImage(); return; }
    if (r.value === 'wallpaper') {
      // The current one, else the first.
      const list = (config && config.wallpapers) || [];
      const cur = list.find((w) => w.id === currentNtp().wallpaper) || list[0];
      if (cur) saveOptions({ background: 'wallpaper', wallpaper: cur.id, imageDim: cur.dim, imageBlur: 0 });
      return;
    }
    saveOptions({ background: r.value });
  }));
  const colorPicker = ColorPicker.attach($('bg-color'), { presets: ['#ffffff', '#f3f4f6', '#1f2023', '#111827', '#1e1b4b', '#14532d'], label: 'Background colour', resetLabel: 'Reset' });
  $('bg-color').addEventListener('change', function () {
    const v = $('bg-color').value.trim();
    saveOptions(v ? { background: 'color', backgroundColor: v } : { background: 'none', backgroundColor: '' });
  });
  $('pick-image').addEventListener('click', pickImage);
  function pickImage() {
    LB.request('ntpPickImage').then(function (c) { applyConfig(c); $('panel-error').textContent = ''; }, function (e) { $('panel-error').textContent = e.message; fillPanel(currentNtp()); });
  }
  $('dim-range').addEventListener('input', function () { $('dim-val').textContent = $('dim-range').value + ' %'; });
  $('dim-range').addEventListener('change', function () { saveOptions({ imageDim: Number($('dim-range').value) }); });
  $('blur-range').addEventListener('input', function () { $('blur-val').textContent = $('blur-range').value + ' px'; });
  $('blur-range').addEventListener('change', function () { saveOptions({ imageBlur: Number($('blur-range').value) }); });
  document.querySelectorAll('input[name=tiles]').forEach((r) => r.addEventListener('change', function () { saveOptions({ tiles: r.value }); }));
  $('cols').addEventListener('change', function () { saveOptions({ columns: Number($('cols').value) }); });
  $('rows').addEventListener('change', function () { saveOptions({ rows: Number($('rows').value) }); });
  $('show-search').addEventListener('change', function () { saveOptions({ showSearch: $('show-search').checked }); });
  $('show-tiles').addEventListener('change', function () { saveOptions({ showTiles: $('show-tiles').checked }); });
  $('show-keys').addEventListener('change', function () { saveOptions({ showShortcuts: $('show-keys').checked }); });

  // ---------- tiles ----------
  const tileDialog = $('tile-dialog');
  let editing = null;
  function openTileDialog(t) {
    editing = t || null;
    $('tile-title').textContent = t ? 'Edit shortcut' : 'Add shortcut';
    $('tile-name').value = t ? t.title : '';
    $('tile-url').value = t ? t.url : '';
    $('tile-error').textContent = '';
    tileDialog.showModal();
    $('tile-name').focus();
  }
  $('tile-cancel').addEventListener('click', function () { tileDialog.close(); });
  $('tile-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const title = $('tile-name').value.trim(), url = $('tile-url').value.trim();
    if (!url) { $('tile-error').textContent = 'Enter an address'; return; }
    const req = editing ? LB.request('editTile', { url: editing.url, newUrl: url, title: title })
      : LB.request('addTile', { url: url, title: title });
    req.then(function () { tileDialog.close(); load(); }, function (err) { $('tile-error').textContent = err.message; });
  });

  // Drag to reorder: the dragged tile drops before/after another; the resulting order of all
  // tiles is sent (tiles that were only "most visited" become pinned by the reorder).
  let dragUrl = null;
  function clearMarks() { box.querySelectorAll('.drop-before, .drop-after').forEach((n) => n.classList.remove('drop-before', 'drop-after')); }
  function reorderTo(target, after) {
    const order = tiles.map((t) => t.url).filter((u) => u !== dragUrl && !(pendingRemoval && pendingRemoval.tile.url === u));
    let i = order.indexOf(target.url);
    if (i < 0) return;
    if (after) i += 1;
    order.splice(i, 0, dragUrl);
    LB.request('reorderTiles', { urls: order }).then(load, function (e) { Page.toast(e.message); });
  }

  // ---------- tile menu (0.22.0) ----------
  // One "⋮" button per tile opens a small menu: Pin/Unpin, Edit, Remove. Arrow keys move
  // between the items, Escape or a click elsewhere closes it and focus goes back.
  let openMenu = null;
  function closeMenu(refocus) {
    if (!openMenu) return;
    const m = openMenu;
    openMenu = null;
    m.menu.remove();
    m.button.setAttribute('aria-expanded', 'false');
    m.button.closest('.tile-wrap')?.classList.remove('menu-open');
    if (refocus) m.button.focus();
  }
  function moreButton(t) {
    const b = el('button', { type: 'button', class: 'tile-more', title: 'Options for ' + t.title, 'aria-label': 'Options for ' + t.title, 'aria-haspopup': 'menu', 'aria-expanded': 'false', draggable: 'false', text: '⋮' });
    // Nothing from here reaches the link (or starts a drag of it).
    ['mousedown', 'mouseup', 'auxclick', 'dragstart'].forEach((k) => b.addEventListener(k, function (e) { e.stopPropagation(); if (k !== 'mousedown') e.preventDefault(); }));
    b.addEventListener('click', function (e) {
      e.preventDefault();
      e.stopPropagation();
      const was = openMenu && openMenu.button === b;
      closeMenu(false);
      if (!was) showMenu(b, t, e.detail === 0);
    });
    b.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown') { e.preventDefault(); closeMenu(false); showMenu(b, t, true); }
    });
    return b;
  }
  function showMenu(b, t, focusFirst) {
    const item = (label, fn) => el('button', { type: 'button', role: 'menuitem', class: 'tile-menu-item', text: label, onclick: function (e) { e.preventDefault(); e.stopPropagation(); closeMenu(false); fn(); } });
    const menu = el('div', { class: 'tile-menu', role: 'menu', 'aria-label': t.title }, [
      item(t.pinned ? 'Unpin' : 'Pin', function () {
        LB.request(t.pinned ? 'unpinSite' : 'pinSite', t.pinned ? { url: t.url } : { url: t.url, title: t.title }).then(load, function (err) { Page.toast(err.message); });
      }),
      item('Edit…', function () { openTileDialog(t); }),
      item('Remove', function () { removeTile(t); }),
    ]);
    menu.addEventListener('keydown', function (e) {
      const items = Array.from(menu.querySelectorAll('[role=menuitem]'));
      const i = items.indexOf(document.activeElement);
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeMenu(true); }
      else if (e.key === 'ArrowDown') { e.preventDefault(); items[(i + 1) % items.length].focus(); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); items[(i - 1 + items.length) % items.length].focus(); }
      else if (e.key === 'Home') { e.preventDefault(); items[0].focus(); }
      else if (e.key === 'End') { e.preventDefault(); items[items.length - 1].focus(); }
      else if (e.key === 'Tab') closeMenu(false);
    });
    ['mousedown', 'mouseup', 'dragstart'].forEach((k) => menu.addEventListener(k, function (e) { e.stopPropagation(); }));
    const wrap = b.closest('.tile-wrap');
    wrap.appendChild(menu);
    wrap.classList.add('menu-open');
    b.setAttribute('aria-expanded', 'true');
    openMenu = { button: b, menu: menu };
    if (focusFirst) menu.querySelector('[role=menuitem]').focus();
  }
  document.addEventListener('mousedown', function (e) { if (openMenu && !openMenu.menu.contains(e.target) && e.target !== openMenu.button) closeMenu(false); });
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && openMenu) closeMenu(true); });

  // Remove takes the tile away at once and does it for real after a few seconds, unless Undo.
  let pendingRemoval = null;
  const undoBar = el('div', { class: 'undo-toast', role: 'status', hidden: true }, [
    el('span', { class: 'undo-text' }),
    el('button', { type: 'button', class: 'link undo-btn', text: 'Undo' }),
  ]);
  document.body.appendChild(undoBar);
  undoBar.querySelector('.undo-btn').addEventListener('click', function () {
    if (!pendingRemoval) return;
    clearTimeout(pendingRemoval.timer);
    pendingRemoval = null;
    undoBar.hidden = true;
    render(tiles);
  });
  function commitRemoval() {
    const p = pendingRemoval;
    if (!p) return Promise.resolve();
    clearTimeout(p.timer);
    pendingRemoval = null;
    undoBar.hidden = true;
    const t = p.tile;
    tiles = tiles.filter(function (x) { return x.url !== t.url; });
    const req = t.pinned ? LB.request('unpinSite', { url: t.url }).then(function () { return LB.request('hideSite', { url: t.url }); })
      : LB.request('hideSite', { url: t.url });
    return req.catch(function (err) { Page.toast(err.message); });
  }
  function removeTile(t) {
    const prev = commitRemoval();
    const all = tiles;
    pendingRemoval = { tile: t, timer: setTimeout(function () { commitRemoval().then(load); }, 6000) };
    render(all);
    undoBar.querySelector('.undo-text').textContent = 'Removed “' + (t.title || t.url) + '”';
    undoBar.hidden = false;
    undoBar.querySelector('.undo-btn').focus();
    return prev;
  }
  window.addEventListener('pagehide', function () { commitRemoval(); });

  function render(list) {
    tiles = list;
    box.textContent = '';
    const n = (config && config.ntp) || {};
    if (!list.length && n.tiles !== 'custom') {
      box.appendChild(el('p', { id: 'empty', text: 'Sites you visit often will appear here.' }));
    }
    list.forEach(function (t) {
      if (pendingRemoval && pendingRemoval.tile.url === t.url) return;
      // 0.22.0: the link holds only the avatar, the label and (pinned) a pin mark; the actions
      // are behind one "⋮" button next to it (a sibling, so a click on it never reaches the
      // link), with a hit area larger than its icon.
      const tile = el('a', { class: 'tile' + (t.pinned ? ' pinned' : ''), href: t.url, title: t.title + '\n' + t.url, draggable: 'true', 'data-url': t.url }, [
        el('span', { class: 'avatar' }, [Page.favicon(t.url, t.favicon)]),
        el('span', { class: 'label', text: t.title }),
        t.pinned ? el('span', { class: 'pin-mark', title: 'Pinned', 'aria-label': 'Pinned' }, [Page.svg(Page.ICONS.pin)]) : null,
      ]);
      const wrap = el('div', { class: 'tile-wrap' }, [tile, moreButton(t)]);
      tile.addEventListener('dragstart', function (e) { dragUrl = t.url; tile.classList.add('dragging'); e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData('text/plain', t.url); });
      tile.addEventListener('dragend', function () { dragUrl = null; tile.classList.remove('dragging'); clearMarks(); });
      tile.addEventListener('dragover', function (e) {
        if (!dragUrl || dragUrl === t.url) return;
        e.preventDefault();
        const r = tile.getBoundingClientRect();
        const after = e.clientX > r.left + r.width / 2;
        clearMarks();
        tile.classList.add(after ? 'drop-after' : 'drop-before');
      });
      tile.addEventListener('drop', function (e) {
        if (!dragUrl || dragUrl === t.url) return;
        e.preventDefault();
        const r = tile.getBoundingClientRect();
        reorderTo(t, e.clientX > r.left + r.width / 2);
      });
      box.appendChild(wrap);
    });
    // "Add" tile: a custom entry by hand.
    const add = el('a', { class: 'tile add', href: '#', id: 'add-tile', title: 'Add shortcut' }, [
      el('span', { class: 'avatar', text: '+' }), el('span', { class: 'label', text: 'Add shortcut' }),
    ]);
    add.addEventListener('click', function (e) { e.preventDefault(); openTileDialog(null); });
    box.appendChild(add);
  }

  function load() {
    LB.request('getTopSites').then(render, function () { render([]); });
  }
  LB.on('newTabConfig', function (m) { applyConfig(m.config); load(); });

  document.getElementById('search').addEventListener('submit', function (e) {
    e.preventDefault();
    const q = document.getElementById('q').value.trim();
    if (q) LB.post('navigate', { input: q });
  });

  LB.post('ready');
  LB.request('getNewTabConfig').then(function (c) { applyConfig(c); load(); }, function () { load(); });
})();
