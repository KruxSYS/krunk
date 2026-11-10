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
  const drafts = new Map();      // unfinished address-bar text per tab: id -> { text, url it was typed over }
  let ghost = null;              // a tab dragged from another window, shown in a gap: { id, index, node }
  let renderedOnce = false;
  let widthsFrozen = false;
  const CLOSE_MS = 160;
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const liveTabs = () => document.querySelectorAll('#tabs .tab:not(.closing):not(.ghost)');
  function tabById(id) { return state.tabs.find(function (t) { return t.id === id; }); }

  function newTabNode(id) {
    const node = el('div', {
      class: 'tab', role: 'tab', 'data-id': id,
      onmousedown: function (e) {
        if (e.button !== 0 || e.target.closest('.close, .audio')) return;
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
    // Only what changed is rebuilt (0.10.0): 0.9.0 rebuilt every tab's contents on every
    // change of any tab. The parts are kept on the node with the state they show.
    const p = node._parts || (node._parts = {
      title: el('span', { class: 'title' }),
      close: el('button', {
        class: 'close', title: 'Close tab (Ctrl+W)', 'aria-label': 'Close tab',
        onclick: function (e) { e.stopPropagation(); closeWithMouse(t.id); },
      }, [svgIcon('i-close')]),
    });
    const iconKey = t.loading ? 'loading' : (t.favicon || '');
    if (p.iconKey !== iconKey || !p.icon) {
      p.iconKey = iconKey;
      p.icon = t.loading
        ? el('span', { class: 'spinner', 'aria-label': 'Loading' })
        : t.favicon
          ? el('img', { class: 'favicon', src: t.favicon, alt: '' })
          : svgIcon('i-globe', 'favicon');
    }
    // A speaker while the page plays sound (crossed out when the tab is muted); a click
    // toggles the mute (0.6.0).
    const speakerKey = (t.audible || t.muted) ? (t.muted ? 'muted' : 'audible') : '';
    if (p.speakerKey !== speakerKey) {
      p.speakerKey = speakerKey;
      p.speaker = speakerKey ? el('button', {
        class: 'audio' + (t.muted ? ' muted' : ''), title: t.muted ? 'Unmute tab' : 'Mute tab', 'aria-label': t.muted ? 'Unmute tab' : 'Mute tab',
        onclick: function (e) { e.stopPropagation(); LB.post('tabMute', { id: t.id, muted: !t.muted }); },
        onmousedown: function (e) { e.stopPropagation(); },
      }, [svgIcon(t.muted ? 'i-audio-off' : 'i-audio')]) : null;
    }
    if (!!p.mark !== !!t.private) {
      p.mark = t.private ? el('span', { class: 'private-mark', title: 'Private tab' }, [svgIcon('i-mask')]) : null;
    }
    const title = t.title || 'New tab';
    if (p.title.textContent !== title) p.title.textContent = title;
    const want = [p.mark, p.icon, p.title, p.speaker, p.close].filter(Boolean);
    const have = node.childNodes;
    if (have.length !== want.length || want.some(function (n, i) { return have[i] !== n; })) {
      node.replaceChildren.apply(node, want);
    }
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

  let settling = false, renderPending = false; // a dropped tab is sliding into its slot
  function renderTabs() {
    if (settling) { renderPending = true; return; }
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
      if (!node && ghost && ghost.id === t.id) {
        // A tab dropped from another window takes the place of its ghost, already open.
        node = newTabNode(t.id);
        tabNodes.set(t.id, node);
        ghost.node.replaceWith(node);
        ghost = null;
        added = true;
      } else if (!node) {
        node = newTabNode(t.id);
        tabNodes.set(t.id, node);
        if (animate) node.classList.add('opening');
        added = true;
      }
      fillTabNode(node, t, index);
      // In state order; closing tabs stay where they were meanwhile.
      const slot = after ? after.nextSibling : box.firstChild;
      let next = slot;
      while (next && (next.classList.contains('closing') || next.classList.contains('ghost'))) next = next.nextSibling;
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
    // The active tab comes into view when it changes or a tab opens (not on every title
    // change: a scrolled strip stays where the user put it).
    const revealActive = added || state.active !== lastRevealed;
    lastRevealed = state.active;
    layoutTabs(revealActive);
  }
  let lastRevealed = null, revealPending = false;

  // Narrow tabs hide inactive close buttons; the active tab stays in view
  // when the row scrolls; the host gets the new hit-test map.
  const TAB_WIDTH = 256, TAB_GAP = 2;
  function layoutTabs(revealActive) {
    const box = $('tabs');
    // The row asks for every tab at full width and shrinks (evenly, through
    // the tabs' own flex-shrink) when the window is narrower.
    const n = box.children.length; // closing tabs too: the row shrinks as they do
    // With frozen widths the row is as wide as its tabs, so "+" follows the last one.
    box.style.flexBasis = widthsFrozen ? 'auto' : n ? (n * TAB_WIDTH + (n - 1) * TAB_GAP) + 'px' : '0px';
    const first = box.querySelector('.tab:not(.closing)');
    box.classList.toggle('narrow', !!first && first.getBoundingClientRect().width < 100);
    updateOverflow();
    // Opening and closing tabs still change the widths: the reveal waits for them to settle.
    if (revealActive) revealPending = true;
    if (revealPending && !box.querySelector('.tab.opening, .tab.closing')) { revealPending = false; revealActiveTab(); }
    reportHitMap();
  }

  // ---------- overflow (0.11.0) ----------
  // Past the minimum width the row scrolls: smoothly with the wheel and the ‹ › buttons, with
  // fading edges where more tabs are; "▾" lists every tab. All of it only while overflowing.
  const FADE = 28;
  function updateOverflow() {
    const box = $('tabs');
    const over = box.scrollWidth > box.clientWidth + 1;
    if (over !== document.documentElement.classList.contains('tabs-overflow')) {
      document.documentElement.classList.toggle('tabs-overflow', over);
      $('tabs-left').hidden = !over;
      $('tabs-right').hidden = !over;
      $('tablist').hidden = !over;
      if (!over && state.popup === 'tabList') { LB.post('hidePopup'); setPopup(null); }
    }
    const max = box.scrollWidth - box.clientWidth;
    const left = over && box.scrollLeft > 1, right = over && box.scrollLeft < max - 1;
    box.classList.toggle('fade-left', left);
    box.classList.toggle('fade-right', right);
    $('tabs-left').disabled = !left;
    $('tabs-right').disabled = !right;
  }
  let scrollGoal = null, scrollRaf = 0;
  function scrollTabsTo(x) {
    const box = $('tabs');
    const max = Math.max(0, box.scrollWidth - box.clientWidth);
    scrollGoal = Math.max(0, Math.min(max, x));
    if (reduceMotion.matches) { box.scrollLeft = scrollGoal; scrollGoal = null; return; }
    if (!scrollRaf) scrollRaf = requestAnimationFrame(stepScroll);
  }
  function scrollTabsBy(dx) {
    scrollTabsTo((scrollGoal === null ? $('tabs').scrollLeft : scrollGoal) + dx);
  }
  function stepScroll() {
    scrollRaf = 0;
    const box = $('tabs');
    if (scrollGoal === null) return;
    const d = scrollGoal - box.scrollLeft;
    const before = box.scrollLeft;
    if (Math.abs(d) < 1) { box.scrollLeft = scrollGoal; scrollGoal = null; return; }
    box.scrollLeft = before + (Math.abs(d) < 4 ? d : d * 0.3);
    if (box.scrollLeft === before) { scrollGoal = null; return; }
    scrollRaf = requestAnimationFrame(stepScroll);
  }
  // The active tab fully in view, clear of the fading edges.
  function revealActiveTab() {
    const box = $('tabs');
    const active = box.querySelector('.tab.active');
    if (!active || box.scrollWidth <= box.clientWidth + 1) return;
    const row = box.getBoundingClientRect(), r = active.getBoundingClientRect();
    // Measured at the current position (not where a running scroll is going).
    if (r.left < row.left + FADE) scrollTabsTo(box.scrollLeft + r.left - row.left - FADE);
    else if (r.right > row.right - FADE) scrollTabsTo(box.scrollLeft + r.right - row.right + FADE);
  }
  // ‹ ›: a click scrolls most of a row; held down, it keeps going.
  ['tabs-left', 'tabs-right'].forEach(function (id, i) {
    const dir = i ? 1 : -1;
    let hold = 0;
    const stop = function () { clearTimeout(hold); clearInterval(hold); hold = 0; };
    const btn = $(id);
    btn.addEventListener('pointerdown', function (e) {
      if (e.button !== 0) return;
      stop();
      scrollTabsBy(dir * Math.max(120, $('tabs').clientWidth * 0.8));
      hold = setTimeout(function () { hold = setInterval(function () { scrollTabsBy(dir * 160); }, 120); }, 400);
    });
    ['pointerup', 'pointerleave', 'pointercancel'].forEach(function (t) { btn.addEventListener(t, stop); });
    // The keyboard (and tests) click without a press.
    btn.addEventListener('click', function (e) { if (e.detail === 0) scrollTabsBy(dir * Math.max(120, $('tabs').clientWidth * 0.8)); });
  });
  $('tablist').addEventListener('pointerdown', function () { rememberOpen('tabList'); });
  $('tablist').addEventListener('click', function () {
    if (state.popup === 'tabList' || downWhileOpen === 'tabList') { downWhileOpen = null; LB.post('hidePopup'); setPopup(null); return; }
    const r = $('tablist').getBoundingClientRect();
    LB.post('showPopup', { popup: 'tabList', rect: { x: r.left, y: r.top, width: r.width, height: r.height } });
    setPopup('tabList');
  });
  // A middle press never starts the engine's autoscroll on the strip (0.11.0): on an
  // overflowing (scrollable) row it took the release, so middle-click did not close the tab.
  $('tabstrip').addEventListener('mousedown', function (e) { if (e.button === 1) e.preventDefault(); });

  // ---------- frameless window ----------
  // The host hit-tests the window: everything in the tab row that is not a
  // tab, the "+" button or a caption button drags the window.
  function rectOf(node) {
    const r = node.getBoundingClientRect();
    return { x: Math.max(0, r.left), y: Math.max(0, r.top), width: Math.max(0, r.width), height: Math.max(0, r.height) };
  }
  let lastMap = '';
  function reportHitMap() {
    // While a ghost holds a gap the host keeps the centres from before it opened (its drop
    // places would move with the gap).
    if (ghost) return;
    const box = $('tabs');
    const tabs = liveTabs();
    const interactive = [rectOf($('newtab'))];
    ['tabs-left', 'tabs-right', 'tablist'].forEach(function (id) { if (!$(id).hidden) interactive.push(rectOf($(id))); });
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
      downloads: $('downloads').hidden ? { x: 0, y: 0, width: 0, height: 0 } : rectOf($('downloads')),
    };
    const json = JSON.stringify(map);
    if (json === lastMap) return;
    lastMap = json;
    LB.post('hitMap', { map: map });
  }
  new ResizeObserver(function () { layoutTabs(true); }).observe($('tabstrip'));
  $('tabs').addEventListener('scroll', function () { updateOverflow(); reportHitMap(); });
  // Tabs slide while they animate: the layout (narrow mode, scrolling) and the host's hit map
  // follow when they settle.
  $('tabs').addEventListener('transitionend', function (e) {
    if (e.propertyName === 'flex-basis' && !e.target.classList.contains('closing')) layoutTabs();
    // The active tab grows to its minimum (narrow tabs): it comes into view at its new width.
    else if (e.propertyName === 'min-width' && e.target.classList.contains('active')) { revealActiveTab(); reportHitMap(); }
  });
  $('tabs').addEventListener('wheel', function (e) {
    const box = $('tabs');
    if (box.scrollWidth <= box.clientWidth) return;
    e.preventDefault();
    const d = Math.abs(e.deltaY) > Math.abs(e.deltaX) ? e.deltaY : e.deltaX;
    scrollTabsBy(e.deltaMode === 1 ? d * 40 : e.deltaMode === 2 ? d * box.clientWidth : d);
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
  const SLIDE_MS = 170;       // the other tabs slide aside, and the dropped tab settles, in this
  let drag = null;            // { id, node, pointerId, startX, startY, offsetX, moved, to, rects, from }
  function clearDropMarks() {
    document.querySelectorAll('.tab').forEach(function (n) {
      n.classList.remove('dragging', 'drop-before', 'drop-after');
    });
  }
  // Transforms of the live reorder (the dragged tab under the cursor, the others aside).
  function clearSlides() {
    instantly(function () {
      document.querySelectorAll('.tab').forEach(function (n) { n.style.transform = ''; n.classList.remove('settling'); });
    });
  }
  // The tabs' rects before the drag moved anything (transforms change getBoundingClientRect).
  function snapshotRects() {
    const rects = new Map();
    liveTabs().forEach(function (n) {
      const r = n.getBoundingClientRect();
      rects.set(Number(n.dataset.id), { left: r.left, width: r.width });
    });
    return rects;
  }
  // While the tab is dragged inside the strip: it follows the cursor, and every other tab
  // slides by the dragged tab's width to make room where it would land. `to` is the index
  // among the other tabs: a tab is passed once the dragged tab's leading edge (its right
  // edge for tabs that were to its right, its left edge for those to its left) crosses
  // that tab's original centre. Tabs can differ in width (frozen widths, a new tab).
  function slideTabs(node, dx) {
    const d = drag;
    const me = d.rects.get(d.id);
    const others = Array.prototype.filter.call(liveTabs(), function (n) { return Number(n.dataset.id) !== d.id; });
    let to = 0;
    const left = me.left + dx, right = left + me.width;
    others.forEach(function (n, j) {
      const r = d.rects.get(Number(n.dataset.id));
      if (!r) return;
      const c = r.left + r.width / 2;
      if (c < (j < d.from ? left : right)) to++;
    });
    const step = me.width + TAB_GAP;
    others.forEach(function (n, j) {
      const shift = (j >= d.from && j < to) ? -step : (j < d.from && j >= to) ? step : 0;
      n.style.transform = shift ? 'translateX(' + shift + 'px)' : '';
    });
    node.style.transform = 'translateX(' + dx + 'px)';
    d.to = to;
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
      if (e.button !== 0 || e.target.closest('.close, .audio')) return;
      if (settling) return;
      const r = node.getBoundingClientRect();
      drag = { id: id, pointerId: e.pointerId, startX: e.clientX, startY: e.clientY,
        offsetX: e.clientX - r.left, moved: false, to: null, rects: null, from: 0 };
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
      drag.rects = snapshotRects();
      drag.from = state.tabs.findIndex(function (t) { return t.id === drag.id; });
    }
    const node = dragNode();
    if (node) node.classList.add('dragging');
    const strip = $('tabstrip').getBoundingClientRect();
    // A window's only tab has nothing to reorder against: it drags the whole window at once
    // (0.11.2; it used to slide in the strip until the pointer left it).
    const alone = state.tabs.length === 1;
    if (alone || e.clientY < strip.top - DETACH_DISTANCE || e.clientY > strip.bottom + DETACH_DISTANCE) {
      // Out of the strip: the host follows the mouse from here.
      const d = drag;
      drag = null;
      clearSlides();
      clearDropMarks();
      try { $('tabs').releasePointerCapture(d.pointerId); } catch (err) { /* already released */ }
      // Where a row that does not scroll starts: the tab's place in its new window.
      const strip = $('tabstrip');
      const firstX = strip.getBoundingClientRect().left + (parseFloat(getComputedStyle(strip).paddingLeft) || 0);
      LB.post('tabDragOut', { id: d.id, offsetX: d.offsetX, offsetY: d.startY, firstX: firstX });
      return;
    }
    if (!node) return;
    // The tab follows the cursor, kept within the tabs' own extent (the row box can be
    // narrower than its scrolled content).
    const me = drag.rects.get(drag.id);
    let first = Infinity, last = -Infinity;
    drag.rects.forEach(function (r) { first = Math.min(first, r.left); last = Math.max(last, r.left + r.width); });
    const dx = Math.max(first - me.left, Math.min(last - me.left - me.width, e.clientX - drag.startX));
    slideTabs(node, dx);
  });
  function finishDrag(e, commit) {
    if (!drag || e.pointerId !== drag.pointerId) return;
    const d = drag;
    drag = null;
    clearDropMarks();
    const node = tabNodes.get(d.id);
    if (!commit || !d.moved || d.to === null || !node) { clearSlides(); return; }
    const from = d.from;
    if (from < 0 || from === d.to) { clearSlides(); return; }
    // The host and the state get the new order now; the strip shows it once the dropped
    // tab has slid into its slot (its final place in the original geometry).
    const moved = state.tabs.splice(from, 1)[0];
    state.tabs.splice(d.to, 0, moved);
    LB.post('moveTab', { id: d.id, index: d.to });
    const ids = Array.from(d.rects.keys());
    const others = ids.filter(function (id) { return id !== d.id; });
    let target = 0;
    if (d.to > from) {
      target = others.slice(from, d.to).reduce(function (a, id) { return a + d.rects.get(id).width + TAB_GAP; }, 0);
    } else {
      target = -others.slice(d.to, from).reduce(function (a, id) { return a + d.rects.get(id).width + TAB_GAP; }, 0);
    }
    const finish = function () {
      settling = false;
      clearSlides();
      renderPending = false;
      renderTabs();
    };
    if (reduceMotion.matches) { finish(); return; }
    settling = true;
    node.classList.remove('dragging');
    node.classList.add('settling');
    let done = false;
    const once = function () { if (!done) { done = true; finish(); } };
    node.addEventListener('transitionend', function (ev) { if (ev.propertyName === 'transform') once(); }, { once: true });
    requestAnimationFrame(function () { node.style.transform = 'translateX(' + target + 'px)'; });
    setTimeout(once, SLIDE_MS + 120);
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
    if (t.more) t.more.classList.add('drop-into');
    else if (t.into !== undefined) t.node.classList.add('drop-into');
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

  // A tab dragged from another window is over this strip (0.11.1): the strip opens a gap where
  // it would land and shows the tab there (a "ghost"); the gap moves with the cursor, sliding
  // like a tab that opens and one that closes. Dropped, the tab takes the ghost's place.
  function ghostNode(g) {
    const icon = g.favicon ? el('img', { class: 'favicon', src: g.favicon, alt: '' }) : svgIcon('i-globe', 'favicon');
    return el('div', { class: 'tab ghost' + (g.private ? ' private' : ''), 'aria-hidden': 'true' },
      [icon, el('span', { class: 'title' }, [g.title || 'New tab'])]);
  }
  function dropGhost(animate) {
    if (!ghost) return;
    const node = ghost.node;
    ghost = null;
    if (animate && !reduceMotion.matches) closeOut(node);
    else { node.remove(); layoutTabs(); }
  }
  LB.on('tabDropHint', function (m) {
    clearDropMarks();
    if (m.index === null || m.index === undefined || !m.ghost) { dropGhost(true); reportHitMap(); return; }
    if (ghost && ghost.id === m.ghost.id && ghost.index === m.index) return;
    const animate = !reduceMotion.matches;
    // The new gap opens as the old one closes.
    if (ghost) {
      const old = ghost.node;
      ghost = null;
      if (animate) closeOut(old); else old.remove();
    }
    const node = ghostNode(m.ghost);
    const tabs = liveTabs();
    const before = m.index < tabs.length ? tabs[m.index] : (tabs.length ? tabs[tabs.length - 1].nextSibling : null);
    $('tabs').insertBefore(node, before);
    ghost = { id: m.ghost.id, index: m.index, node: node };
    if (animate) {
      node.classList.add('opening');
      requestAnimationFrame(function () {
        requestAnimationFrame(function () { node.classList.remove('opening'); });
      });
    }
    layoutTabs();
    if (node.scrollIntoView) node.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  });

  // ---------- toolbar ----------
  function renderNav() {
    const n = state.nav;
    if (!n) return;
    if (state.navTab !== undefined && state.navTab !== n.tabId) {
      closeFind(false);
      // Ctrl+Tab while typing: the text stays with the tab it was typed in (0.11.6).
      if (state.editing && state.dirty) { saveDraft(); state.dirty = false; }
    }
    state.navTab = n.tabId;
    state.navUrl = n.url;
    const zoomed = Math.abs((n.zoom || 1) - 1) > 0.001;
    $('zoom').hidden = !zoomed;
    $('key').hidden = !n.loginForm;
    // 0.6.0: the page has videos; one of them is in the floating window.
    $('pip').hidden = !(n.videos > 0);
    $('pip').classList.toggle('active', !!n.pip);
    $('pip').title = n.pip ? 'Leave Picture-in-Picture (Alt+P)' : n.videos > 1 ? 'Picture-in-Picture: choose a video (Alt+P)' : 'Picture-in-Picture (Alt+P)';
    // 0.6.0: camera / microphone in use.
    $('media-chip').hidden = !(n.camera || n.microphone);
    $('media-cam').hidden = !n.camera;
    $('media-mic').hidden = !n.microphone;
    $('media-chip').title = n.camera && n.microphone ? 'This page uses your camera and microphone' : n.camera ? 'This page uses your camera' : 'This page uses your microphone';
    $('zoom').textContent = Math.round((n.zoom || 1) * 100) + '%';
    $('back').disabled = !n.canGoBack;
    $('forward').disabled = !n.canGoForward;
    $('reload-icon').setAttribute('href', n.loading ? '#i-stop' : '#i-reload');
    $('reload').title = n.loading ? 'Stop (Esc)' : 'Reload (Ctrl+R)';
    // The user's own text stays while they type; a focused but untouched field
    // follows the page (a bookmark opened while the caret was in the field).
    // 0.11.6: a tab's unfinished text comes back with the tab, until its page changes.
    if (!(state.editing && state.dirty)) {
      const d = drafts.get(n.tabId);
      if (d && d.url !== n.url) drafts.delete(n.tabId);
      $('address').value = d && d.url === n.url ? d.text : n.url;
    }
    renderAddressDisplay();
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
  // Right-click: the history menu under the button (0.8.0). A disabled button shows none.
  ['back', 'forward'].forEach(function (id) {
    $(id).addEventListener('contextmenu', function (e) {
      e.preventDefault();
      if ($(id).disabled) return;
      const r = $(id).getBoundingClientRect();
      LB.post('showNavMenu', { forward: id === 'forward', rect: { x: Math.max(0, r.left), y: r.bottom + 2, width: 340, height: 300 } });
      setPopup('navMenu');
    });
  });
  $('reload').addEventListener('click', function () {
    LB.post(state.nav && state.nav.loading ? 'stop' : 'reload');
  });
  $('newtab').addEventListener('click', function () { LB.post('newTab', {}); });
  $('star').addEventListener('click', function () { LB.post('toggleBookmark'); });

  // ---------- address bar ----------
  const address = $('address');
  // The unfocused bar shows the address formatted (0.7.0, R5): the host in full colour, the
  // scheme and the rest muted. It is the field's own text in the same place (the field's text is
  // only made transparent), so a press puts the caret exactly where the pointer is.
  function renderAddressDisplay() {
    const box = $('address-display');
    const m = document.activeElement !== address && /^(https?:\/\/)([^\/?#]*)(.*)$/.exec(address.value);
    box.textContent = '';
    address.classList.toggle('formatted', !!m);
    box.hidden = !m;
    if (m) box.append(el('span', { text: m[1] }), el('span', { class: 'host', text: m[2] }), el('span', { text: m[3] }));
  }
  // Suggestions while typing (0.5.1): the host shows them in a popup under the bar that
  // never takes the focus; the shell keeps the rows and the keyboard selection.
  // `inline`: the completion shown selected after the caret (0.6.0); `noInline`: the last
  // edit was a deletion, so the completion is not put back for that keystroke.
  const suggest = { text: '', entries: [], selected: null, timer: null, open: false, inline: null, noInline: false, tail: false };
  function suggestRect() {
    const r = address.getBoundingClientRect();
    return { x: r.left, y: r.bottom + 4, width: r.width, height: 0 };
  }
  // The text the user typed: the field's value without a selected inline completion
  // (`tail`: the selection up to the end is a completion the bar put there).
  // Unfinished address-bar text per tab (0.11.6), in `drafts`.
  function saveDraft() {
    if (state.navTab === undefined) return;
    // The address of the tab it was typed in (`state.nav` may already be the next tab's).
    const url = state.navUrl || '';
    const text = typedText();
    if (text.trim() && text !== url) drafts.set(state.navTab, { text: text, url: url });
    else drafts.delete(state.navTab);
  }
  function typedText() {
    const v = address.value;
    if (suggest.tail && address.selectionEnd === v.length && address.selectionStart < v.length) return v.slice(0, address.selectionStart);
    return v;
  }
  function askSuggestions() {
    clearTimeout(suggest.timer);
    suggest.timer = setTimeout(function () {
      const text = typedText().trim();
      suggest.text = text;
      suggest.selected = null;
      if (!text) { closeSuggestions(); return; }
      LB.post('addressSuggest', { text: text, rect: suggestRect() });
    }, 60);
  }
  function closeSuggestions() {
    clearTimeout(suggest.timer);
    suggest.entries = [];
    suggest.selected = null;
    if (suggest.open) { suggest.open = false; LB.post('hidePopup'); }
  }
  function selectSuggestion(delta) {
    const n = suggest.entries.length;
    if (!n) return false;
    let i = suggest.selected === null ? (delta > 0 ? 0 : n - 1) : suggest.selected + delta;
    if (i < 0 || i >= n) i = null;
    suggest.selected = i;
    suggest.inline = null;
    suggest.tail = false;
    address.value = i === null ? suggest.text : suggest.entries[i].fill;
    address.setSelectionRange(address.value.length, address.value.length);
    LB.post('addressSuggestSelect', { selected: i });
    return true;
  }
  LB.on('addressSuggestions', function (e) {
    // Only for what the bar asked last (its value may already carry an inline completion).
    if (document.activeElement !== address || e.text !== suggest.text) return;
    const typed = typedText();
    const predicted = typed !== address.value;
    suggest.entries = e.entries || [];
    suggest.selected = null;
    suggest.inline = null;
    suggest.open = suggest.entries.length > 0;
    // Inline autocomplete: the first matching address is completed after the caret and
    // selected, so typing on replaces it and Enter goes there; never after a deletion.
    // A completion the bar already shows (kept while typing, 0.7.2) is left as it is, so
    // the field does not change at all when the answer arrives.
    const atEnd = address.selectionEnd === address.value.length && (predicted || address.selectionStart === address.value.length);
    if (e.inline && !suggest.noInline && atEnd && typed.trim() === suggest.text && e.inline.text.length > suggest.text.length) {
      const full = typed + e.inline.text.slice(typed.length);
      if (address.value !== full) {
        address.value = full;
        address.setSelectionRange(typed.length, full.length);
      }
      suggest.inline = e.inline;
      suggest.tail = true;
      suggest.selected = e.inline.index;
      LB.post('addressSuggestSelect', { selected: e.inline.index });
    } else if (predicted) {
      // The kept completion is no longer offered: back to what was typed.
      address.value = typed;
      address.setSelectionRange(typed.length, typed.length);
      suggest.tail = false;
    }
  });
  // 0.15.0: the search engine's suggestions arrived: the same rows with more under them, so
  // the keyboard selection and the inline completion stay as they are.
  LB.on('addressSearchSuggestions', function (e) {
    if (document.activeElement !== address || e.text !== suggest.text || !suggest.open) return;
    suggest.entries = e.entries || [];
    if (suggest.selected !== null && suggest.selected >= suggest.entries.length) suggest.selected = null;
  });
  // Focus by keyboard (Ctrl+L, a new tab) selects the whole address. A mouse press lets the field
  // do what text fields do (0.7.0, R4): drag selects, a double click a word, a triple click all;
  // a plain click into the unfocused bar selects all on release, as in Chrome.
  const press = { pointer: false, wasFocused: false, selectTimer: null };
  address.addEventListener('mousedown', function (e) {
    if (e.button !== 0) return;
    press.pointer = true;
    press.wasFocused = document.activeElement === address;
    // A press inside a selection would drag the text; collapse it so the press starts a new one.
    if (press.wasFocused && e.detail === 1 && address.selectionStart !== address.selectionEnd) {
      address.setSelectionRange(address.selectionStart, address.selectionStart);
    }
  });
  address.addEventListener('focus', function () {
    // A tab's unfinished text (0.11.6) is still the user's: page updates leave it alone.
    state.editing = true; state.dirty = drafts.has(state.navTab);
    suggest.inline = null; suggest.tail = false;
    renderAddressDisplay();
    if (!press.pointer) address.select();
  });
  // (After the click: the engine places the caret only once the click is handled; `detail`
  // counts the clicks.)
  address.addEventListener('click', function (e) {
    const fresh = press.pointer && !press.wasFocused;
    press.pointer = false;
    if (!fresh || e.detail !== 1) return;
    press.selectTimer = setTimeout(function () {
      press.selectTimer = null;
      if (document.activeElement === address && !state.dirty && address.selectionStart === address.selectionEnd) address.select();
    }, 0);
  });
  // A key pressed before that (or a second press) wins over the select-all.
  ['keydown', 'mousedown'].forEach(function (t) {
    address.addEventListener(t, function () { if (press.selectTimer) { clearTimeout(press.selectTimer); press.selectTimer = null; } }, true);
  });
  // The address is never dragged out of the field: a press inside a selection starts a new one.
  address.addEventListener('dragstart', function (e) { e.preventDefault(); });
  address.addEventListener('blur', function () {
    if (state.dirty) saveDraft();
    state.editing = false; state.dirty = false; press.pointer = false;
    // The selection goes with the focus (R5): the unfocused bar shows the formatted address.
    address.setSelectionRange(0, 0);
    address.scrollLeft = 0;
    // A press on a suggestion takes the focus from the bar: the list stays a moment, so the
    // press reaches it (0.11.4).
    clearTimeout(suggest.blurTimer);
    suggest.blurTimer = setTimeout(function () { if (document.activeElement !== address) closeSuggestions(); }, 150);
    renderNav();
  });
  // A press anywhere else in the shell (toolbar, tabs, bookmarks bar) takes the focus from the bar.
  document.addEventListener('pointerdown', function (e) {
    if (document.activeElement === address && !$('omnibox').contains(e.target)) address.blur();
  }, true);
  address.addEventListener('input', function (e) {
    state.dirty = true;
    suggest.noInline = !!(e.inputType && e.inputType.indexOf('delete') === 0);
    // Typing the next letters of the inline completion keeps it in place in the same frame
    // (0.7.2, R3): the bar never shows the typed text alone while the host is asked again.
    const v = address.value;
    const full = suggest.inline && suggest.inline.text;
    if (full && e.inputType === 'insertText' && address.selectionStart === v.length && full.length > v.length
        && full.slice(0, v.length).toLowerCase() === v.toLowerCase()) {
      address.value = v + full.slice(v.length);
      address.setSelectionRange(v.length, address.value.length);
      suggest.tail = true;
    } else {
      suggest.inline = null;
      suggest.tail = false;
    }
    askSuggestions();
  });
  // Copying the whole, unedited address copies the page's full URL (never a shortened display).
  address.addEventListener('copy', function (e) {
    if (state.dirty || !state.nav || !state.nav.url) return;
    if (address.selectionStart === 0 && address.selectionEnd === address.value.length && e.clipboardData) {
      e.clipboardData.setData('text/plain', state.nav.url);
      e.preventDefault();
    }
  });
  // The context menu (0.6.0): the host shows it without taking the focus, so the bar keeps
  // its selection; the host answers with addressEdit for the edits the shell must make.
  let addressMenuOpen = false;
  address.addEventListener('contextmenu', function (e) {
    e.preventDefault();
    if (document.activeElement !== address) { address.focus(); address.select(); }
    const selection = address.value.substring(address.selectionStart, address.selectionEnd);
    closeSuggestions();
    addressMenuOpen = true;
    LB.post('showAddressMenu', { x: e.clientX, y: e.clientY, selection: selection, hasText: address.value.length > 0, dirty: state.dirty });
  });
  document.addEventListener('mousedown', function () { if (addressMenuOpen) { addressMenuOpen = false; LB.post('hidePopup'); } }, true);
  LB.on('addressEdit', function (m) {
    addressMenuOpen = false;
    if (document.activeElement !== address) address.focus();
    const ed = m.edit;
    const kind = typeof ed === 'string' ? ed : Object.keys(ed)[0];
    if (kind === 'undo') { document.execCommand('undo'); }
    else if (kind === 'deleteSelection') { address.setRangeText('', address.selectionStart, address.selectionEnd, 'end'); }
    else if (kind === 'insert') { address.setRangeText(ed.insert.text, address.selectionStart, address.selectionEnd, 'end'); }
    else if (kind === 'selectAll') { address.select(); return; }
    state.dirty = true;
    suggest.noInline = kind === 'deleteSelection';
    askSuggestions();
  });
  address.addEventListener('keydown', function (e) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (selectSuggestion(e.key === 'ArrowDown' ? 1 : -1)) e.preventDefault();
      return;
    }
    if (e.key === 'Escape') {
      if (addressMenuOpen) { e.preventDefault(); addressMenuOpen = false; LB.post('hidePopup'); return; }
      if (suggest.open) { e.preventDefault(); address.value = suggest.text; closeSuggestions(); return; }
      address.value = state.nav ? state.nav.url : '';
      state.dirty = false;
      drafts.delete(state.navTab);
      address.blur();
    }
  });
  $('omnibox').addEventListener('submit', function (e) {
    e.preventDefault();
    const chosen = suggest.selected !== null ? suggest.entries[suggest.selected] : null;
    const input = address.value.trim();
    if (!chosen && !input) return;
    closeSuggestions();
    drafts.delete(state.navTab);
    if (chosen) LB.post('addressSuggestPick', { url: chosen.url });
    else LB.post('navigate', { input: input });
    state.editing = false;
    state.dirty = false;
    address.blur();
  });

  // ---------- popups (menu, shield) ----------
  // 0.6.0: the enabled extensions' toolbar buttons (icon from the extension's origin, a
  // badge, the title). A click sends the button's rect: the host opens the popup under it.
  LB.on('extActions', function (m) {
    const box = $('ext-actions');
    box.textContent = '';
    (m.actions || []).forEach(function (a) {
      const badge = a.badge ? el('span', { class: 'badge ext-badge', text: a.badge }) : null;
      if (badge && a.badgeColor) badge.style.background = a.badgeColor;   // CSSOM: the shell's CSP has no inline styles
      const b = el('button', { class: 'tb ext-action', 'data-ext': a.id, title: a.title, 'aria-label': a.title }, [
        a.icon ? el('img', { src: a.icon, alt: '' }) : el('span', { class: 'ext-letter', text: (a.title || '?').charAt(0).toUpperCase() }),
        badge,
      ]);
      b.addEventListener('click', function () {
        const r = b.getBoundingClientRect();
        LB.post('extAction', { id: a.id, rect: { x: r.left, y: r.top, width: r.width, height: r.height } });
      });
      box.appendChild(b);
    });
  });

  const POPUP_SIZES = { menu: [280, 670], shield: [300, 226], siteInfo: [360, 260], downloads: [380, 420], pip: [340, 300], vpn: [320, 330] };
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
    $('downloads').classList.toggle('on', kind === 'downloads');
    $('pip').classList.toggle('on', kind === 'pip');
    $('shield').classList.toggle('on', kind === 'shield');
    // The VPN button's `.on` is its state (connected); open is `.open`.
    $('vpn').classList.toggle('open', kind === 'vpn');
    $('vpn').setAttribute('aria-expanded', kind === 'vpn' ? 'true' : 'false');
    $('site-icon').classList.toggle('on', kind === 'siteInfo');
    $('tablist').classList.toggle('on', kind === 'tabList');
    document.documentElement.classList.toggle('tab-menu', kind === 'tabMenu');
    document.querySelectorAll('.bm[data-folder]').forEach(function (b) {
      b.classList.toggle('on', kind === 'folder:' + b.dataset.folder);
    });
    if ($('bm-more')) $('bm-more').classList.toggle('on', kind === 'folder:' + BAR_ROOT);
  }

  // ---------- bookmarks bar ----------
  const BAR_ROOT = 1; // the "Bookmarks bar" folder
  // A middle press never starts the engine's autoscroll on the bar (0.11.0): it would take the
  // release that opens the bookmark.
  $('bookmarks-bar').addEventListener('mousedown', function (e) { if (e.button === 1) e.preventDefault(); });
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
        // Middle-click: a new tab behind this one (0.11.0; it used to come to the front).
        btn.addEventListener('auxclick', function (e) { if (e.button === 1) { e.preventDefault(); LB.post('openBookmarkBackground', { id: b.id }); } });
        bar.appendChild(btn);
      }
    });
    // 0.7.2 (R10): what does not fit goes into "»" at the right end, a dropdown of the bar's
    // folder from the first hidden item on.
    const more = el('button', { class: 'bm-more', id: 'bm-more', title: 'More bookmarks', 'aria-label': 'More bookmarks', hidden: '' }, [
      el('span', { text: '»' }),
    ]);
    more.addEventListener('pointerdown', function () { rememberOpen('folder:' + BAR_ROOT); });
    more.addEventListener('click', function () {
      const key = 'folder:' + BAR_ROOT;
      if (state.popup === key || downWhileOpen === key) { downWhileOpen = null; LB.post('hidePopup'); setPopup(null); return; }
      const r = more.getBoundingClientRect();
      LB.post('showBookmarkFolder', { id: BAR_ROOT, rect: { x: r.left, y: r.top, width: Math.max(1, r.width), height: Math.max(1, r.height) }, from: state.barOverflowFrom });
      setPopup(key);
    });
    bar.appendChild(more);
    layoutBar();
  }
  // Shows the items that fit completely and "»" for the rest (on every render and resize).
  function layoutBar() {
    const bar = $('bookmarks-bar');
    const more = $('bm-more');
    if (!more || bar.hidden) return;
    const items = Array.prototype.slice.call(bar.querySelectorAll('.bm'));
    items.forEach(function (n) { n.classList.remove('overflowed'); });
    more.hidden = true;
    const right = function () { return bar.getBoundingClientRect().right - parseFloat(getComputedStyle(bar).paddingRight || '0'); };
    let first = items.findIndex(function (n) { return n.getBoundingClientRect().right > right() + 0.5; });
    if (first >= 0) {
      more.hidden = false;
      const end = right() - more.getBoundingClientRect().width - 4;
      first = items.findIndex(function (n) { return n.getBoundingClientRect().right > end + 0.5; });
      for (let i = first; i < items.length; i++) items[i].classList.add('overflowed');
    }
    const from = first >= 0 ? first : items.length;
    if (state.barOverflowFrom !== from && state.popup === 'folder:' + BAR_ROOT) { LB.post('hidePopup'); setPopup(null); }
    state.barOverflowFrom = from;
  }
  if (window.ResizeObserver) new ResizeObserver(function () { layoutBar(); }).observe($('bookmarks-bar'));

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
  function barItems() { return Array.prototype.slice.call($('bookmarks-bar').querySelectorAll('.bm:not(.overflowed)')); }
  function clearBarMarks() {
    barItems().forEach(function (n) { n.classList.remove('drop-before', 'drop-after', 'drop-into', 'dragging'); });
    if ($('bm-more')) $('bm-more').classList.remove('drop-into');
  }
  // Where a drop at x goes: { into: folderId } or { index } among the other items; on "»" it
  // goes to the end of the bar, into the overflow (0.7.2).
  function barTarget(x) {
    const more = $('bm-more');
    if (more && !more.hidden && x >= more.getBoundingClientRect().left) return { index: 1000000, others: [], more: more };
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
    const t = barTarget(e.clientX);
    barDrag.target = t;
    markBarTarget(t);
    barDrag.item.classList.add('dragging');
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

  // Dragging the page by the site icon onto the bar bookmarks it where it is dropped; a chip
  // follows the cursor. (Up to 0.6.0 the unfocused field dragged it too; since 0.7.0 a drag in
  // the field selects text.) The host takes the title and address from the tab itself.
  const urlDrag = { pointerId: null, startX: 0, startY: 0, moved: false, chip: null, target: null, suppressClick: false };
  function urlDragStart(e) {
    if (e.button !== 0 || !state.nav || !state.nav.url || state.nav.url.startsWith('krunk://')) return;
    urlDrag.pointerId = e.pointerId; urlDrag.startX = e.clientX; urlDrag.startY = e.clientY;
    urlDrag.moved = false; urlDrag.target = null;
  }
  $('site-icon').addEventListener('pointerdown', function (e) { rememberOpen('siteInfo'); urlDragStart(e); });
  // A click (not a drag) on the site icon: the lock popup with the connection and certificate.
  $('site-icon').addEventListener('click', function () {
    if (urlDrag.suppressClick || !state.nav || !state.nav.url) return;
    const r = $('site-icon').getBoundingClientRect();
    const size = POPUP_SIZES.siteInfo;
    if (state.popup === 'siteInfo' || downWhileOpen === 'siteInfo') { downWhileOpen = null; LB.post('hidePopup'); setPopup(null); return; }
    LB.post('showPopup', { popup: 'siteInfo', rect: { x: Math.max(0, r.left - 8), y: r.bottom + 6, width: size[0], height: size[1] } });
    setPopup('siteInfo');
  });
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
    const moved = urlDrag.moved, t = urlDrag.target;
    urlDrag.pointerId = null;
    if (urlDrag.chip) { urlDrag.chip.remove(); urlDrag.chip = null; }
    clearBarMarks();
    if (!moved) return;
    // The release that ends a drag is not a click on the icon.
    urlDrag.suppressClick = true;
    setTimeout(function () { urlDrag.suppressClick = false; }, 0);
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
  // Several videos (and none in the floating window yet): the chooser under the button;
  // otherwise the command toggles the one video.
  $('pip').addEventListener('pointerdown', function () { rememberOpen('pip'); });
  $('pip').addEventListener('click', function () {
    const n = state.nav || {};
    if (n.videos > 1 && !n.pip) togglePopup('pip', $('pip'));
    else LB.post('command', { command: { command: 'pictureInPicture' } });
  });
  $('zoom').addEventListener('click', function () { LB.post('zoom', { action: 'reset' }); });
  $('menu').addEventListener('pointerdown', function () { rememberOpen('menu'); });
  $('menu').addEventListener('click', function () { togglePopup('menu', $('menu')); });
  // 0.14.0: the VPN (shown only when turned on in Settings → Privacy & security → VPN).
  $('vpn').addEventListener('pointerdown', function () { rememberOpen('vpn'); });
  $('vpn').addEventListener('click', function () { togglePopup('vpn', $('vpn')); });
  LB.on('vpn', function (m) {
    const b = $('vpn');
    const st = (m.status && m.status.state) || 'stopped';
    const look = st === 'connected' ? 'on' : (st === 'connecting' || st === 'reconnecting') ? 'connecting' : st === 'failed' ? 'error' : 'off';
    ['off', 'connecting', 'on', 'error'].forEach(function (c) { b.classList.toggle(c, c === look); });
    const p = (m.profiles || []).find(function (x) { return x.id === m.active; });
    const text = { on: 'VPN connected', connecting: st === 'reconnecting' ? 'VPN reconnecting' : 'VPN connecting', error: 'VPN error: connections are stopped', off: 'VPN off' }[look] +
      (p && look !== 'off' ? ' (' + p.name + ')' : '');
    b.title = text;
    b.setAttribute('aria-label', text);
    const hide = !m.showButton;
    if (b.hidden !== hide) {
      b.hidden = hide;
      if (hide && state.popup === 'vpn') { LB.post('hidePopup'); setPopup(null); }
      reportHitMap();
    }
  });
  $('shield').addEventListener('pointerdown', function () { rememberOpen('shield'); });
  $('shield').addEventListener('click', function () { togglePopup('shield', $('shield')); });
  // 0.12.0: the AI agent's panel (the button shows only while AI is on) and a running macro.
  $('agent').addEventListener('click', function () { LB.post('toggleAgent'); });
  $('macro-stop').addEventListener('click', function () { LB.post('stopMacros'); });
  LB.on('agentButton', function (m) {
    const b = $('agent');
    b.hidden = !m.visible;
    b.classList.toggle('on', !!m.open);
    b.setAttribute('aria-pressed', m.open ? 'true' : 'false');
    reportHitMap();
  });
  // A message from a userscript (GM_notification) or a macro's toast(): shown for 4 s.
  let toastTimer = 0;
  LB.on('toast', function (m) {
    const t = $('shell-toast');
    t.textContent = String(m.message || '').slice(0, 200);
    t.hidden = !t.textContent;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.hidden = true; }, 4000);
  });
  LB.on('macroRunning', function (m) {
    $('macro-run').hidden = !m.id;
    $('macro-run-name').textContent = m.name || '';
    $('macro-run').title = m.name ? 'Macro running: ' + m.name : '';
    reportHitMap();
  });

  // ---------- host events ----------
  LB.on('tabs', function (m) {
    state.tabs = m.tabs;
    state.active = m.active;
    const live = new Set(m.tabs.map(function (t) { return t.id; }));
    drafts.forEach(function (_, id) { if (!live.has(id)) drafts.delete(id); });
    renderTabs();
  });
  // One tab changed (title, address, icon, loading, sound): only its node is patched, and the
  // strip's layout stays as it is (0.10.0).
  LB.on('tabUpdate', function (m) {
    const i = state.tabs.findIndex(function (t) { return t.id === m.tab.id; });
    if (i < 0) return;
    state.tabs[i] = m.tab;
    const node = tabNodes.get(m.tab.id);
    if (node && !settling) fillTabNode(node, m.tab, i);
    else renderTabs();
  });
  LB.on('nav', function (m) {
    state.nav = m.state;
    // Shown always, never, or only on the new-tab page: the host decides per window.
    $('bookmarks-bar').hidden = !m.state.bookmarksBar;
    renderNav();
  });
  // Appearance (0.6.0): colours as CSS variables, density/shape as classes, hidden items.
  function applyAppearance(a, colors) {
    if (!a) return;
    const root = document.documentElement;
    const set = (name, v) => { if (v) root.style.setProperty(name, v); else root.style.removeProperty(name); };
    set('--accent', a.accent);
    set('--toolbar-bg', a.toolbarColor);
    set('--strip-bg', a.stripColor);
    set('--bg', a.backgroundColor);
    // Readable text on the custom colours (0.7.0, R3; computed by the host, WCAG AA).
    const c = colors || {};
    set('--accent-fg', c.accentFg);
    set('--toolbar-fg', c.toolbarFg);
    set('--strip-fg', c.stripFg);
    set('--bg-fg', c.backgroundFg);
    root.classList.toggle('custom-strip', !!(c.stripFg || c.backgroundFg));
    // (common.css sets the size on html and body: both get the override.)
    const fs = a.uiFontPx && a.uiFontPx !== 13 ? a.uiFontPx + 'px' : '';
    root.style.fontSize = fs;
    document.body.style.fontSize = fs;
    root.classList.toggle('compact', a.density === 'compact');
    root.classList.toggle('square-tabs', a.roundedTabs === false);
    state.showDownloads = a.showDownloadsButton !== false;
    $('downloads').classList.toggle('off', !state.showDownloads);
    $('zoom').classList.toggle('off', a.showZoomChip === false);
    state.appearance = a;
    reportHitMap();
  }
  LB.on('shellConfig', function (m) {
    document.documentElement.classList.toggle('dark', m.theme === 'dark');
    document.documentElement.classList.toggle('light', m.theme === 'light');
    // The placeholder names the default search engine (it changes with the setting).
    if (m.searchName) address.placeholder = 'Search with ' + m.searchName + ' or enter address';
    applyAppearance(m.appearance, m.colors);
  });
  // A request while the user is typing (the new tab page finished loading, the window got the
  // focus back) leaves their text and caret alone (0.7.2, R3).
  LB.on('focusAddressBar', function () {
    if (document.activeElement === address && state.dirty) return;
    address.focus(); address.select();
  });
  LB.on('addressNavigated', function () {
    drafts.delete(state.navTab);
    state.dirty = false;
  });
  LB.on('blurAddressBar', function () {
    // The page took the focus (a click into it, or another tab): unfinished text stays with
    // its tab (0.11.6).
    if (state.dirty) saveDraft();
    state.dirty = false;
    if (document.activeElement === address) address.blur();
    else { address.setSelectionRange(0, 0); renderNav(); }
  });
  // The address bar may be focused before the shell itself has the keyboard
  // focus: select again when it arrives, so typing replaces the text.
  window.addEventListener('focus', function () { if (document.activeElement === address && !state.dirty) address.select(); });
  LB.on('bookmarksBar', function (m) { renderBar(m.items); });
  // 0.7.2 (R13): a newer Krunk is offered: a dot on ⋮ (the menu's first item leads to it).
  LB.on('updateAvailable', function (m) {
    const b = $('menu');
    b.classList.toggle('has-update', !!m.version);
    b.title = m.version ? 'Menu – update available: Krunk ' + m.version : 'Menu';
    b.setAttribute('aria-label', b.title);
  });
  LB.on('downloadsChanged', function (m) {
    // Shown once something was downloaded in this session; badge while active.
    if (m.active > 0) $('downloads').hidden = false;
    $('downloads').classList.toggle('idle', m.active === 0);
    $('downloads-count').textContent = String(m.active);
    $('downloads').title = m.active ? m.active + ' download(s) in progress (Ctrl+J)' : 'Downloads (Ctrl+J)';
  });
  // 0.6.0: the button opens its popup (Ctrl+J still opens the page); a new download pulses it.
  $('downloads').addEventListener('pointerdown', function () { rememberOpen('downloads'); });
  $('downloads').addEventListener('click', function () { togglePopup('downloads', $('downloads')); });
  LB.on('downloadStarted', function () {
    const b = $('downloads');
    b.hidden = false;
    b.classList.remove('pulse');
    void b.offsetWidth;
    b.classList.add('pulse');
    setTimeout(function () { b.classList.remove('pulse'); }, 900);
    reportHitMap();
  });
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
  LB.on('popupClosed', function () { setPopup(null); addressMenuOpen = false; suggest.open = false; suggest.entries = []; suggest.selected = null; suggest.inline = null; });

  // Keep keyboard shortcuts working while the shell has focus: the host sees
  // them through AcceleratorKeyPressed, so the shell does nothing extra here.
  document.addEventListener('contextmenu', function (e) {
    if (e.target !== address) e.preventDefault();
  });

  LB.post('ready');
})();
