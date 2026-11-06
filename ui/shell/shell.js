// Krunk shell: tab strip, toolbar and address bar.
// All state comes from the host (`tabs`, `nav`, `shellConfig` events); the
// shell only renders it and sends commands back.
(function () {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const el = LB.el;

  const state = { tabs: [], active: null, nav: null, editing: false, popup: null };

  // ---------- tabs ----------
  // One node per tab, kept across updates, so the strip animates: a closed tab shrinks away
  // (the host closed it at once; only the strip animates), a new one grows in, and the others
  // slide along (their widths transition). Closing with the mouse keeps the other tabs' widths
  // while the pointer stays on the strip, as in Chrome: the next tab's close button comes to
  // rest under the pointer. They take their new widths when it leaves.
  const tabNodes = new Map();    // tab id -> node
  let renderedOnce = false;
  let widthsFrozen = false;
  const CLOSE_MS = 160;
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const liveTabs = () => document.querySelectorAll('#tabs .tab:not(.closing)');
  function tabById(id) { return state.tabs.find(function (t) { return t.id === id; }); }

  function newTabNode(id) {
    const node = el('div', {
      class: 'tab', role: 'tab', 'data-id': id,
      onmousedown: function (e) {
        if (e.button !== 0 || e.target.closest('.close')) return;
        // Double-click duplicates. Detected here rather than with dblclick: the first press
        // activates the tab and re-renders it, which can swallow a dblclick.
        if (lastPress.id === id && e.timeStamp - lastPress.at < DOUBLE_PRESS_MS) {
          lastPress = { id: null, at: 0 };
          LB.post('duplicateTab', { id: id });
          return;
        }
        lastPress = { id: id, at: e.timeStamp };
        LB.post('activateTab', { id: id });
      },
      onauxclick: function (e) {
        if (e.button === 1) { e.preventDefault(); closeWithMouse(id); }
      },
      oncontextmenu: function (e) {
        e.preventDefault();
        LB.post('showTabMenu', { id: id, rect: { x: e.clientX, y: e.clientY, width: 240, height: 256 } });
        setPopup('tabMenu');
      },
    });
    attachDrag(node, id);
    return node;
  }

  function fillTabNode(node, t, index) {
    const active = t.id === state.active;
    node.className = 'tab' + (active ? ' active' : '') + (t.private ? ' private' : '') +
      (t.sleeping ? ' sleeping' : '') + (node.classList.contains('opening') ? ' opening' : '') +
      (node.classList.contains('dragging') ? ' dragging' : '');
    node.setAttribute('aria-selected', active ? 'true' : 'false');
    node.setAttribute('title', (t.private ? '[Private] ' : '') + t.title + (t.url ? '\n' + t.url : ''));
    node.dataset.index = index;
    const icon = t.loading
      ? el('span', { class: 'spinner', 'aria-label': 'Loading' })
      : t.favicon
        ? el('img', { class: 'favicon', src: t.favicon, alt: '' })
        : svgIcon('i-globe', 'favicon');
    node.replaceChildren.apply(node, [
      t.private ? el('span', { class: 'private-mark', title: 'Private tab' }, [svgIcon('i-mask')]) : null,
      icon,
      el('span', { class: 'title', text: t.title || 'New tab' }),
      el('button', {
        class: 'close', title: 'Close tab (Ctrl+W)', 'aria-label': 'Close tab',
        onclick: function (e) { e.stopPropagation(); closeWithMouse(t.id); },
      }, [svgIcon('i-close')]),
    ].filter(Boolean));
  }

  // Close button or middle click: the other tabs keep their widths for now.
  function closeWithMouse(id) {
    if (!reduceMotion.matches) freezeWidths();
    LB.post('closeTab', { id: id });
  }
  // Style changes that must not animate themselves (pinning a width the tab already has).
  function instantly(fn) {
    const box = $('tabs');
    box.classList.add('no-anim');
    fn();
    void box.offsetWidth;
    box.classList.remove('no-anim');
  }
  function freezeWidths() {
    const tabs = Array.prototype.slice.call(liveTabs());
    const w = tabs.map(function (n) { return n.getBoundingClientRect().width; }); // all before any change
    instantly(function () {
      tabs.forEach(function (n, i) { n.style.flex = '0 0 ' + w[i] + 'px'; });
      widthsFrozen = true;
      layoutTabs();
    });
  }
  function thawWidths() {
    if (!widthsFrozen) return;
    widthsFrozen = false;
    liveTabs().forEach(function (n) { n.style.flex = ''; });
    layoutTabs();
  }
  $('tabstrip').addEventListener('pointerleave', thawWidths);

  // A closed tab's node shrinks to nothing, then goes.
  function closeOut(node) {
    node.classList.remove('active', 'opening', 'dragging', 'drop-before', 'drop-after');
    const w = node.getBoundingClientRect().width;
    instantly(function () { node.style.flex = '0 0 ' + w + 'px'; });
    node.classList.add('closing');
    node.style.flexBasis = '0px';
    let done = false;
    const finish = function () {
      if (done) return;
      done = true;
      node.remove();
      layoutTabs();
    };
    node.addEventListener('transitionend', function (e) { if (e.propertyName === 'flex-basis') finish(); });
    setTimeout(finish, CLOSE_MS + 120);
  }

  function renderTabs() {
    const box = $('tabs');
    const animate = renderedOnce && !reduceMotion.matches;
    const ids = new Set(state.tabs.map(function (t) { return t.id; }));
    tabNodes.forEach(function (node, id) {
      if (ids.has(id)) return;
      tabNodes.delete(id);
      if (animate && node.isConnected) closeOut(node);
      else node.remove();
    });
    let added = false;
    let after = null;
    state.tabs.forEach(function (t, index) {
      let node = tabNodes.get(t.id);
      if (!node) {
        node = newTabNode(t.id);
        tabNodes.set(t.id, node);
        if (animate) node.classList.add('opening');
        added = true;
      }
      fillTabNode(node, t, index);
      // In state order; closing tabs stay where they were meanwhile.
      const slot = after ? after.nextSibling : box.firstChild;
      let next = slot;
      while (next && next.classList.contains('closing')) next = next.nextSibling;
      if (next !== node) box.insertBefore(node, slot);
      after = node;
    });
    if (added) {
      // A new tab: the others take their new widths now.
      thawWidths();
      if (animate) {
        requestAnimationFrame(function () {
          requestAnimationFrame(function () {
            box.querySelectorAll('.tab.opening').forEach(function (n) { n.classList.remove('opening'); });
          });
        });
      }
    }
    renderedOnce = true;
    layoutTabs();
  }

  // Narrow tabs hide inactive close buttons; the active tab stays in view
  // when the row scrolls; the host gets the new hit-test map.
  const TAB_WIDTH = 256, TAB_GAP = 2;
  function layoutTabs() {
    const box = $('tabs');
    // The row asks for every tab at full width and shrinks (evenly, through
    // the tabs' own flex-shrink) when the window is narrower.
    const n = box.children.length; // closing tabs too: the row shrinks as they do
    // With frozen widths the row is as wide as its tabs, so "+" follows the last one.
    box.style.flexBasis = widthsFrozen ? 'auto' : n ? (n * TAB_WIDTH + (n - 1) * TAB_GAP) + 'px' : '0px';
    const first = box.querySelector('.tab:not(.closing)');
    box.classList.toggle('narrow', !!first && first.getBoundingClientRect().width < 100);
    const active = box.querySelector('.tab.active');
    if (active) active.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    reportHitMap();
  }

  // ---------- frameless window ----------
  // The host hit-tests the window: everything in the tab row that is not a
  // tab, the "+" button or a caption button drags the window.
  function rectOf(node) {
    const r = node.getBoundingClientRect();
    return { x: Math.max(0, r.left), y: Math.max(0, r.top), width: Math.max(0, r.width), height: Math.max(0, r.height) };
  }
  let lastMap = '';
  function reportHitMap() {
    const box = $('tabs');
    const tabs = liveTabs();
    const interactive = [rectOf($('newtab'))];
    if (tabs.length) {
      // The tabs as one band: the visible part of the scrolling row.
      const row = box.getBoundingClientRect();
      const last = tabs[tabs.length - 1].getBoundingClientRect();
      const right = Math.min(row.right, last.right);
      interactive.push({ x: Math.max(0, row.left), y: Math.max(0, last.top), width: Math.max(0, right - row.left), height: last.height });
    }
    // Tab centres: where a tab dragged from another window lands (frame::drop_index).
    const centers = Array.prototype.map.call(tabs, function (n) {
      const r = n.getBoundingClientRect();
      return Math.round((r.left + r.width / 2) * 10) / 10;
    }).slice(0, 512);
    const map = {
      stripHeight: $('tabstrip').getBoundingClientRect().height,
      interactive: interactive,
      tabCenters: centers,
      minimize: rectOf($('win-min')),
      maximize: rectOf($('win-max')),
      close: rectOf($('win-close')),
    };
    const json = JSON.stringify(map);
    if (json === lastMap) return;
    lastMap = json;
    LB.post('hitMap', { map: map });
  }
  new ResizeObserver(function () { layoutTabs(); }).observe($('tabstrip'));
  $('tabs').addEventListener('scroll', reportHitMap);
  // Tabs slide while they animate: the layout (narrow mode, scrolling) and the host's hit map
  // follow when they settle.
  $('tabs').addEventListener('transitionend', function (e) {
    if (e.propertyName === 'flex-basis' && !e.target.classList.contains('closing')) layoutTabs();
  });
  $('tabs').addEventListener('wheel', function (e) {
    const box = $('tabs');
    if (box.scrollWidth <= box.clientWidth) return;
    e.preventDefault();
    box.scrollLeft += Math.abs(e.deltaY) > Math.abs(e.deltaX) ? e.deltaY : e.deltaX;
  }, { passive: false });

  // Mouse clicks on these are handled natively (they are hit-tested as
  // caption buttons); a click event only arrives from keyboard or tests.
  [['win-min', 'minimize'], ['win-max', 'maximize'], ['win-close', 'close']].forEach(function (b) {
    $(b[0]).addEventListener('click', function () { LB.post('windowControl', { action: b[1] }); });
  });
  const CAPTION_IDS = { minimize: 'win-min', maximize: 'win-max', close: 'win-close' };
  LB.on('captionState', function (m) {
    Object.keys(CAPTION_IDS).forEach(function (k) {
      const node = $(CAPTION_IDS[k]);
      node.classList.toggle('hover', m.hover === k);
      node.classList.toggle('pressed', m.hover === k && !!m.pressed);
    });
  });
  LB.on('windowState', function (m) {
    const root = document.documentElement;
    root.classList.toggle('maximized', !!m.maximized);
    root.classList.toggle('inactive', !m.active);
    root.classList.toggle('fullscreen', !!m.fullscreen);
    $('win-max-use').setAttribute('href', m.maximized ? '#i-win-restore' : '#i-win-max');
    $('win-max').title = m.maximized ? 'Restore' : 'Maximize';
    $('win-max').setAttribute('aria-label', $('win-max').title);
    reportHitMap();
  });

  function svgIcon(name, cls) {
    const ns = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(ns, 'svg');
    svg.setAttribute('class', 'icon' + (cls ? ' ' + cls : ''));
    const use = document.createElementNS(ns, 'use');
    use.setAttribute('href', '#' + name);
    svg.appendChild(use);
    return svg;
  }

  let lastPress = { id: null, at: 0 };
  const DOUBLE_PRESS_MS = 500;

  // ---------- dragging tabs ----------
  // Pointer events, not HTML5 drag and drop (whose OLE loop the host could not take over).
  // Within the strip the tab reorders; once the pointer leaves the strip the host takes
  // the drag over (tabDragOut) and moves the tab into a new window or onto another one's strip.
  const DRAG_THRESHOLD = 5;   // px before a press becomes a drag
  const DETACH_DISTANCE = 24; // px above or below the strip before the tab leaves it
  let drag = null;            // { id, node, pointerId, startX, startY, offsetX, moved, to }
  function clearDropMarks() {
    document.querySelectorAll('.tab').forEach(function (n) {
      n.classList.remove('dragging', 'drop-before', 'drop-after');
    });
  }
  // Insert position for x among the other tabs (the index after removing the dragged one).
  function dropIndex(x, exceptId) {
    let index = 0;
    liveTabs().forEach(function (n) {
      if (Number(n.dataset.id) === exceptId) return;
      const r = n.getBoundingClientRect();
      if (r.left + r.width / 2 < x) index++;
    });
    return index;
  }
  // Marks where index `to` (among the tabs other than `exceptId`) is.
  function markDrop(to, exceptId) {
    const others = Array.prototype.filter.call(liveTabs(), function (n) {
      return Number(n.dataset.id) !== exceptId;
    });
    others.forEach(function (n) { n.classList.remove('drop-before', 'drop-after'); });
    if (!others.length) return;
    if (to < others.length) others[to].classList.add('drop-before');
    else others[others.length - 1].classList.add('drop-after');
  }
  // The press is on a tab; the capture and the move/up handling are on the strip itself,
  // because activating the pressed tab re-renders the tabs (the node goes away).
  function attachDrag(node, id) {
    node.addEventListener('pointerdown', function (e) {
      if (e.button !== 0 || e.target.closest('.close')) return;
      const r = node.getBoundingClientRect();
      drag = { id: id, pointerId: e.pointerId, startX: e.clientX, startY: e.clientY,
        offsetX: e.clientX - r.left, moved: false, to: null };
      $('tabs').setPointerCapture(e.pointerId);
    });
  }
  function dragNode() {
    return drag ? document.querySelector('#tabs .tab:not(.closing)[data-id="' + drag.id + '"]') : null;
  }
  $('tabs').addEventListener('pointermove', function (e) {
    if (!drag || e.pointerId !== drag.pointerId) return;
    if (!drag.moved) {
      if (Math.abs(e.clientX - drag.startX) < DRAG_THRESHOLD && Math.abs(e.clientY - drag.startY) < DRAG_THRESHOLD) return;
      drag.moved = true;
    }
    const node = dragNode();
    if (node) node.classList.add('dragging');
    const strip = $('tabstrip').getBoundingClientRect();
    if (e.clientY < strip.top - DETACH_DISTANCE || e.clientY > strip.bottom + DETACH_DISTANCE) {
      // Out of the strip: the host follows the mouse from here.
      const d = drag;
      drag = null;
      clearDropMarks();
      try { $('tabs').releasePointerCapture(d.pointerId); } catch (err) { /* already released */ }
      LB.post('tabDragOut', { id: d.id, offsetX: d.offsetX, offsetY: d.startY });
      return;
    }
    drag.to = dropIndex(e.clientX, drag.id);
    markDrop(drag.to, drag.id);
  });
  function finishDrag(e, commit) {
    if (!drag || e.pointerId !== drag.pointerId) return;
    const d = drag;
    drag = null;
    clearDropMarks();
    if (!commit || !d.moved || d.to === null) return;
    const from = state.tabs.findIndex(function (t) { return t.id === d.id; });
    if (from < 0 || from === d.to) return;
    const moved = state.tabs.splice(from, 1)[0];
    state.tabs.splice(d.to, 0, moved);
    renderTabs();
    LB.post('moveTab', { id: d.id, index: d.to });
  }
  $('tabs').addEventListener('pointerup', function (e) { finishDrag(e, true); });
  $('tabs').addEventListener('pointercancel', function (e) { finishDrag(e, false); });
  $('tabs').addEventListener('lostpointercapture', function (e) { finishDrag(e, false); });

  // ---------- docked DevTools ----------
  // Shown while the active tab has DevTools docked: move it to the other side, or close it.
  let devtoolsDock = 'right';
  LB.on('devTools', function (m) {
    $('devtools-ctl').hidden = !m.open;
    devtoolsDock = m.dock;
    const to = m.dock === 'right' ? 'bottom' : 'right';
    $('dt-dock-use').setAttribute('href', '#i-dock-' + to);
    const label = 'Dock DevTools ' + (to === 'bottom' ? 'at the bottom' : 'on the right');
    $('dt-dock').title = label;
    $('dt-dock').setAttribute('aria-label', label);
  });
  $('dt-dock').addEventListener('click', function () {
    LB.post('devToolsDock', { dock: devtoolsDock === 'right' ? 'bottom' : 'right' });
  });
  $('dt-close').addEventListener('click', function () { LB.post('devToolsClose'); });

  // A bookmark dragged out of a folder dropdown (the host follows the mouse): the marks
  // follow the cursor over the bar, and the release moves the bookmark there.
  function markBarTarget(t) {
    clearBarMarks();
    if (!t) return;
    if (t.into !== undefined) t.node.classList.add('drop-into');
    else if (t.others.length) {
      if (t.index < t.others.length) t.others[t.index].classList.add('drop-before');
      else t.others[t.others.length - 1].classList.add('drop-after');
    }
  }
  LB.on('barDragAt', function (m) { markBarTarget(barTarget(m.x)); });
  LB.on('barDragEnd', function () { clearBarMarks(); });
  LB.on('barDragDrop', function (m) {
    clearBarMarks();
    const t = barTarget(m.x);
    if (t.into !== undefined) {
      if (t.into !== m.id) LB.post('moveBookmark', { id: m.id, parent: t.into, index: 1000000 });
    } else {
      LB.post('moveBookmark', { id: m.id, parent: BAR_ROOT, index: t.index });
    }
  });

  // A tab dragged from another window is over this strip: where it would land.
  LB.on('tabDropHint', function (m) {
    clearDropMarks();
    if (m.index === null || m.index === undefined) return;
    const tabs = liveTabs();
    if (!tabs.length) return;
    if (m.index < tabs.length) tabs[m.index].classList.add('drop-before');
    else tabs[tabs.length - 1].classList.add('drop-after');
  });

  // ---------- toolbar ----------
  function renderNav() {
    const n = state.nav;
    if (!n) return;
    if (state.navTab !== undefined && state.navTab !== n.tabId) closeFind(false);
    state.navTab = n.tabId;
    const zoomed = Math.abs((n.zoom || 1) - 1) > 0.001;
    $('zoom').hidden = !zoomed;
    $('key').hidden = !n.loginForm;
    $('zoom').textContent = Math.round((n.zoom || 1) * 100) + '%';
    $('back').disabled = !n.canGoBack;
    $('forward').disabled = !n.canGoForward;
    $('reload-icon').setAttribute('href', n.loading ? '#i-stop' : '#i-reload');
    $('reload').title = n.loading ? 'Stop (Esc)' : 'Reload (Ctrl+R)';
    // The user's own text stays while they type; a focused but untouched field
    // follows the page (a bookmark opened while the caret was in the field).
    if (!(state.editing && state.dirty)) $('address').value = n.url;
    $('star').classList.toggle('bookmarked', !!n.bookmarked);
    // Built-in pages (krunk://) cannot be bookmarked.
    $('star').hidden = !n.url || n.url.startsWith('krunk://');
    const secure = n.secure;
    $('site-icon').classList.toggle('secure', secure);
    $('site-icon-use').setAttribute('href', secure ? '#i-lock' : '#i-info');
    $('site-icon').title = secure ? 'Connection is secure' : (n.url ? 'Connection is not secure' : '');
    $('private-badge').hidden = !n.private;
    $('omnibox').classList.toggle('private', !!n.private);
    const count = $('shield-count');
    count.hidden = !n.blocked;
    count.textContent = n.blocked > 99 ? '99+' : String(n.blocked);
    $('shield').classList.toggle('allowed', !!n.siteAllowed);
    $('shield').title = n.siteAllowed ? 'Ad blocking is off for this site'
      : n.blocked + ' ads and trackers blocked on this page';
  }

  $('back').addEventListener('click', function () { LB.post('back'); });
  $('forward').addEventListener('click', function () { LB.post('forward'); });
  $('reload').addEventListener('click', function () {
    LB.post(state.nav && state.nav.loading ? 'stop' : 'reload');
  });
  $('newtab').addEventListener('click', function () { LB.post('newTab', {}); });
  $('star').addEventListener('click', function () { LB.post('toggleBookmark'); });

  // ---------- address bar ----------
  const address = $('address');
  address.addEventListener('focus', function () { state.editing = true; state.dirty = false; address.select(); });
  address.addEventListener('blur', function () { state.editing = false; state.dirty = false; renderNav(); });
  address.addEventListener('input', function () { state.dirty = true; });
  address.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      address.value = state.nav ? state.nav.url : '';
      state.dirty = false;
      address.blur();
    }
  });
  $('omnibox').addEventListener('submit', function (e) {
    e.preventDefault();
    const input = address.value.trim();
    if (!input) return;
    LB.post('navigate', { input: input });
    state.editing = false;
    state.dirty = false;
    address.blur();
  });

  // ---------- popups (menu, shield) ----------
  const POPUP_SIZES = { menu: [280, 542], shield: [300, 226] };
  // The press on the button deactivates the open popup, which closes it before the click
  // arrives (the host then says popupClosed). The click must not reopen it: the press
  // remembers what was open. The host has the same guard (lb_core::toggle).
  let downWhileOpen = null;
  function rememberOpen(kind) { downWhileOpen = state.popup === kind ? kind : null; }
  function togglePopup(kind, button) {
    if (state.popup === kind || downWhileOpen === kind) {
      downWhileOpen = null;
      LB.post('hidePopup');
      setPopup(null);
      return;
    }
    const r = button.getBoundingClientRect();
    const size = POPUP_SIZES[kind];
    const x = Math.max(0, Math.min(r.right - size[0], window.innerWidth - size[0]));
    LB.post('showPopup', { popup: kind, rect: { x: x, y: r.bottom + 2, width: size[0], height: size[1] } });
    setPopup(kind);
  }
  function setPopup(kind) {
    if (kind !== null) downWhileOpen = null;
    state.popup = kind;
    $('menu').classList.toggle('on', kind === 'menu');
    $('shield').classList.toggle('on', kind === 'shield');
    document.documentElement.classList.toggle('tab-menu', kind === 'tabMenu');
    document.querySelectorAll('.bm[data-folder]').forEach(function (b) {
      b.classList.toggle('on', kind === 'folder:' + b.dataset.folder);
    });
  }

  // ---------- bookmarks bar ----------
  const BAR_ROOT = 1; // the "Bookmarks bar" folder
  function renderBar(items) {
    const bar = $('bookmarks-bar');
    bar.textContent = '';
    if (!items.length) {
      bar.appendChild(el('span', { class: 'bm-empty', text: 'Bookmarks in the “Bookmarks bar” folder appear here (Ctrl+D adds the page).' }));
      return;
    }
    items.forEach(function (b) {
      if (b.isFolder) {
        const btn = el('button', { class: 'bm folder', title: b.title, 'data-folder': b.id, 'data-id': b.id }, [
          svgIcon('i-folder', 'folder-icon'), el('span', { text: b.title }),
        ]);
        btn.addEventListener('pointerdown', function () { rememberOpen('folder:' + b.id); });
        btn.addEventListener('click', function () {
          if (barDrag.suppressClick) return;
          const key = 'folder:' + b.id;
          if (state.popup === key || downWhileOpen === key) { downWhileOpen = null; LB.post('hidePopup'); setPopup(null); return; }
          const r = btn.getBoundingClientRect();
          LB.post('showBookmarkFolder', { id: b.id, rect: { x: r.left, y: r.top, width: Math.max(1, r.width), height: Math.max(1, r.height) } });
          setPopup(key);
        });
        bar.appendChild(btn);
      } else {
        const btn = el('button', { class: 'bm', title: b.title + '\n' + b.url, 'data-id': b.id }, [
          b.favicon ? el('img', { src: b.favicon, alt: '', draggable: 'false' }) : svgIcon('i-globe'),
          el('span', { text: b.title }),
        ]);
        btn.addEventListener('click', function (e) {
          if (barDrag.suppressClick) return;
          LB.post('openUrl', { url: b.url, newTab: e.ctrlKey });
        });
        btn.addEventListener('auxclick', function (e) { if (e.button === 1) LB.post('openUrl', { url: b.url, newTab: true }); });
        bar.appendChild(btn);
      }
    });
  }

  // Right-click: the bar's menu for the item (bookmark or folder) or for the bar itself.
  $('bookmarks-bar').addEventListener('contextmenu', function (e) {
    e.preventDefault();
    const item = e.target.closest('.bm');
    LB.post('showBookmarkMenu', { id: item ? Number(item.dataset.id) : null, rect: { x: e.clientX, y: e.clientY, width: 240, height: 320 } });
    setPopup('bookmarkMenu');
  });

  // Dragging an item: along the bar it moves there (a mark shows where); over the middle of a
  // folder it goes into that folder (the folder lights up). Pointer events, as for tabs.
  const barDrag = { item: null, startX: 0, pointerId: null, moved: false, target: null, suppressClick: false };
  function barItems() { return Array.prototype.slice.call($('bookmarks-bar').querySelectorAll('.bm')); }
  function clearBarMarks() {
    barItems().forEach(function (n) { n.classList.remove('drop-before', 'drop-after', 'drop-into', 'dragging'); });
  }
  // Where a drop at x goes: { into: folderId } or { index } among the other items.
  function barTarget(x) {
    const others = barItems().filter(function (n) { return n !== barDrag.item; });
    let index = 0;
    for (let i = 0; i < others.length; i++) {
      const n = others[i];
      const r = n.getBoundingClientRect();
      if (n.classList.contains('folder') && x >= r.left + r.width / 4 && x <= r.right - r.width / 4) {
        return { into: Number(n.dataset.id), node: n };
      }
      if (x > r.left + r.width / 2) index = i + 1;
    }
    return { index: index, others: others };
  }
  $('bookmarks-bar').addEventListener('pointerdown', function (e) {
    const item = e.target.closest('.bm');
    if (!item || e.button !== 0) return;
    barDrag.item = item; barDrag.startX = e.clientX; barDrag.pointerId = e.pointerId;
    barDrag.moved = false; barDrag.target = null;
  });
  $('bookmarks-bar').addEventListener('pointermove', function (e) {
    if (!barDrag.item || e.pointerId !== barDrag.pointerId) return;
    if (!barDrag.moved) {
      if (Math.abs(e.clientX - barDrag.startX) < DRAG_THRESHOLD) return;
      barDrag.moved = true;
      $('bookmarks-bar').setPointerCapture(e.pointerId);
      LB.post('hidePopup'); setPopup(null);
    }
    clearBarMarks();
    barDrag.item.classList.add('dragging');
    const t = barTarget(e.clientX);
    barDrag.target = t;
    if (t.into !== undefined) t.node.classList.add('drop-into');
    else if (t.others.length) {
      if (t.index < t.others.length) t.others[t.index].classList.add('drop-before');
      else t.others[t.others.length - 1].classList.add('drop-after');
    }
  });
  function endBarDrag(e, commit) {
    if (!barDrag.item || e.pointerId !== barDrag.pointerId) return;
    const item = barDrag.item, t = barDrag.target, moved = barDrag.moved;
    barDrag.item = null;
    clearBarMarks();
    if (!moved) return;
    // The click that ends a drag does not open the bookmark.
    barDrag.suppressClick = true;
    setTimeout(function () { barDrag.suppressClick = false; }, 0);
    if (!commit || !t) return;
    const id = Number(item.dataset.id);
    if (t.into !== undefined) {
      if (t.into !== id) LB.post('moveBookmark', { id: id, parent: t.into, index: 1000000 });
    } else {
      LB.post('moveBookmark', { id: id, parent: BAR_ROOT, index: t.index });
    }
  }
  $('bookmarks-bar').addEventListener('pointerup', function (e) { endBarDrag(e, true); });
  $('bookmarks-bar').addEventListener('pointercancel', function (e) { endBarDrag(e, false); });

  // Dragging the page from the address bar (the site icon, or the field while it is not
  // being edited) onto the bar bookmarks it where it is dropped; a chip follows the
  // cursor. A plain click still focuses the field. The host takes the title and address
  // from the tab itself.
  const urlDrag = { pointerId: null, startX: 0, startY: 0, moved: false, chip: null, target: null, fromField: false };
  function urlDragStart(e, fromField) {
    if (e.button !== 0 || !state.nav || !state.nav.url || state.nav.url.startsWith('krunk://')) return;
    if (fromField && state.editing) return;
    // The field must not take the caret (and select its text, which would start a text drag)
    // on the press; a click without movement focuses it on release instead.
    if (fromField) e.preventDefault();
    urlDrag.pointerId = e.pointerId; urlDrag.startX = e.clientX; urlDrag.startY = e.clientY;
    urlDrag.moved = false; urlDrag.target = null; urlDrag.fromField = fromField;
  }
  $('site-icon').addEventListener('pointerdown', function (e) { urlDragStart(e, false); });
  address.addEventListener('pointerdown', function (e) { urlDragStart(e, true); });
  $('omnibox').addEventListener('pointermove', function (e) {
    if (urlDrag.pointerId !== e.pointerId) return;
    if (!urlDrag.moved) {
      if (Math.abs(e.clientX - urlDrag.startX) < DRAG_THRESHOLD && Math.abs(e.clientY - urlDrag.startY) < DRAG_THRESHOLD) return;
      urlDrag.moved = true;
      $('omnibox').setPointerCapture(e.pointerId);
      const tab = (state.tabs || []).find(function (x) { return x.id === state.nav.tabId; });
      const t = (tab && tab.title) || state.nav.url;
      urlDrag.chip = el('div', { id: 'url-chip', text: t });
      document.body.appendChild(urlDrag.chip);
    }
    urlDrag.chip.style.left = (e.clientX + 12) + 'px';
    urlDrag.chip.style.top = (e.clientY + 12) + 'px';
    const bar = $('bookmarks-bar');
    const br = bar.getBoundingClientRect();
    const overBar = !bar.hidden && e.clientY >= br.top && e.clientY <= br.bottom;
    urlDrag.target = overBar ? barTarget(e.clientX) : null;
    markBarTarget(urlDrag.target);
  });
  function endUrlDrag(e, commit) {
    if (urlDrag.pointerId !== e.pointerId) return;
    const moved = urlDrag.moved, t = urlDrag.target, fromField = urlDrag.fromField;
    urlDrag.pointerId = null;
    if (urlDrag.chip) { urlDrag.chip.remove(); urlDrag.chip = null; }
    clearBarMarks();
    if (!moved) {
      // A click: the field gets the caret as usual.
      if (fromField && commit) { address.focus(); address.select(); }
      return;
    }
    if (!commit || !t) return;
    if (t.into !== undefined) LB.post('bookmarkActivePage', { parent: t.into, index: 1000000 });
    else LB.post('bookmarkActivePage', { parent: BAR_ROOT, index: t.index });
  }
  $('omnibox').addEventListener('pointerup', function (e) { endUrlDrag(e, true); });
  $('omnibox').addEventListener('pointercancel', function (e) { endUrlDrag(e, false); });
  // Text dragged with the mouse (the selected address while editing, or from a page):
  // an http(s) address dropped on the bar becomes a bookmark there.
  $('bookmarks-bar').addEventListener('dragover', function (e) {
    if (!e.dataTransfer.types.includes('text/plain') && !e.dataTransfer.types.includes('text/uri-list')) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    const t = barTarget(e.clientX);
    markBarTarget(t);
  });
  $('bookmarks-bar').addEventListener('dragleave', function () { clearBarMarks(); });
  $('bookmarks-bar').addEventListener('drop', function (e) {
    e.preventDefault();
    clearBarMarks();
    const text = (e.dataTransfer.getData('text/uri-list') || e.dataTransfer.getData('text/plain') || '').trim().split('\n')[0];
    if (!/^https?:\/\/\S+$/i.test(text)) return;
    const t = barTarget(e.clientX);
    if (t.into !== undefined) LB.post('addBookmarkAt', { url: text, parent: t.into, index: 1000000 });
    else LB.post('addBookmarkAt', { url: text, parent: BAR_ROOT, index: t.index });
  });

  // ---------- find in page ----------
  const find = $('find');
  function runFind(backwards) {
    const q = find.value;
    if (!q) { $('find-count').textContent = ''; $('findbar').classList.remove('nomatch'); LB.post('findClose'); return; }
    LB.post('find', { query: q, backwards: !!backwards });
  }
  function closeFind(notify) {
    if ($('findbar').hidden) return;
    $('findbar').hidden = true;
    if (notify) LB.post('findClose');
  }
  find.addEventListener('input', Page_debounce(function () { runFind(false); }, 120));
  find.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); runFind(e.shiftKey); }
    if (e.key === 'Escape') { e.preventDefault(); closeFind(true); }
  });
  $('find-next').addEventListener('click', function () { runFind(false); });
  $('find-prev').addEventListener('click', function () { runFind(true); });
  $('find-close').addEventListener('click', function () { closeFind(true); });
  $('findbar').addEventListener('submit', function (e) { e.preventDefault(); });
  function Page_debounce(fn, ms) {
    let t;
    return function () { clearTimeout(t); t = setTimeout(fn, ms); };
  }
  $('key').addEventListener('click', function () { LB.post('requestFill'); });
  $('zoom').addEventListener('click', function () { LB.post('zoom', { action: 'reset' }); });
  $('menu').addEventListener('pointerdown', function () { rememberOpen('menu'); });
  $('menu').addEventListener('click', function () { togglePopup('menu', $('menu')); });
  $('shield').addEventListener('pointerdown', function () { rememberOpen('shield'); });
  $('shield').addEventListener('click', function () { togglePopup('shield', $('shield')); });

  // ---------- host events ----------
  LB.on('tabs', function (m) {
    state.tabs = m.tabs;
    state.active = m.active;
    renderTabs();
  });
  LB.on('nav', function (m) {
    state.nav = m.state;
    // Shown always, never, or only on the new-tab page: the host decides per window.
    $('bookmarks-bar').hidden = !m.state.bookmarksBar;
    renderNav();
  });
  LB.on('shellConfig', function (m) {
    document.documentElement.classList.toggle('dark', m.theme === 'dark');
    document.documentElement.classList.toggle('light', m.theme === 'light');
  });
  LB.on('focusAddressBar', function () { address.focus(); address.select(); });
  LB.on('blurAddressBar', function () { state.dirty = false; if (document.activeElement === address) address.blur(); else renderNav(); });
  // The address bar may be focused before the shell itself has the keyboard
  // focus: select again when it arrives, so typing replaces the text.
  window.addEventListener('focus', function () { if (document.activeElement === address) address.select(); });
  LB.on('bookmarksBar', function (m) { renderBar(m.items); });
  LB.on('downloadsChanged', function (m) {
    // Shown once something was downloaded in this session; badge while active.
    if (m.active > 0) $('downloads').hidden = false;
    $('downloads').classList.toggle('idle', m.active === 0);
    $('downloads-count').textContent = String(m.active);
    $('downloads').title = m.active ? m.active + ' download(s) in progress (Ctrl+J)' : 'Downloads (Ctrl+J)';
  });
  $('downloads').addEventListener('click', function () { LB.post('command', { command: { command: 'downloads' } }); });
  LB.on('showFind', function () {
    $('findbar').hidden = false;
    find.focus();
    find.select();
    if (find.value) runFind(false);
  });
  LB.on('findResult', function (m) {
    $('find-count').textContent = find.value ? (m.total > 0 ? m.active + '/' + m.total : '0/0') : '';
    $('findbar').classList.toggle('nomatch', !!find.value && m.total === 0);
  });
  LB.on('popupClosed', function () { setPopup(null); });

  // Keep keyboard shortcuts working while the shell has focus: the host sees
  // them through AcceleratorKeyPressed, so the shell does nothing extra here.
  document.addEventListener('contextmenu', function (e) {
    if (e.target !== address) e.preventDefault();
  });

  LB.post('ready');
})();
