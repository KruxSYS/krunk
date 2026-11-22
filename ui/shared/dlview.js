// Krunk 0.7.0 (R9-R11): what the downloads popup and the downloads page share.
//   DlView.kind(name)        the file-type class of a name ('archive', 'video', … 'file')
//   DlView.icon(d)           the file-type icon of a download
//   DlView.size(n), status(d), left(s), site(d), kindLabel(kind)
//   DlView.bar(d)            a progress bar: one fill, or the parts of a download in parts,
//                            each at its place in the file and coloured by its connection
//   DlView.motion            moves the fills smoothly between the host's updates
//   DlView.dragOut(row, start) a press on a finished download's row that moves a few pixels
//                            calls start(): the host then drags the file out (Windows shell)
// The host pushes progress a few times a second; between pushes the fills move on at the
// measured speed (requestAnimationFrame, never backwards, never past the next byte count by
// more than a moment's worth). With reduced motion the fills jump to each pushed value.
(function () {
  'use strict';
  const el = LB.el;

  const KINDS = {
    archive: ['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst', 'cab'],
    video: ['mp4', 'mkv', 'webm', 'mov', 'avi', 'm4v', 'wmv', 'flv'],
    audio: ['mp3', 'wav', 'flac', 'ogg', 'oga', 'm4a', 'aac', 'opus', 'wma'],
    image: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif', 'heic', 'tif', 'tiff'],
    pdf: ['pdf'],
    doc: ['doc', 'docx', 'odt', 'rtf', 'txt', 'md', 'xls', 'xlsx', 'ods', 'csv', 'ppt', 'pptx', 'odp', 'epub'],
    app: ['exe', 'msi', 'msix', 'appx', 'bat', 'cmd', 'ps1', 'apk', 'dmg', 'deb', 'rpm', 'jar', 'crx'],
    disk: ['iso', 'img', 'vhd', 'vhdx', 'bin', 'vmdk', 'qcow2'],
    code: ['js', 'mjs', 'ts', 'json', 'html', 'htm', 'css', 'py', 'rs', 'c', 'h', 'cpp', 'java', 'xml', 'yaml', 'yml', 'sh', 'go'],
  };
  const BY_EXT = {};
  Object.keys(KINDS).forEach(function (k) { KINDS[k].forEach(function (e) { BY_EXT[e] = k; }); });

  function kind(name) {
    const m = /\.([a-z0-9]{1,6})$/i.exec(name || '');
    return (m && BY_EXT[m[1].toLowerCase()]) || 'file';
  }
  function icon(d) {
    const k = kind(d.fileName);
    return el('span', { class: 'dl-icon', 'data-kind': k, 'aria-hidden': 'true' }, [el('span', { class: 'dl-glyph' })]);
  }

  function size(n) {
    if (n === null || n === undefined) return '';
    const u = ['B', 'KB', 'MB', 'GB', 'TB'];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i ? n.toFixed(1) : Math.round(n)) + ' ' + u[i];
  }
  function left(s) { return s >= 3600 ? Math.round(s / 3600) + ' h left' : s >= 60 ? Math.round(s / 60) + ' min left' : s + ' s left'; }
  function host(url) { try { return new URL(url).hostname.replace(/^www\./, ''); } catch (e) { return ''; } }
  // 0.15.0: the site a download came from (its page, else its address), and a kind's name.
  function site(d) { return host(d.pageUrl || '') || host(d.url); }
  const KIND_LABELS = { archive: 'Archive', video: 'Video', audio: 'Audio', image: 'Image', pdf: 'PDF', doc: 'Document', app: 'Program', disk: 'Disk image', code: 'Code', file: 'File' };
  function kindLabel(k) { return KIND_LABELS[k] || 'File'; }
  function status(d) {
    if (d.state === 'completed') return [size(d.total || d.received), host(d.url)].filter(Boolean).join(' · ');
    if (d.state === 'cancelled') return 'Cancelled';
    if (d.state === 'interrupted') return 'Failed' + (d.reason ? ' – ' + d.reason : '');
    let s = size(d.received) + (d.total ? ' of ' + size(d.total) : '');
    if (d.state === 'paused') return s + ' · Paused' + (d.reason ? ' – ' + d.reason : '');
    if (d.speed) s += ' · ' + size(d.speed) + '/s';
    if (d.eta !== null && d.eta !== undefined) s += ' · ' + left(d.eta);
    return s;
  }

  // ---------- smooth fills ----------
  const reduce = window.matchMedia ? window.matchMedia('(prefers-reduced-motion: reduce)') : { matches: false };
  // At most this long of predicted progress past the last pushed value.
  const AHEAD_MS = 1500;
  const tracks = new Map();   // key -> { fill, base, at, rate, shown, len, seen }
  let frame = 0;

  function paint(t) {
    t.fill.style.transform = 'scaleX(' + (t.len > 0 ? Math.min(1, t.shown / t.len) : 0).toFixed(5) + ')';
  }
  function tick(now) {
    frame = 0;
    let moving = false;
    tracks.forEach(function (t) {
      if (!t.fill.isConnected) return;
      if (t.rate > 0 && t.shown < t.len) {
        const predicted = Math.min(t.len, t.base + t.rate * Math.min(now - t.at, AHEAD_MS) / 1000);
        if (predicted > t.shown) { t.shown = predicted; paint(t); }
        moving = moving || now - t.at < AHEAD_MS;
      }
    });
    if (moving && !document.hidden) frame = requestAnimationFrame(tick);
  }
  // A pushed value for one fill: `value` of `len` bytes, `rate` bytes/s if known (else measured).
  function track(key, fill, value, len, rate, running) {
    const now = performance.now();
    let t = tracks.get(key);
    if (!t || t.len !== len) {
      t = { fill: fill, base: value, at: now, rate: 0, shown: value, len: len, seen: 0 };
      tracks.set(key, t);
    } else {
      t.fill = fill;
      if (rate === null || rate === undefined) {
        const dt = now - t.at;
        if (dt > 50) {
          const measured = Math.max(0, (value - t.base) * 1000 / dt);
          t.rate = t.seen ? t.rate + 0.5 * (measured - t.rate) : measured;
        }
      }
      t.base = value;
      t.at = now;
      t.seen++;
    }
    if (rate !== null && rate !== undefined) t.rate = rate;
    if (!running) t.rate = 0;
    // Never backwards: an overshoot waits for the real count.
    t.shown = reduce.matches ? value : Math.max(t.shown, value);
    if (value >= len) t.shown = len;
    paint(t);
    if (!reduce.matches && running && !frame) frame = requestAnimationFrame(tick);
  }
  function forget(prefix) {
    tracks.forEach(function (_, k) { if (k === prefix || k.indexOf(prefix + ':') === 0) tracks.delete(k); });
  }
  document.addEventListener('visibilitychange', function () { if (!document.hidden && !frame) frame = requestAnimationFrame(tick); });

  // ---------- bars ----------
  // The bar's element for `d` (made once per download; `update` refreshes it in place).
  function bar(d) {
    const parts = d.partProgress && d.partProgress.length > 1;
    const node = parts ? el('div', { class: 'dl-segments' }) : el('div', { class: 'dl-bar' }, [el('span', { class: 'dl-fill' })]);
    node.dataset.id = String(d.id);
    update(node, d);
    return node;
  }
  function update(node, d) {
    const running = d.state === 'inProgress';
    const total = d.total || 0;
    if (node.classList.contains('dl-bar')) {
      node.classList.toggle('indeterminate', !total);
      node.classList.toggle('paused', d.state === 'paused');
      track(String(d.id), node.firstChild, d.received, total, d.speed || 0, running);
      return;
    }
    // Parts: each at its offset (they are split while they run, so the set changes).
    const sum = total || d.partProgress.reduce(function (a, p) { return a + p.len; }, 0) || 1;
    const want = d.partProgress.map(function (p) { return p.start + ':' + p.len; }).join(',');
    if (node.dataset.layout !== want) {
      node.dataset.layout = want;
      node.textContent = '';
      forget(String(d.id));
      d.partProgress.forEach(function (p) {
        const seg = el('span', { class: 'seg c' + (p.conn % 8), title: 'Connection ' + (p.conn + 1) }, [el('span', { class: 'fill' })]);
        seg.style.left = (100 * p.start / sum) + '%';
        seg.style.width = (100 * p.len / sum) + '%';
        node.appendChild(seg);
      });
    }
    node.classList.toggle('paused', d.state === 'paused');
    const segs = node.children;
    d.partProgress.forEach(function (p, i) {
      if (segs[i]) track(d.id + ':' + p.start, segs[i].firstChild, p.done, p.len, null, running && p.done < p.len);
    });
    node.title = d.partProgress.length + ' parts over ' + (d.connections || new Set(d.partProgress.map(function (p) { return p.conn; })).size) + ' connections';
  }

  // 0.27.0: dragging a finished download out to Explorer, the desktop or another app. The page
  // can't put a file on the system's drag, so once the pointer moved a few pixels with the
  // primary button held, start() asks the host, which runs the shell's drag (it follows the
  // still-held button from there). Presses on the row's action buttons don't drag; the row's
  // own drag and text selection are off, and the click a drag may end with doesn't open.
  const DRAG_SLOP = 4;
  function dragOut(row, start) {
    let from = null;
    let dragged = false;
    row.addEventListener('dragstart', function (e) { e.preventDefault(); });
    row.addEventListener('mousedown', function (e) {
      dragged = false;
      from = e.button === 0 && !e.target.closest('.acts, .dl-acts') ? { x: e.clientX, y: e.clientY } : null;
      if (!from) return;
      // No text selection (or the page's own drag of it); focus as the press would have.
      e.preventDefault();
      const target = e.target.closest('button, [tabindex]');
      if (target && row.contains(target)) target.focus({ preventScroll: true });
    });
    row.addEventListener('mousemove', function (e) {
      if (!from) return;
      if (!(e.buttons & 1)) { from = null; return; }
      if (Math.abs(e.clientX - from.x) < DRAG_SLOP && Math.abs(e.clientY - from.y) < DRAG_SLOP) return;
      from = null;
      dragged = true;
      start();
    });
    row.addEventListener('mouseup', function () { from = null; });
    row.addEventListener('click', function (e) {
      if (dragged) { dragged = false; e.stopPropagation(); e.preventDefault(); }
    }, true);
  }

  window.DlView = { dragOut: dragOut, kind: kind, icon: icon, size: size, left: left, status: status, bar: bar, update: update, forget: forget, host: host,
    site: site, kindLabel: kindLabel };
})();
