// Fills the download box from /download/release.json (written by publish-release.sh).
(function () {
  'use strict';
  const box = document.getElementById('download');

  function el(tag, attrs, children) {
    const n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      if (k === 'text') n.textContent = attrs[k];
      else n.setAttribute(k, attrs[k]);
    });
    (children || []).forEach(function (c) { if (c) n.appendChild(typeof c === 'string' ? document.createTextNode(c) : c); });
    return n;
  }

  // An arrow into a tray (inline SVG: the CSP allows no data: images).
  function downloadIcon() {
    const NS = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('class', 'dl-icon');
    svg.setAttribute('aria-hidden', 'true');
    ['M12 3v12', 'm7 10 5 5 5-5', 'M4 17v3h16v-3'].forEach(function (d) {
      const p = document.createElementNS(NS, 'path');
      p.setAttribute('d', d);
      svg.appendChild(p);
    });
    return svg;
  }

  // '' when the release data has no usable size.
  function size(bytes) {
    return typeof bytes === 'number' && bytes > 0 ? (bytes / (1024 * 1024)).toFixed(0) + ' MB' : '';
  }

  // Only our own relative /download/ paths become links.
  function safePath(p) {
    return typeof p === 'string' && /^\/download\/[A-Za-z0-9._\/-]+$/.test(p) && p.indexOf('..') < 0 ? p : null;
  }

  function none() {
    box.textContent = '';
    box.appendChild(el('p', { class: 'none', text: 'No release published yet.' }));
  }

  function render(r) {
    const files = Array.isArray(r.files) ? r.files : [];
    const installer = files.find(function (f) { return f.kind === 'installer' && safePath(f.url); });
    const portable = files.find(function (f) { return f.kind === 'portable' && safePath(f.url); });
    if (!installer && !portable) return none();
    box.textContent = '';
    const main = installer || portable;
    box.appendChild(el('a', { class: 'btn primary big', href: safePath(main.url), download: '' }, [
      downloadIcon(),
      el('span', {}, [
        el('span', { class: 'dl-main', text: installer ? 'Download for Windows' : 'Download the portable zip' }),
        el('span', { class: 'sub', text: [installer ? 'Installer' : 'Zip', size(main.size), '64-bit'].filter(Boolean).join(' · ') }),
      ]),
    ]));
    const meta = el('p', { class: 'version' }, [
      'Version ', el('strong', { text: String(r.version) }),
      r.released ? ' · ' + String(r.released) : '',
      safePath(r.notes) ? ' · ' : '',
      safePath(r.notes) ? el('a', { href: safePath(r.notes), text: 'What\u2019s new' }) : null,
      installer && portable ? ' · ' : '',
      installer && portable ? el('a', { href: safePath(portable.url), download: '', text: 'Portable zip' + (size(portable.size) ? ' (' + size(portable.size) + ')' : '') }) : null,
    ]);
    box.appendChild(meta);
    const sums = el('dl', { class: 'sums' });
    [installer, portable].forEach(function (f) {
      if (!f) return;
      sums.appendChild(el('dt', { text: String(f.name) }));
      sums.appendChild(el('dd', {}, [el('code', { text: String(f.sha256) })]));
    });
    box.appendChild(el('details', { class: 'checksums' }, [el('summary', { text: 'SHA-256 checksums' }), sums]));
  }

  // A visitor using Krunk: the browser leaves `window.krunk = { version }` on this site only
  // (frozen, main frame, independent of the User-Agent). The box says whether it is current.
  function krunkVersion() {
    try {
      var k = window.krunk;
      return k && typeof k.version === 'string' && /^\d+\.\d+\.\d+/.test(k.version) ? k.version : null;
    } catch (e) { return null; }
  }
  function cmpVersion(a, b) {
    var pa = a.split('.').map(Number), pb = b.split('.').map(Number);
    for (var i = 0; i < 3; i++) { if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) < (pb[i] || 0) ? -1 : 1; }
    return 0;
  }
  function usingKrunk(latest) {
    var mine = krunkVersion();
    if (!mine) return;
    var p = el('p', { class: 'using' + (latest && cmpVersion(mine, latest) < 0 ? ' outdated' : ''), id: 'using-krunk' });
    p.appendChild(el('strong', { text: "You're using Krunk " + mine }));
    if (!latest) p.appendChild(document.createTextNode(' — thank you.'));
    else if (cmpVersion(mine, latest) < 0) p.appendChild(document.createTextNode(' — version ' + latest + ' is available: Krunk offers it in Settings → About (Update now), or download it below.'));
    else p.appendChild(document.createTextNode(' — up to date' + (cmpVersion(mine, latest) > 0 ? ' (ahead of the published release)' : '') + '.'));
    box.insertBefore(p, box.firstChild);
  }

  fetch('/download/release.json', { cache: 'no-cache' })
    .then(function (res) { if (!res.ok) throw new Error(res.status); return res.json(); })
    .then(function (r) { render(r); usingKrunk(String(r.version)); })
    .catch(function () { none(); usingKrunk(null); });
})();
