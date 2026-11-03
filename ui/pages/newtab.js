// New tab page: search box and the most-visited tiles (frecency, from local
// history only; no network requests).
(function () {
  'use strict';
  const el = LB.el;
  const box = document.getElementById('tiles');

  function render(tiles) {
    box.textContent = '';
    if (!tiles.length) {
      box.appendChild(el('p', { id: 'empty', text: 'Sites you visit often will appear here.' }));
      return;
    }
    tiles.forEach(function (t) {
      const pinBtn = Page.iconButton('pin', t.pinned ? 'Unpin' : 'Pin', function (e) {
        e.preventDefault();
        LB.request(t.pinned ? 'unpinSite' : 'pinSite', t.pinned ? { url: t.url } : { url: t.url, title: t.title }).then(load);
      });
      pinBtn.classList.add('pin');
      const tile = el('a', { class: 'tile' + (t.pinned ? ' pinned' : ''), href: t.url, title: t.title + '\n' + t.url }, [
        t.pinned ? el('span', { class: 'pinned-mark', title: 'Pinned' }, [Page.svg(Page.ICONS.pin)]) : null,
        el('span', { class: 'avatar' }, [Page.favicon(t.url, t.favicon)]),
        el('span', { class: 'label', text: t.title }),
        el('span', { class: 'tools' }, [
          pinBtn,
          Page.iconButton('close', 'Remove', function (e) {
            e.preventDefault();
            const req = t.pinned ? LB.request('unpinSite', { url: t.url }).then(function () { return LB.request('hideSite', { url: t.url }); })
              : LB.request('hideSite', { url: t.url });
            req.then(load);
          }),
        ]),
      ]);
      box.appendChild(tile);
    });
  }

  function load() {
    LB.request('getTopSites').then(render, function () { render([]); });
  }

  document.getElementById('search').addEventListener('submit', function (e) {
    e.preventDefault();
    const q = document.getElementById('q').value.trim();
    if (q) LB.post('navigate', { input: q });
  });

  LB.post('ready');
  load();
})();
