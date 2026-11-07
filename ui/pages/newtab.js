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
    const order = tiles.map((t) => t.url).filter((u) => u !== dragUrl);
    let i = order.indexOf(target.url);
    if (i < 0) return;
    if (after) i += 1;
    order.splice(i, 0, dragUrl);
    LB.request('reorderTiles', { urls: order }).then(load, function (e) { Page.toast(e.message); });
  }

  function render(list) {
    tiles = list;
    box.textContent = '';
    const n = (config && config.ntp) || {};
    if (!list.length && n.tiles !== 'custom') {
      box.appendChild(el('p', { id: 'empty', text: 'Sites you visit often will appear here.' }));
    }
    list.forEach(function (t) {
      const pinBtn = Page.iconButton('pin', t.pinned ? 'Unpin' : 'Pin', function (e) {
        e.preventDefault();
        LB.request(t.pinned ? 'unpinSite' : 'pinSite', t.pinned ? { url: t.url } : { url: t.url, title: t.title }).then(load);
      });
      pinBtn.classList.add('pin');
      const tile = el('a', { class: 'tile' + (t.pinned ? ' pinned' : ''), href: t.url, title: t.title + '\n' + t.url, draggable: 'true', 'data-url': t.url }, [
        t.pinned ? el('span', { class: 'pinned-mark', title: 'Pinned' }, [Page.svg(Page.ICONS.pin)]) : null,
        el('span', { class: 'avatar' }, [Page.favicon(t.url, t.favicon)]),
        el('span', { class: 'label', text: t.title }),
        el('span', { class: 'tools' }, [
          Page.iconButton('edit', 'Edit', function (e) { e.preventDefault(); openTileDialog(t); }),
          pinBtn,
          Page.iconButton('close', 'Remove', function (e) {
            e.preventDefault();
            const req = t.pinned ? LB.request('unpinSite', { url: t.url }).then(function () { return LB.request('hideSite', { url: t.url }); })
              : LB.request('hideSite', { url: t.url });
            req.then(load);
          }),
        ]),
      ]);
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
      box.appendChild(tile);
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
