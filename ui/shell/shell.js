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
          if (e.button === 0 && !e.target.closest('.close')) LB.post('activateTab', { id: t.id });
        },
        onauxclick: function (e) { if (e.button === 1) { e.preventDefault(); LB.post('closeTab', { id: t.id }); } },
        ondblclick: function (e) { if (!e.target.closest('.close')) LB.post('duplicateTab', { id: t.id }); },
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

  // ---------- drag to reorder ----------
  let dragId = null;
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
      const after = e.offsetX > node.clientWidth / 2;
      node.classList.toggle('drop-after', after);
      node.classList.toggle('drop-before', !after);
    });
    node.addEventListener('dragleave', function () { node.classList.remove('drop-before', 'drop-after'); });
    node.addEventListener('drop', function (e) {
      e.preventDefault();
      if (dragId === null || dragId === id) return;
      const after = e.offsetX > node.clientWidth / 2;
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
    $('back').disabled = !n.canGoBack;
    $('forward').disabled = !n.canGoForward;
    $('reload-icon').setAttribute('href', n.loading ? '#i-stop' : '#i-reload');
    $('reload').title = n.loading ? 'Stop (Esc)' : 'Reload (Ctrl+R)';
    if (!state.editing) $('address').value = n.url;
    $('star').classList.toggle('bookmarked', !!n.bookmarked);
    $('star').hidden = !n.url;
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
  const POPUP_SIZES = { menu: [280, 440], shield: [300, 180] };
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
  }
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
    document.documentElement.classList.toggle('dark', !!m.dark);
    document.documentElement.classList.toggle('light', !m.dark);
  });
  LB.on('focusAddressBar', function () { address.focus(); address.select(); });
  LB.on('popupClosed', function () { setPopup(null); });

  // Keep keyboard shortcuts working while the shell has focus: the host sees
  // them through AcceleratorKeyPressed, so the shell does nothing extra here.
  document.addEventListener('contextmenu', function (e) {
    if (e.target !== address) e.preventDefault();
  });

  LB.post('ready');
})();
