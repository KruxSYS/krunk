// LightBrowser shell: tab strip, toolbar and address bar.
// All state comes from the host (`tabs`, `nav`, `shellConfig` events); the
// shell only renders it and sends commands back.
(function () {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const el = LB.el;

  const state = { tabs: [], active: null, nav: null, editing: false, popup: null };

  // ---------- tabs ----------
  function renderTabs() {
    const box = $('tabs');
    box.textContent = '';
    state.tabs.forEach(function (t, index) {
      const icon = t.loading
        ? el('span', { class: 'spinner', 'aria-label': 'Loading' })
        : t.favicon
          ? el('img', { class: 'favicon', src: t.favicon, alt: '' })
          : svgIcon('i-globe', 'favicon');
      const tab = el('div', {
        class: 'tab' + (t.id === state.active ? ' active' : '') + (t.private ? ' private' : '') +
          (t.sleeping ? ' sleeping' : ''),
        role: 'tab',
        'aria-selected': t.id === state.active ? 'true' : 'false',
        title: (t.private ? '[Private] ' : '') + t.title + (t.url ? '\n' + t.url : ''),
        draggable: 'true',
        'data-id': t.id,
        'data-index': index,
        onmousedown: function (e) {
          if (e.button !== 0 || e.target.closest('.close')) return;
          // Double-click duplicates. Detected here rather than with dblclick:
          // the first press activates the tab, which re-renders the strip, so
          // a dblclick on an inactive tab would never fire.
          if (lastPress.id === t.id && e.timeStamp - lastPress.at < DOUBLE_PRESS_MS) {
            lastPress = { id: null, at: 0 };
            LB.post('duplicateTab', { id: t.id });
            return;
          }
          lastPress = { id: t.id, at: e.timeStamp };
          LB.post('activateTab', { id: t.id });
        },
        onauxclick: function (e) { if (e.button === 1) { e.preventDefault(); LB.post('closeTab', { id: t.id }); } },
      }, [
        t.private ? el('span', { class: 'private-mark', title: 'Private tab' }, [svgIcon('i-mask')]) : null,
        icon,
        el('span', { class: 'title', text: t.title || 'New tab' }),
        el('button', {
          class: 'close', title: 'Close tab (Ctrl+W)', 'aria-label': 'Close tab',
          onclick: function (e) { e.stopPropagation(); LB.post('closeTab', { id: t.id }); },
        }, [svgIcon('i-close')]),
      ]);
      attachDrag(tab, t.id);
      box.appendChild(tab);
    });
  }

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

  // ---------- drag to reorder ----------
  let dragId = null;
  // Right half of the tab? Measured on the tab itself: offsetX would be
  // relative to whichever child (title, icon, close button) is under the cursor.
  function inRightHalf(e, node) {
    const r = node.getBoundingClientRect();
    return e.clientX - r.left > r.width / 2;
  }
  function attachDrag(node, id) {
    node.addEventListener('dragstart', function (e) {
      dragId = id;
      node.classList.add('dragging');
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', String(id));
    });
    node.addEventListener('dragend', function () {
      dragId = null;
      document.querySelectorAll('.tab').forEach(function (n) {
        n.classList.remove('dragging', 'drop-before', 'drop-after');
      });
    });
    node.addEventListener('dragover', function (e) {
      if (dragId === null || dragId === id) return;
      e.preventDefault();
      const after = inRightHalf(e, node);
      node.classList.toggle('drop-after', after);
      node.classList.toggle('drop-before', !after);
    });
    node.addEventListener('dragleave', function () { node.classList.remove('drop-before', 'drop-after'); });
    node.addEventListener('drop', function (e) {
      e.preventDefault();
      if (dragId === null || dragId === id) return;
      const after = inRightHalf(e, node);
      const from = state.tabs.findIndex(function (t) { return t.id === dragId; });
      let to = state.tabs.findIndex(function (t) { return t.id === id; }) + (after ? 1 : 0);
      if (from < to) to -= 1; // index after removing the dragged tab
      if (from !== to) {
        const moved = state.tabs.splice(from, 1)[0];
        state.tabs.splice(to, 0, moved);
        renderTabs();
        LB.post('moveTab', { id: dragId, index: to });
      }
    });
  }

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
    if (!state.editing) $('address').value = n.url;
    $('star').classList.toggle('bookmarked', !!n.bookmarked);
    // Built-in pages (lb://) cannot be bookmarked.
    $('star').hidden = !n.url || n.url.startsWith('lb://');
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
  $('home').addEventListener('click', function () { LB.post('home'); });
  $('newtab').addEventListener('click', function () { LB.post('newTab', {}); });
  $('star').addEventListener('click', function () { LB.post('toggleBookmark'); });
  $('tabstrip').addEventListener('dblclick', function (e) {
    if (e.target === $('tabstrip') || e.target === $('tabs')) LB.post('newTab', {});
  });

  // ---------- address bar ----------
  const address = $('address');
  address.addEventListener('focus', function () { state.editing = true; address.select(); });
  address.addEventListener('blur', function () { state.editing = false; renderNav(); });
  address.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      address.value = state.nav ? state.nav.url : '';
      address.blur();
    }
  });
  $('omnibox').addEventListener('submit', function (e) {
    e.preventDefault();
    const input = address.value.trim();
    if (!input) return;
    LB.post('navigate', { input: input });
    state.editing = false;
    address.blur();
  });

  // ---------- popups (menu, shield) ----------
  const POPUP_SIZES = { menu: [280, 482], shield: [300, 180] };
  function togglePopup(kind, button) {
    if (state.popup === kind) {
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
    state.popup = kind;
    $('menu').classList.toggle('on', kind === 'menu');
    $('shield').classList.toggle('on', kind === 'shield');
    document.querySelectorAll('.bm[data-folder]').forEach(function (b) {
      b.classList.toggle('on', kind === 'folder:' + b.dataset.folder);
    });
  }

  // ---------- bookmarks bar ----------
  function renderBar(items) {
    const bar = $('bookmarks-bar');
    bar.textContent = '';
    if (!items.length) {
      bar.appendChild(el('span', { class: 'bm-empty', text: 'Bookmarks in the “Bookmarks bar” folder appear here (Ctrl+D adds the page).' }));
      return;
    }
    items.forEach(function (b) {
      if (b.isFolder) {
        const btn = el('button', { class: 'bm', title: b.title, 'data-folder': b.id }, [
          svgIcon('i-folder', 'folder-icon'), el('span', { text: b.title }),
        ]);
        btn.addEventListener('click', function () {
          const key = 'folder:' + b.id;
          if (state.popup === key) { LB.post('hidePopup'); setPopup(null); return; }
          const r = btn.getBoundingClientRect();
          LB.post('showBookmarkFolder', { id: b.id, rect: { x: r.left, y: r.top, width: Math.max(1, r.width), height: Math.max(1, r.height) } });
          setPopup(key);
        });
        bar.appendChild(btn);
      } else {
        const btn = el('button', { class: 'bm', title: b.title + '\n' + b.url }, [
          b.favicon ? el('img', { src: b.favicon, alt: '' }) : svgIcon('i-globe'),
          el('span', { text: b.title }),
        ]);
        btn.addEventListener('click', function (e) { LB.post('openUrl', { url: b.url, newTab: e.ctrlKey }); });
        btn.addEventListener('auxclick', function (e) { if (e.button === 1) LB.post('openUrl', { url: b.url, newTab: true }); });
        bar.appendChild(btn);
      }
    });
  }

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
  $('menu').addEventListener('click', function () { togglePopup('menu', $('menu')); });
  $('shield').addEventListener('click', function () { togglePopup('shield', $('shield')); });

  // ---------- host events ----------
  LB.on('tabs', function (m) {
    state.tabs = m.tabs;
    state.active = m.active;
    renderTabs();
  });
  LB.on('nav', function (m) { state.nav = m.state; renderNav(); });
  LB.on('shellConfig', function (m) {
    $('home').hidden = !m.showHomeButton;
    $('bookmarks-bar').hidden = !m.showBookmarksBar;
    document.documentElement.classList.toggle('dark', m.theme === 'dark');
    document.documentElement.classList.toggle('light', m.theme === 'light');
  });
  LB.on('focusAddressBar', function () { address.focus(); address.select(); });
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
