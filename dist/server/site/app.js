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

  function size(bytes) {
    return (bytes / (1024 * 1024)).toFixed(0) + ' MB';
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
    box.appendChild(el('p', { class: 'version' }, [
      'Version ', el('strong', { text: String(r.version) }),
      r.released ? ' · released ' + String(r.released) : '',
      safePath(r.notes) ? ' · ' : '',
      safePath(r.notes) ? el('a', { href: safePath(r.notes), text: 'release notes' }) : null,
    ]));
    const buttons = el('div', { class: 'buttons' });
    if (installer) {
      buttons.appendChild(el('a', { class: 'button primary', href: safePath(installer.url), download: '' }, [
        'Download for Windows', el('span', { class: 'sub', text: 'Installer · ' + size(installer.size) }),
      ]));
    }
    if (portable) {
      buttons.appendChild(el('a', { class: 'button', href: safePath(portable.url), download: '' }, [
        'Portable zip', el('span', { class: 'sub', text: size(portable.size) }),
      ]));
    }
    box.appendChild(buttons);
    box.appendChild(el('p', { class: 'req', text: 'Windows 10 or 11, 64-bit.' }));
    const sums = el('dl', { class: 'sums' });
    [installer, portable].forEach(function (f) {
      if (!f) return;
      sums.appendChild(el('dt', { text: String(f.name) }));
      sums.appendChild(el('dd', {}, [el('code', { text: 'SHA-256 ' + String(f.sha256) })]));
    });
    box.appendChild(sums);
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
