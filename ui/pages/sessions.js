// krunk://sessions (0.15.0): the session logs. A list of past sessions (start, end, duration,
// top sites; the running one marked) and a detail per session (#s<id>): time per site and tab,
// Krunk's CPU use as a stacked chart (SVG built with DOM calls) and the timeline of visits and
// time in front. Delete one, clear all, how long to keep them; turning logging on when it is off.
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);
  const SVG = 'http://www.w3.org/2000/svg';
  const RETENTION = [1, 7, 14, 30, 60, 90, 180, 365];
  const SERIES = [
    { key: 'renderers', name: 'Tabs' },
    { key: 'browser', name: 'Browser' },
    { key: 'gpu', name: 'GPU' },
    { key: 'other', name: 'Utilities' },
  ];
  let state = { enabled: false, retentionDays: 30, current: null, sessions: [] };
  let shown = null;

  // ---- formatting ----
  function two(n) { return String(n).padStart(2, '0'); }
  function clock(ms) { const d = new Date(ms); return two(d.getHours()) + ':' + two(d.getMinutes()); }
  function clockS(ms) { return clock(ms) + ':' + two(new Date(ms).getSeconds()); }
  function day(ms) {
    const d = new Date(ms);
    return d.toLocaleDateString(undefined, { weekday: 'short', year: 'numeric', month: 'short', day: 'numeric' });
  }
  function dur(ms) {
    const s = Math.round(ms / 1000);
    if (s < 60) return s + ' s';
    const m = Math.floor(s / 60);
    if (m < 60) return m + ' min';
    const h = Math.floor(m / 60);
    return h + ' h ' + (m % 60 ? (m % 60) + ' min' : '');
  }
  function pct(v) { return (v || 0).toFixed(1) + ' %'; }
  function size(n) {
    if (!n) return '–';
    const u = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i && n < 10 ? n.toFixed(1) : Math.round(n)) + ' ' + u[i];
  }
  function range(s) {
    const end = s.current ? 'now' : clock(s.ended || s.started);
    const sameDay = s.current || new Date(s.ended || s.started).toDateString() === new Date(s.started).toDateString();
    return clock(s.started) + ' – ' + (sameDay ? end : day(s.ended) + ' ' + end);
  }

  // ---- header controls ----
  function fillRetention() {
    const sel = $('retention');
    const days = RETENTION.includes(state.retentionDays) ? RETENTION : RETENTION.concat([state.retentionDays]).sort((a, b) => a - b);
    sel.textContent = '';
    days.forEach(function (d) {
      sel.appendChild(el('option', { value: String(d), text: d === 1 ? '1 day' : d + ' days' }));
    });
    sel.value = String(state.retentionDays);
  }
  $('retention').addEventListener('change', function () {
    set({ retentionDays: Number($('retention').value) }, 'Sessions are kept for ' + $('retention').selectedOptions[0].textContent);
  });
  $('turn-on').addEventListener('click', function () { set({ enabled: true }, 'Session logging is on'); });
  $('open-settings').addEventListener('click', function (e) {
    e.preventDefault();
    LB.post('openPage', { page: 'settings', section: 'privacy' });
  });
  $('clear-all').addEventListener('click', function () {
    Page.confirm('Delete every session log? The running session starts over empty.', { ok: 'Clear all', danger: true }).then(function (ok) {
      if (!ok) return;
      LB.request('sessionClear').then(function () { Page.toast('Session logs cleared'); location.hash = ''; load(); },
        function (e) { Page.toast(e.message); });
    });
  });

  function set(args, message) {
    LB.request('sessionLogSet', args).then(function (data) { state = data; Page.toast(message); render(); },
      function (e) { Page.toast(e.message); fillRetention(); });
  }

  // ---- the list ----
  function sessionRow(s) {
    const top = s.topSites.map(function (t) {
      return el('span', { class: 'chip', title: t.host }, [t.host, t.ms ? el('span', { class: 't', text: dur(t.ms) }) : null]);
    });
    if (!top.length) top.push(el('span', { class: 'sub', text: 'No sites' }));
    const row = el('div', { class: 'row session', tabindex: '0', role: 'link', 'data-id': String(s.id), 'aria-label': 'Session of ' + day(s.started) + ', ' + range(s) }, [
      el('span', { class: 'when' }, [
        el('span', { class: 'date', text: day(s.started) }),
        el('span', { class: 'sub', text: range(s) }),
      ]),
      el('span', { class: 'dur' }, [
        el('span', { text: dur(s.durationMs) }),
        el('span', { class: 'sub', text: dur(s.activeMs) + ' in front' }),
      ]),
      el('span', { class: 'sites' }, top),
      el('span', { class: 'sub', text: s.visits + (s.visits === 1 ? ' page' : ' pages') }),
      s.current ? el('span', { class: 'badge', text: 'Running' }) : null,
      el('span', { class: 'actions' }, [
        s.current ? null : Page.iconButton('trash', 'Delete this session', function (e) { e.stopPropagation(); remove(s.id); }),
      ]),
    ]);
    const open = function () { location.hash = 's' + s.id; };
    row.addEventListener('click', open);
    row.addEventListener('keydown', function (e) { if (e.key === 'Enter') open(); });
    return row;
  }

  function renderList() {
    const list = $('list');
    list.textContent = '';
    if (!state.sessions.length) {
      list.appendChild(el('div', { class: 'empty' }, [
        el('strong', { text: 'No sessions yet' }),
        el('p', { text: state.enabled ? 'The running session shows here once Krunk has written it (every 30 seconds).' : 'Turn logging on to start recording sessions.' }),
      ]));
      return;
    }
    state.sessions.forEach(function (s) { list.appendChild(sessionRow(s)); });
  }

  function remove(id) {
    Page.confirm('Delete this session log?', { ok: 'Delete', danger: true }).then(function (ok) {
      if (!ok) return;
      LB.request('sessionDelete', { id: id }).then(function () { Page.toast('Session deleted'); if (location.hash) location.hash = ''; load(); },
        function (e) { Page.toast(e.message); });
    });
  }

  function render() {
    $('off').hidden = state.enabled;
    $('clear-all').disabled = !state.sessions.length;
    fillRetention();
    renderList();
  }

  // ---- the detail ----
  function stat(label, value) {
    return el('div', { class: 'stat' }, [el('div', { class: 'label', text: label }), el('div', { class: 'value', text: value })]);
  }

  function bars(box, items, label, title) {
    box.textContent = '';
    const max = Math.max(1, ...items.map((i) => i.ms));
    const shownItems = items.filter((i) => i.ms > 0).slice(0, 12);
    if (!shownItems.length) { box.appendChild(el('p', { class: 'muted', text: 'Nothing was in front.' })); return; }
    shownItems.forEach(function (i) {
      const fill = el('div', { class: 'fill' });
      fill.style.width = (i.ms / max * 100) + '%';
      box.appendChild(el('div', { class: 'bar-row', title: title(i) }, [
        el('span', { class: 'name', text: label(i) }),
        el('span', { class: 't', text: dur(i.ms) }),
        el('div', { class: 'track' }, [fill]),
      ]));
    });
  }

  function svgEl(tag, attrs) {
    const n = document.createElementNS(SVG, tag);
    Object.keys(attrs || {}).forEach(function (k) { n.setAttribute(k, attrs[k]); });
    return n;
  }

  function niceMax(v) {
    const steps = [1, 2, 5, 10, 20, 25, 50, 100];
    return steps.find((s) => s >= v) || 100;
  }

  // A stacked area chart: tabs at the bottom, then browser, GPU, utilities.
  function chart(box, samples) {
    box.textContent = '';
    const W = Math.max(320, box.clientWidth || 900), H = 220, L = 40, R = 8, T = 8, B = 22;
    const svg = svgEl('svg', { viewBox: '0 0 ' + W + ' ' + H, role: 'img', 'aria-label': 'CPU use over the session' });
    box.appendChild(svg);
    if (samples.length < 2) {
      const t = svgEl('text', { x: String(W / 2), y: String(H / 2), 'text-anchor': 'middle', class: 'nodata' });
      t.textContent = 'Not enough samples yet (one every 5 seconds).';
      svg.appendChild(t);
      return;
    }
    const t0 = samples[0].ts, t1 = samples[samples.length - 1].ts;
    const tot = (s) => s.renderers + s.browser + s.gpu + s.other;
    const ymax = niceMax(Math.max(...samples.map(tot)));
    const x = (ts) => L + (ts - t0) / Math.max(1, t1 - t0) * (W - L - R);
    const y = (v) => T + (1 - v / ymax) * (H - T - B);
    // Grid and axes.
    [0, 0.5, 1].forEach(function (f) {
      const v = ymax * f;
      svg.appendChild(svgEl('line', { class: 'gridline', x1: String(L), x2: String(W - R), y1: String(y(v)), y2: String(y(v)) }));
      const lab = svgEl('text', { class: 'axis', x: String(L - 6), y: String(y(v) + 4), 'text-anchor': 'end' });
      lab.textContent = (Number.isInteger(v) ? v : v.toFixed(1)) + '%';
      svg.appendChild(lab);
    });
    const ticks = 5;
    for (let i = 0; i <= ticks; i++) {
      const ts = t0 + (t1 - t0) * i / ticks;
      const lab = svgEl('text', { class: 'axis', x: String(x(ts)), y: String(H - 6), 'text-anchor': i === 0 ? 'start' : i === ticks ? 'end' : 'middle' });
      lab.textContent = clock(ts);
      svg.appendChild(lab);
    }
    // Areas, bottom up.
    const base = samples.map(() => 0);
    SERIES.forEach(function (ser) {
      const top = samples.map((s, i) => base[i] + (s[ser.key] || 0));
      let d = 'M' + x(samples[0].ts) + ',' + y(top[0]);
      for (let i = 1; i < samples.length; i++) d += 'L' + x(samples[i].ts) + ',' + y(top[i]);
      for (let i = samples.length - 1; i >= 0; i--) d += 'L' + x(samples[i].ts) + ',' + y(base[i]);
      svg.appendChild(svgEl('path', { class: 'area ' + ser.key, d: d + 'Z' }));
      top.forEach((v, i) => { base[i] = v; });
    });
    // Hover: a crosshair and the values at the nearest sample.
    const cross = svgEl('line', { class: 'cross', y1: String(T), y2: String(H - B), visibility: 'hidden' });
    svg.appendChild(cross);
    const tip = el('div', { class: 'tip', hidden: true });
    box.appendChild(tip);
    svg.addEventListener('mousemove', function (e) {
      const r = svg.getBoundingClientRect();
      const px = (e.clientX - r.left) / r.width * W;
      const ts = t0 + (px - L) / (W - L - R) * (t1 - t0);
      let best = samples[0];
      samples.forEach((s) => { if (Math.abs(s.ts - ts) < Math.abs(best.ts - ts)) best = s; });
      const cx = x(best.ts);
      cross.setAttribute('x1', String(cx));
      cross.setAttribute('x2', String(cx));
      cross.setAttribute('visibility', 'visible');
      tip.textContent = '';
      tip.appendChild(el('div', { text: clockS(best.ts) + ' · total ' + pct(tot(best)) }));
      SERIES.slice().reverse().forEach(function (ser) {
        tip.appendChild(el('div', { class: 'row2' }, [el('span', { class: 'sw ' + ser.key }), el('span', { text: ser.name }), el('span', { class: 'v', text: pct(best[ser.key]) })]));
      });
      tip.appendChild(el('div', { class: 'row2 muted' }, [el('span', { text: 'Memory' }), el('span', { class: 'v', text: size(best.memory) })]));
      tip.hidden = false;
      const left = cx / W * r.width;
      tip.style.left = (left > r.width / 2 ? left - tip.offsetWidth - 12 : left + 12) + 'px';
    });
    svg.addEventListener('mouseleave', function () { cross.setAttribute('visibility', 'hidden'); tip.hidden = true; });
  }

  function legend() {
    const box = $('legend');
    box.textContent = '';
    SERIES.forEach(function (ser) {
      box.appendChild(el('span', { class: 'key' }, [el('span', { class: 'sw ' + ser.key }), el('span', { text: ser.name })]));
    });
  }

  function timeline(d) {
    const box = $('timeline');
    box.textContent = '';
    const items = d.visits.map((v) => ({ at: v.ts, kind: 'visit', v: v }))
      .concat(d.focus.map((f) => ({ at: f.started, kind: 'focus', f: f })))
      .sort((a, b) => a.at - b.at || (a.kind === 'visit' ? -1 : 1));
    if (!items.length) { box.appendChild(el('li', { class: 'muted', text: 'Nothing recorded yet.' })); }
    items.forEach(function (it) {
      if (it.kind === 'visit') {
        box.appendChild(el('li', { class: 'visit' }, [
          el('span', { class: 'at', text: clockS(it.at) }),
          el('span', { class: 'kind', text: 'Opened' }),
          el('span', { class: 'what', title: it.v.url }, [it.v.title || it.v.url, el('span', { class: 'sub', text: it.v.host })]),
        ]));
      } else {
        box.appendChild(el('li', { class: 'focus' }, [
          el('span', { class: 'at', text: clockS(it.at) }),
          el('span', { class: 'kind', text: 'In front' }),
          el('span', { class: 'what' }, [it.f.host, el('span', { class: 'sub', text: dur(it.f.ended - it.f.started) + ' (tab ' + it.f.tab + ')' })]),
        ]));
      }
    });
    const more = d.visitsTotal - d.visits.length;
    $('timeline-more').hidden = more <= 0;
    $('timeline-more').textContent = more > 0 ? more + ' later pages are not listed.' : '';
  }

  function renderDetail(d) {
    shown = d;
    const s = d.session;
    $('detail-title').textContent = day(s.started) + ', ' + range(s);
    $('detail-current').hidden = !s.current;
    $('delete').hidden = s.current;
    const stats = $('stats');
    stats.textContent = '';
    [
      stat('Duration', dur(s.durationMs)),
      stat('In front', dur(s.activeMs)),
      stat('Pages', String(s.visits)),
      stat('Sites', String(s.sites)),
      stat('CPU average', pct(d.cpu.avgTotal)),
      stat('CPU peak', pct(d.cpu.peakTotal)),
      stat('GPU process average', pct(d.cpu.avgGpu)),
      stat('Memory peak', size(d.cpu.peakMemory)),
    ].forEach((n) => stats.appendChild(n));
    bars($('sites'), d.sites, (i) => i.host, (i) => i.host + ': ' + dur(i.ms) + ', ' + i.visits + ' page(s)');
    bars($('tabs'), d.tabs, (i) => i.title, (i) => i.title + ' (' + i.host + ')');
    legend();
    chart($('chart'), d.samples);
    timeline(d);
  }

  $('back').addEventListener('click', function () { location.hash = ''; });
  $('delete').addEventListener('click', function () { if (shown) remove(shown.session.id); });

  // ---- routing ----
  function route() {
    const m = /^#s(\d+)$/.exec(location.hash);
    $('list-view').hidden = !!m;
    $('detail-view').hidden = !m;
    if (!m) { shown = null; return; }
    LB.request('sessionDetail', { id: Number(m[1]) }).then(renderDetail, function (e) {
      Page.toast(e.message);
      location.hash = '';
    });
  }
  window.addEventListener('hashchange', route);

  function load() {
    return LB.request('sessionList').then(function (data) { state = data; render(); }, function (e) { Page.toast(e.message); });
  }
  // Up to date when the tab comes back to the front.
  document.addEventListener('visibilitychange', function () { if (!document.hidden) { load(); if (shown) route(); } });

  load().then(route);
})();
