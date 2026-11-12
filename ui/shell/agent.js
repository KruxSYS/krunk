// The AI agent panel (0.12.0): a per-window view docked right of the page. UI only: the agent
// loop runs in the browser process, which sends `agent*` events here; the panel never sees keys
// or provider traffic. Everything the model or a page wrote is put in with textContent
// (Markdown through markdown.js, which builds nodes the same way).
(function () {
  'use strict';
  const $ = function (id) { return document.getElementById(id); };
  const MD = window.KrunkMarkdown;
  const log = $('log');
  const input = $('input');
  const BUSY = { running: true, asking: true, maxSteps: true };

  const st = {
    state: 'idle',
    steps: 0,
    config: null,
    live: null,          // the assistant message being streamed
    stepRows: new Map(), // step n -> row
    asks: new Map(),     // open cards: key -> close(reason)
    scripts: new Map(),  // kind:id -> card
    scratch: { plan: [], notes: [], bytes: 0 }, // 0.16.0: the agent's plan and notes
    openNotes: new Set(), // names of the notes shown expanded
  };

  // ---------- small helpers ----------

  function icon(name, cls) {
    const ns = 'http://www.w3.org/2000/svg';
    const s = document.createElementNS(ns, 'svg');
    s.setAttribute('class', 'icon' + (cls ? ' ' + cls : ''));
    s.setAttribute('aria-hidden', 'true');
    const u = document.createElementNS(ns, 'use');
    u.setAttribute('href', '#i-' + name);
    s.appendChild(u);
    return s;
  }
  function el(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text !== undefined && text !== null) n.textContent = text;
    return n;
  }
  function button(cls, text, onClick, label) {
    const b = el('button', cls, text);
    b.type = 'button';
    if (label) b.setAttribute('aria-label', label);
    b.addEventListener('click', onClick);
    return b;
  }
  function busy() { return !!BUSY[st.state]; }

  const mdOpts = { onLink: function (url) { if (/^https?:\/\//i.test(url)) LB.post('openUrl', { url: url, newTab: true }); } };

  // Keep the newest content in view, unless the user scrolled up to read.
  function nearBottom() { return log.scrollHeight - log.scrollTop - log.clientHeight < 48; }
  function follow(was) { if (was) log.scrollTop = log.scrollHeight; }
  function append(node) {
    const was = nearBottom();
    $('empty').hidden = true;
    log.appendChild(node);
    follow(was);
    return node;
  }
  // Screen readers hear finished messages, questions and errors (not every streamed word).
  let announceTimer = 0;
  function announce(text) {
    clearTimeout(announceTimer);
    $('announce').textContent = '';
    announceTimer = setTimeout(function () { $('announce').textContent = String(text).slice(0, 600); }, 30);
  }

  // ---------- header: model and quick switch ----------

  function renderConfig() {
    const c = st.config || {};
    const name = c.model ? (c.providerName ? c.providerName + ' · ' : '') + c.model : 'No model';
    $('model-name').textContent = name;
    $('model').title = 'Model: ' + name + ' (click to switch)';
    $('model').classList.toggle('problem', !!c.problem);
    $('problem').hidden = !c.problem;
    $('problem-text').textContent = c.problem || '';
    if (!$('models').hidden) renderModels();
  }

  function renderModels() {
    const box = $('models');
    const c = st.config || {};
    const models = Array.isArray(c.models) ? c.models : [];
    box.textContent = '';
    if (!models.length) {
      box.appendChild(el('div', 'mempty', 'No models yet. Set up a provider in Settings → AI.'));
      return;
    }
    const providers = new Set(models.map(function (m) { return m.provider; }));
    let group = null;
    models.forEach(function (m) {
      if (providers.size > 1 && m.provider !== group) {
        group = m.provider;
        box.appendChild(el('div', 'mgroup', m.providerName || m.provider));
      }
      const on = m.provider === c.provider && m.model === c.model;
      const b = button('mopt', null, function () { selectModel(m); });
      b.setAttribute('role', 'option');
      b.setAttribute('aria-selected', on ? 'true' : 'false');
      b.appendChild(icon('check'));
      b.appendChild(el('span', null, m.model));
      b.title = (m.providerName ? m.providerName + ' · ' : '') + m.model;
      box.appendChild(b);
    });
  }

  function openModels(open) {
    const box = $('models');
    if (open === !box.hidden) return;
    box.hidden = !open;
    $('model').setAttribute('aria-expanded', open ? 'true' : 'false');
    if (open) {
      renderModels();
      LB.post('agentModels');
      const cur = box.querySelector('.mopt[aria-selected="true"]') || box.querySelector('.mopt');
      (cur || box).focus();
    }
  }

  function selectModel(m) {
    openModels(false);
    $('model').focus();
    if (st.config && st.config.provider === m.provider && st.config.model === m.model) return;
    LB.post('agentSelectModel', { provider: m.provider, model: m.model });
    // Shown at once; the host's agentConfig confirms it.
    st.config = Object.assign({}, st.config, { provider: m.provider, providerName: m.providerName, model: m.model });
    renderConfig();
  }

  $('model').addEventListener('click', function () { openModels($('models').hidden); });
  $('models').addEventListener('keydown', function (e) {
    const opts = Array.from($('models').querySelectorAll('.mopt'));
    const i = opts.indexOf(document.activeElement);
    let to = -1;
    if (e.key === 'ArrowDown') to = Math.min(opts.length - 1, i + 1);
    else if (e.key === 'ArrowUp') to = Math.max(0, i - 1);
    else if (e.key === 'Home') to = 0;
    else if (e.key === 'End') to = opts.length - 1;
    else if (e.key === 'Tab') { openModels(false); return; }
    if (to >= 0 && opts[to]) { e.preventDefault(); opts[to].focus(); }
  });
  document.addEventListener('pointerdown', function (e) {
    if (!$('models').hidden && !$('models').contains(e.target) && !$('model').contains(e.target)) openModels(false);
  });
  $('new').addEventListener('click', function () { LB.post('agentNew'); input.focus(); });
  $('close').addEventListener('click', function () { LB.post('agentClose'); });

  // ---------- scratchpad (0.16.0) ----------
  // The agent's plan and notes of this conversation, as the host last sent them (`agentScratch`
  // after every change). All of it is model output, possibly copied from a page: text nodes only.

  const PLAN_STATES = { todo: 'To do', doing: 'Doing', done: 'Done', dropped: 'Dropped' };

  function sizeText(bytes) {
    if (bytes === 1) return '1 byte';
    if (bytes < 1024) return bytes + ' bytes';
    return (bytes / 1024).toFixed(bytes < 10240 ? 1 : 0) + ' KiB';
  }

  function cleanScratch(v) {
    const out = { plan: [], notes: [], bytes: 0 };
    if (!v || typeof v !== 'object') return out;
    (Array.isArray(v.plan) ? v.plan : []).forEach(function (p) {
      if (!p || typeof p.text !== 'string') return;
      out.plan.push({ id: String(p.id == null ? '' : p.id), text: p.text, state: PLAN_STATES[p.state] ? p.state : 'todo' });
    });
    (Array.isArray(v.notes) ? v.notes : []).forEach(function (n) {
      if (!n || typeof n.name !== 'string') return;
      const content = typeof n.content === 'string' ? n.content : '';
      out.notes.push({ name: n.name, content: content, bytes: typeof n.bytes === 'number' ? n.bytes : content.length });
    });
    out.bytes = typeof v.bytes === 'number' ? v.bytes : out.notes.reduce(function (a, n) { return a + n.bytes; }, 0);
    return out;
  }

  function renderScratch() {
    const sc = st.scratch;
    const any = sc.plan.length > 0 || sc.notes.length > 0;
    const parts = [];
    if (sc.plan.length) parts.push(sc.plan.length === 1 ? '1 plan item' : sc.plan.length + ' plan items');
    if (sc.notes.length) parts.push((sc.notes.length === 1 ? '1 note' : sc.notes.length + ' notes') + ', ' + sizeText(sc.bytes));
    const summary = parts.join(' · ');
    const b = $('scratch-btn');
    b.classList.toggle('has', any);
    const label = "Scratchpad: the agent's plan and notes" + (any ? ' (' + parts.join(', ') + ')' : '');
    b.title = label;
    b.setAttribute('aria-label', label);
    $('sp-size').textContent = summary;
    $('sp-empty').hidden = any;
    $('sp-clear').disabled = !any;

    $('sp-plan-box').hidden = !sc.plan.length;
    const plan = $('sp-plan');
    plan.textContent = '';
    sc.plan.forEach(function (p) {
      const li = el('li', 'sp-item ' + p.state);
      li.appendChild(el('span', 'sp-state', PLAN_STATES[p.state]));
      li.appendChild(el('span', 'sp-text', p.text));
      plan.appendChild(li);
    });

    $('sp-notes-box').hidden = !sc.notes.length;
    const names = new Set(sc.notes.map(function (n) { return n.name; }));
    Array.from(st.openNotes).forEach(function (n) { if (!names.has(n)) st.openNotes.delete(n); });
    const notes = $('sp-notes');
    // The focus stays on a note's row when the list is drawn again under it.
    const focused = document.activeElement && document.activeElement.classList.contains('sp-note-head')
      ? document.activeElement.dataset.name : null;
    notes.textContent = '';
    sc.notes.forEach(function (n) {
      const open = st.openNotes.has(n.name);
      const row = el('div', 'sp-note');
      const head = el('button', 'sp-note-head');
      head.type = 'button';
      head.dataset.name = n.name;
      head.setAttribute('aria-expanded', open ? 'true' : 'false');
      head.appendChild(el('span', 'sp-note-name', n.name));
      head.appendChild(el('span', 'sp-note-size', sizeText(n.bytes)));
      head.appendChild(icon('chevron', 'chev'));
      const body = el('pre', 'sp-note-body' + (n.content ? '' : ' blank'), n.content || '(empty)');
      body.hidden = !open;
      head.addEventListener('click', function () {
        const show = body.hidden;
        body.hidden = !show;
        head.setAttribute('aria-expanded', show ? 'true' : 'false');
        if (show) st.openNotes.add(n.name); else st.openNotes.delete(n.name);
      });
      row.appendChild(head);
      row.appendChild(body);
      notes.appendChild(row);
      if (focused === n.name) head.focus({ preventScroll: true });
    });
  }

  function openScratch(open) {
    $('scratch').hidden = !open;
    $('scratch-btn').setAttribute('aria-expanded', open ? 'true' : 'false');
  }

  LB.on('agentScratch', function (m) {
    st.scratch = cleanScratch(m.scratch);
    renderScratch();
  });
  $('scratch-btn').addEventListener('click', function () { openScratch($('scratch').hidden); });
  // Clear asks nothing: the host drops the notes and the plan and confirms with an empty
  // `agentScratch`; the view is emptied at once.
  $('sp-clear').addEventListener('click', function () {
    LB.post('agentScratchClear');
    st.scratch = cleanScratch(null);
    st.openNotes.clear();
    renderScratch();
    $('scratch-btn').focus();
  });

  // ---------- messages ----------

  function addMessage(role, text) {
    if (role === 'assistant') {
      const m = el('div', 'msg assistant');
      const body = el('div', 'md');
      body.appendChild(MD.render(text, mdOpts));
      m.appendChild(body);
      append(m);
      announce(text);
      return m;
    }
    const m = el('div', 'msg ' + (role === 'user' ? 'user' : 'notice'), text);
    append(m);
    if (role !== 'user') announce(text);
    return m;
  }

  // Streamed text: the settled Markdown blocks are rendered once, the tail once per frame.
  function liveMessage() {
    if (st.live) return st.live;
    const m = el('div', 'msg assistant live');
    const body = el('div', 'md');
    m.appendChild(body);
    append(m);
    st.live = { el: m, body: body, text: '', cut: 0, tail: [], frame: 0 };
    return st.live;
  }

  function paintLive() {
    const l = st.live;
    if (!l) return;
    l.frame = 0;
    const was = nearBottom();
    const cut = MD.stableCut(l.text, l.cut);
    l.tail.forEach(function (n) { n.remove(); });
    if (cut > l.cut) {
      l.body.appendChild(MD.render(l.text.slice(l.cut, cut), mdOpts));
      l.cut = cut;
    }
    const tail = MD.render(l.text.slice(l.cut), mdOpts);
    l.tail = Array.from(tail.childNodes);
    l.body.appendChild(tail);
    follow(was);
  }

  function schedulePaint() {
    const l = st.live;
    if (!l || l.frame) return;
    l.frame = requestAnimationFrame(paintLive);
  }

  // The streamed message is complete: one full render (lists or code split across the settled
  // part and the tail join up again).
  function finishLive(finalText) {
    const l = st.live;
    if (!l) return;
    st.live = null;
    if (l.frame) cancelAnimationFrame(l.frame);
    if (finalText !== undefined) l.text = finalText;
    const was = nearBottom();
    l.el.classList.remove('live');
    if (!l.text.trim()) { l.el.remove(); return; }
    l.body.textContent = '';
    l.body.appendChild(MD.render(l.text, mdOpts));
    follow(was);
    announce(l.text);
  }

  LB.on('agentDelta', function (m) {
    if (typeof m.text !== 'string' || !m.text) return;
    const l = liveMessage();
    l.text += m.text;
    schedulePaint();
  });

  LB.on('agentMessage', function (m) {
    const text = typeof m.text === 'string' ? m.text : '';
    if (m.role === 'assistant' && st.live) {
      // The final answer repeats what was streamed (a plain-text reply): replace, don't repeat.
      const streamed = st.live.text.trim();
      if (!streamed || text.trim().startsWith(streamed)) { finishLive(text); return; }
    }
    finishLive();
    addMessage(m.role, text);
  });

  LB.on('agentError', function (m) {
    finishLive();
    const box = el('div', 'msg error');
    box.setAttribute('role', 'alert');
    box.appendChild(icon('error'));
    box.appendChild(el('span', null, m.message || 'Something went wrong.'));
    append(box);
  });

  // ---------- step log ----------

  const STATUS_TEXT = { running: 'Running', ok: 'Done', error: 'Failed', refused: 'Refused', cancelled: 'Cancelled' };
  const STATUS_ICON = { ok: 'ok', error: 'error', refused: 'refused', cancelled: 'cancelled' };

  // Step screenshots come from the host's own origin (`/agent-shot/…`) or as data: images.
  function safeImage(src) {
    if (typeof src !== 'string') return null;
    if (/^\/(?!\/)[\w\-./%]+$/.test(src)) return src;
    if (/^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/=]+$/.test(src)) return src;
    return null;
  }

  function fillStep(row, s) {
    const status = STATUS_TEXT[s.status] ? s.status : 'running';
    row.className = 'step ' + status;
    const head = row.firstChild;
    const si = head.querySelector('.si');
    si.textContent = '';
    if (status === 'running') si.appendChild(el('span', 'spinner'));
    else si.appendChild(icon(STATUS_ICON[status]));
    si.title = STATUS_TEXT[status];
    head.querySelector('.step-label').textContent = s.label || s.tool || 'Step ' + s.n;
    head.querySelector('.step-sum').textContent = s.summary || '';
    head.setAttribute('aria-label', 'Step ' + s.n + ': ' + (s.label || s.tool || '') + '. ' + STATUS_TEXT[status] +
      (s.summary ? '. ' + s.summary : '') + '. Show details');
    row._step = s;
    if (!row.lastChild.hidden) fillStepBody(row);
  }

  function fillStepBody(row) {
    const s = row._step;
    const body = row.lastChild;
    body.textContent = '';
    if (s.detail) body.appendChild(el('pre', null, s.detail));
    const src = safeImage(s.image);
    if (src) {
      const t = button('thumb', null, function () { openViewer(src); }, 'Enlarge the screenshot of step ' + s.n);
      const img = el('img');
      img.alt = 'Screenshot of step ' + s.n;
      img.src = src;
      t.appendChild(img);
      body.appendChild(t);
    }
    const meta = [s.tool];
    if (s.ms) meta.push(s.ms < 1000 ? s.ms + ' ms' : (s.ms / 1000).toFixed(1) + ' s');
    if (!s.detail && !src) meta.push('no details');
    body.appendChild(el('div', 'step-meta', meta.filter(Boolean).join(' · ')));
  }

  function newStepRow(s) {
    const row = el('div', 'step');
    row.dataset.n = s.n;
    const head = el('button', 'step-head');
    head.type = 'button';
    head.setAttribute('aria-expanded', 'false');
    head.appendChild(el('span', 'si'));
    head.appendChild(el('span', 'step-label'));
    head.appendChild(el('span', 'step-sum'));
    head.appendChild(icon('chevron', 'chev'));
    const body = el('div', 'step-body');
    body.hidden = true;
    head.addEventListener('click', function () {
      const open = body.hidden;
      body.hidden = !open;
      head.setAttribute('aria-expanded', open ? 'true' : 'false');
      if (open) fillStepBody(row);
    });
    row.appendChild(head);
    row.appendChild(body);
    return row;
  }

  LB.on('agentStep', function (m) {
    const s = m.step;
    if (!s || typeof s.n !== 'number') return;
    let row = st.stepRows.get(s.n);
    if (!row) {
      // Text streamed before a tool call is a message of its own.
      finishLive();
      row = newStepRow(s);
      st.stepRows.set(s.n, row);
      fillStep(row, s);
      append(row);
    } else {
      const was = nearBottom();
      fillStep(row, s);
      follow(was);
    }
    if (s.n > st.steps) st.steps = s.n;
    renderStatus();
  });

  // ---------- the enlarged screenshot ----------

  let viewerReturn = null;
  function openViewer(src) {
    viewerReturn = document.activeElement;
    $('viewer-img').src = src;
    $('viewer').hidden = false;
    $('viewer-close').focus();
  }
  function closeViewer() {
    $('viewer').hidden = true;
    $('viewer-img').removeAttribute('src');
    if (viewerReturn && viewerReturn.isConnected) viewerReturn.focus();
    viewerReturn = null;
  }
  $('viewer').addEventListener('click', closeViewer);

  // ---------- cards: ask_user, consent, continue ----------

  // A card for a question: `build(card, answer)` adds its controls; `answer(text)` turns it
  // read-only with the given summary. Open cards close as "Not answered" when the task ends.
  function questionCard(key, cls, iconName, question, build) {
    finishLive();
    const card = el('div', 'card ' + cls);
    card.setAttribute('role', 'group');
    const q = el('div', 'card-q');
    q.appendChild(icon(iconName));
    const qt = el('span', null, question);
    qt.id = 'q-' + key.replace(/[^\w-]/g, '');
    card.setAttribute('aria-labelledby', qt.id);
    q.appendChild(qt);
    card.appendChild(q);
    const controls = el('div', 'controls');
    card.appendChild(controls);
    function close(summary, ok) {
      if (!st.asks.has(key)) return;
      st.asks.delete(key);
      card.classList.add('done');
      const was = nearBottom();
      controls.remove();
      const a = el('div', 'answer');
      if (ok) a.appendChild(icon('check'));
      a.appendChild(el('span', null, summary));
      card.appendChild(a);
      follow(was);
    }
    st.asks.set(key, function (reason) { close(reason || 'Not answered', false); });
    build(controls, function (summary) { close(summary, true); });
    append(card);
    announce(question);
    const first = controls.querySelector('button:not(:disabled), input');
    if (first && document.hasFocus()) first.focus({ preventScroll: true });
    return card;
  }

  LB.on('agentAsk', function (m) {
    const a = m.ask;
    if (!a || typeof a.id !== 'number' || !Array.isArray(a.options)) return;
    questionCard('ask' + a.id, 'ask', 'question', a.question || 'The agent has a question.', function (box, done) {
      const opts = el('div', 'opts');
      box.appendChild(opts);
      let other = null;
      let send = null;
      function submit(choices) {
        const o = other ? other.value.trim() : '';
        if (!choices.length && !o) return;
        const msg = { ask: a.id, choices: choices };
        if (o) msg.other = o;
        LB.post('agentAnswer', msg);
        const said = choices.map(function (i) { return a.options[i]; });
        if (o) said.push('Other: ' + o);
        done('You answered: ' + said.join(', '));
      }
      function checked() {
        return Array.from(opts.querySelectorAll('input:checked')).map(function (c) { return Number(c.value); });
      }
      function update() {
        if (send) send.disabled = !(checked().length || (other && other.value.trim()));
      }
      a.options.forEach(function (text, i) {
        if (a.multi) {
          const l = el('label', 'opt');
          const c = el('input');
          c.type = 'checkbox';
          c.value = String(i);
          c.addEventListener('change', update);
          l.appendChild(c);
          l.appendChild(el('span', null, text));
          opts.appendChild(l);
        } else {
          opts.appendChild(button('opt', text, function () { submit([i]); }));
        }
      });
      if (a.allowOther) {
        const row = el('div', 'other');
        other = el('input');
        other.type = 'text';
        other.maxLength = 2000;
        other.placeholder = 'Other…';
        other.setAttribute('aria-label', 'Other answer');
        other.addEventListener('input', update);
        other.addEventListener('keydown', function (e) {
          if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); if (send && !send.disabled) send.click(); }
        });
        row.appendChild(other);
        if (!a.multi) {
          send = button('btn primary', 'Send', function () { submit([]); });
          row.appendChild(send);
        }
        box.appendChild(row);
      }
      if (a.multi) {
        const acts = el('div', 'acts');
        send = button('btn primary', 'Send', function () { submit(checked()); });
        acts.appendChild(send);
        box.appendChild(acts);
      }
      update();
    });
  });

  LB.on('agentConsentAsk', function (m) {
    if (typeof m.ask !== 'number') return;
    const what = m.label || m.category || 'some of your data';
    // 0.16.0: entering a password is allowed per site; the question names it.
    const password = m.category === 'passwords';
    const site = typeof m.site === 'string' && m.site ? m.site : 'this site';
    const question = password ? 'The agent wants to enter a password on ' + site + '. Allow?' : 'The agent wants to read ' + what + '. Allow?';
    questionCard('consent' + m.ask, 'consent', 'lock', question, function (box, done) {
      const acts = el('div', 'acts');
      acts.appendChild(button('btn primary', 'Allow', function () {
        LB.post('agentConsent', { ask: m.ask, allow: true });
        done(password ? 'Allowed on this site for this conversation' : 'Allowed for this conversation');
      }));
      acts.appendChild(button('btn', 'Deny', function () {
        LB.post('agentConsent', { ask: m.ask, allow: false });
        done('Denied');
      }));
      box.appendChild(acts);
    });
  });

  LB.on('agentContinueAsk', function (m) {
    if (typeof m.ask !== 'number') return;
    questionCard('continue' + m.ask, 'continue', 'steps', 'The agent used ' + (m.steps || 0) + ' steps. Continue?', function (box, done) {
      const acts = el('div', 'acts');
      acts.appendChild(button('btn primary', 'Continue', function () {
        LB.post('agentContinue', { ask: m.ask, go: true });
        done('Continued');
      }));
      acts.appendChild(button('btn', 'Stop', function () {
        LB.post('agentContinue', { ask: m.ask, go: false });
        done('Stopped here');
      }));
      box.appendChild(acts);
    });
  });

  function closeOpenCards(reason) {
    Array.from(st.asks.values()).forEach(function (close) { close(reason); });
  }

  // ---------- script cards ----------

  function fillScript(card, c) {
    card._c = c;
    const what = c.kind === 'macro' ? 'macro' : 'userscript';
    const name = card.querySelector('.script-name');
    name.textContent = '';
    name.appendChild(document.createTextNode((card._updated ? 'Updated ' : 'Created ') + what + ': '));
    name.appendChild(el('b', null, c.name || '(no name)'));
    name.title = c.name || '';
    const badge = card.querySelector('.badge');
    badge.textContent = c.enabled ? 'On' : 'Off';
    badge.classList.toggle('off', !c.enabled);
    const t = card.querySelector('.toggle');
    t.textContent = c.enabled ? 'Disable' : 'Enable';
    t.setAttribute('aria-label', (c.enabled ? 'Disable ' : 'Enable ') + what + ' ' + (c.name || ''));
  }

  LB.on('agentScript', function (m) {
    const c = m.card;
    if (!c || typeof c.id !== 'string') return;
    const key = c.kind + ':' + c.id;
    let card = st.scripts.get(key);
    if (card) { card._updated = true; fillScript(card, c); return; }
    finishLive();
    card = el('div', 'card script');
    const row = el('div', 'script-row');
    row.appendChild(icon(c.kind === 'macro' ? 'macro' : 'script'));
    row.appendChild(el('span', 'script-name'));
    row.appendChild(el('span', 'badge'));
    card.appendChild(row);
    const acts = el('div', 'acts');
    acts.appendChild(button('btn toggle', 'Disable', function () {
      const cur = card._c;
      const enabled = !cur.enabled;
      LB.post('agentToggleScript', { id: cur.id, kind: cur.kind, enabled: enabled });
      fillScript(card, Object.assign({}, cur, { enabled: enabled }));
    }));
    acts.appendChild(button('btn open', 'Open in editor', function () {
      LB.post('agentOpenScript', { id: card._c.id, kind: card._c.kind });
    }));
    card.appendChild(acts);
    st.scripts.set(key, card);
    fillScript(card, c);
    append(card);
    announce(card.querySelector('.script-name').textContent);
  });

  // ---------- password card (0.16.0) ----------
  // A password Krunk generated for the agent's fill_password. The event has no value: the card
  // shows dots, Show asks the host for it (`agentPasswordShow`, the reply is for this panel
  // only) and Hide drops it again; Copy lets the host put it on the clipboard.

  LB.on('agentPassword', function (m) {
    if (typeof m.id !== 'number') return;
    finishLive();
    const site = typeof m.site === 'string' && m.site ? m.site : 'this site';
    const length = Math.max(8, Math.min(32, Number(m.length) || 12));
    const fields = Number(m.fields) || 0;
    const card = el('div', 'card password');
    card.setAttribute('role', 'group');
    const title = el('div', 'card-q');
    title.appendChild(icon('lock'));
    const tt = el('span', null, 'Krunk created a password for ' + site);
    tt.id = 'pw-' + m.id;
    card.setAttribute('aria-labelledby', tt.id);
    title.appendChild(tt);
    card.appendChild(title);
    const row = el('div', 'pw-row');
    const value = el('code', 'pw-value masked');
    const status = el('div', 'pw-status');
    status.setAttribute('role', 'status');
    let shown = false;
    function mask() {
      shown = false;
      value.textContent = '\u2022'.repeat(length);
      value.classList.add('masked');
      value.setAttribute('aria-label', 'Password hidden');
      show.textContent = 'Show';
    }
    const show = button('btn pw-show', 'Show', function () {
      if (shown) { mask(); return; }
      show.disabled = true;
      LB.request('agentPasswordShow', { id: m.id }).then(function (d) {
        show.disabled = false;
        if (!d || typeof d.password !== 'string') { status.textContent = 'The password is no longer kept.'; return; }
        shown = true;
        value.textContent = d.password;
        value.classList.remove('masked');
        value.removeAttribute('aria-label');
        show.textContent = 'Hide';
        status.textContent = '';
      }, function (e) {
        show.disabled = false;
        status.textContent = (e && e.message) || 'The password could not be shown.';
      });
    });
    const copy = button('btn pw-copy', 'Copy', function () {
      LB.request('agentPasswordCopy', { id: m.id }).then(function () {
        status.textContent = 'Copied. Krunk clears the clipboard after 30 seconds.';
      }, function (e) {
        status.textContent = (e && e.message) || 'The password could not be copied.';
      });
    });
    row.appendChild(value);
    row.appendChild(show);
    row.appendChild(copy);
    card.appendChild(row);
    card.appendChild(el('div', 'pw-note', 'Entered into ' + (fields === 1 ? '1 field' : fields + ' fields') +
      '. When you send the form, Krunk saves it with your passwords or asks you. Until then it is only kept here, for this conversation.'));
    card.appendChild(status);
    mask();
    append(card);
    announce(tt.textContent);
  });

  // ---------- run state ----------

  function renderStatus() {
    const b = busy();
    $('status').hidden = !b;
    if (b) {
      $('status-text').textContent = st.state === 'running'
        ? 'Working…' + (st.steps ? ' (step ' + st.steps + ')' : '')
        : 'Waiting for your answer…';
    }
    // While it works, the big Stop replaces Send (the message box stays, disabled).
    $('stop').hidden = !b;
    $('send').hidden = b;
    $('compose').classList.toggle('busy', b);
    input.disabled = b;
    $('send').disabled = b || !input.value.trim();
  }

  LB.on('agentState', function (m) {
    const prev = st.state;
    st.state = m.state;
    if (typeof m.steps === 'number' && m.steps > st.steps) st.steps = m.steps;
    if (m.state !== 'running') finishLive();
    if (!busy()) {
      closeOpenCards('Not answered');
      if (m.state === 'stopped' && prev !== 'stopped') {
        const s = el('div', 'msg stopped', 'Stopped');
        s.setAttribute('role', 'status');
        append(s);
        announce('Stopped');
      }
    }
    renderStatus();
    // The box was disabled while it worked: give it the focus back (Esc works from anywhere).
    if (!busy() && (document.activeElement === document.body || document.activeElement === $('stop') || !document.activeElement)) input.focus();
  });

  LB.on('agentConfig', function (m) {
    st.config = m;
    renderConfig();
  });

  LB.on('agentClear', function () {
    finishLive();
    closeOpenCards('Not answered');
    st.stepRows.clear();
    st.scripts.clear();
    st.asks.clear();
    st.steps = 0;
    st.scratch = cleanScratch(null);
    st.openNotes.clear();
    renderScratch();
    Array.from(log.children).forEach(function (n) { if (n.id !== 'empty') n.remove(); });
    $('empty').hidden = false;
    $('announce').textContent = '';
    renderStatus();
  });

  // ---------- input ----------

  function grow() {
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 6 * 19 + 6) + 'px';
  }

  function send() {
    const text = input.value.trim();
    if (!text || busy()) return;
    LB.post('agentSend', { text: text.slice(0, 20000) });
    input.value = '';
    grow();
    renderStatus();
  }

  $('compose').addEventListener('submit', function (e) { e.preventDefault(); send(); });
  input.addEventListener('input', function () { grow(); $('send').disabled = busy() || !input.value.trim(); });
  input.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
  });
  $('stop').addEventListener('click', function () { LB.post('agentStop'); });

  // Esc closes the screenshot or the model list first; otherwise it stops a running task.
  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Escape' || e.isComposing) return;
    if (!$('viewer').hidden) { e.preventDefault(); closeViewer(); return; }
    if (!$('models').hidden) { e.preventDefault(); openModels(false); $('model').focus(); return; }
    if (busy()) { e.preventDefault(); LB.post('agentStop'); }
  });

  renderConfig();
  renderScratch();
  renderStatus();
  input.focus();
  LB.post('ready');
})();
