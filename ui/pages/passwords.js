// Passwords page: vault setup, unlock, list, reveal (needs the master
// password again), copy (clipboard cleared after 30 s by the host), edit,
// delete, generate and change master password.
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);
  let entries = [];
  let query = '';
  let countdown = null;

  function show(section) {
    ['setup', 'unlock', 'list-section'].forEach((s) => { $(s).hidden = s !== section; });
    $('toolbar').hidden = section !== 'list-section';
    const focus = { setup: 'setup-pw', unlock: 'unlock-pw' }[section];
    if (focus) setTimeout(() => $(focus).focus(), 0);
  }

  function status() {
    return LB.request('vaultStatus').then(function (s) {
      if (!s.exists) show('setup');
      else if (s.locked) { show('unlock'); rateLimit(s.waitMs, ''); }
      else {
        show('list-section');
        load();
        // Linked from Settings → "Change master password…".
        if (location.hash === '#change') { history.replaceState(null, '', location.pathname); $('change').click(); }
        // Linked from the "Password saved" toast (0.6.0): show that entry.
        if (location.hash.indexOf('#entry=') === 0) {
          const id = decodeURIComponent(location.hash.slice(7));
          history.replaceState(null, '', location.pathname);
          let tries = 0;
          const find = setInterval(function () {
            const row = document.querySelector('.pw-row[data-id="' + id.replace(/"/g, '') + '"]');
            if (row) { row.classList.add('highlight'); row.scrollIntoView({ block: 'center' }); }
            if (row || ++tries > 20) clearInterval(find);
          }, 100);
        }
      }
      return s;
    });
  }

  // The host refuses attempts during the delay; this only shows it.
  function rateLimit(waitMs, message) {
    clearInterval(countdown);
    const btn = $('unlock-btn');
    const err = $('unlock-error');
    err.textContent = message || '';
    let wait = waitMs || 0;
    btn.disabled = wait > 0;
    if (wait <= 0) return;
    const tick = () => {
      err.textContent = 'Too many attempts. Try again in ' + Math.ceil(wait / 1000) + ' s.';
      wait -= 1000;
      if (wait < 0) { clearInterval(countdown); btn.disabled = false; err.textContent = ''; }
    };
    tick();
    countdown = setInterval(tick, 1000);
  }

  // ---- setup ----
  function strength(pw) {
    let score = Math.min(4, Math.floor(pw.length / 5));
    if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++;
    if (/\d/.test(pw)) score++;
    if (/[^A-Za-z0-9]/.test(pw)) score++;
    return Math.min(score, 6);
  }
  ['setup-pw', 'setup-pw2'].forEach((id) => $(id).addEventListener('input', () => { $('setup-error').textContent = ''; }));
  $('setup-pw').addEventListener('input', function () {
    const s = strength(this.value);
    $('meter').style.width = (s / 6 * 100) + '%';
    $('meter').className = s >= 5 ? 'good' : s >= 3 ? 'ok' : '';
  });
  $('setup-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const a = $('setup-pw').value;
    const b = $('setup-pw2').value;
    if (a.length < 8) { $('setup-error').textContent = 'Use at least 8 characters.'; return; }
    if (a !== b) { $('setup-error').textContent = 'The passwords do not match.'; return; }
    $('setup-btn').disabled = true;
    $('setup-btn').textContent = 'Creating…';
    LB.request('vaultSetup', { password: a }).then(function () {
      $('setup-pw').value = ''; $('setup-pw2').value = '';
      Page.toast('Vault created');
      status();
    }, function (err) { $('setup-error').textContent = err.message; })
      .finally(function () { $('setup-btn').disabled = false; $('setup-btn').textContent = 'Create vault'; });
  });

  // ---- unlock ----
  $('unlock-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const pw = $('unlock-pw').value;
    if (!pw || $('unlock-btn').disabled) return;
    $('unlock-btn').disabled = true;
    $('unlock-btn').textContent = 'Unlocking…';
    $('unlock-pw').value = '';
    LB.request('vaultUnlock', { password: pw }).then(function () {
      $('unlock-error').textContent = '';
      status();
    }, function (err) {
      LB.request('vaultStatus').then(function (s) { rateLimit(s.waitMs, err.message); });
    }).finally(function () { $('unlock-btn').textContent = 'Unlock'; if (!countdown) $('unlock-btn').disabled = false; });
  });

  // ---- list ----
  function load() {
    LB.request('vaultList').then(function (list) { entries = list; render(); }, function () { status(); });
    LB.request('getNeverSave').then(renderNever, function () {});
    loadCards();
  }

  function render() {
    const box = $('list');
    box.textContent = '';
    const q = query.toLowerCase();
    const shown = entries.filter((e) => !q || (e.origin + ' ' + e.username).toLowerCase().includes(q));
    if (!shown.length) {
      box.appendChild(el('div', { class: 'empty', text: entries.length ? 'No matches.' : 'No saved passwords yet. Sign in to a site and choose “Save” when asked, or click Add.' }));
      return;
    }
    shown.forEach(function (e) {
      const secret = el('span', { class: 'secret', text: '••••••••••' });
      const note = el('span', { class: 'note', hidden: true });
      box.appendChild(el('div', { class: 'row pw-row', 'data-id': e.id }, [
        Page.favicon(e.origin),
        el('span', { class: 'main' }, [
          el('a', { href: e.origin, text: Page.host(e.origin) || e.origin, title: e.origin }),
          el('span', { class: 'user', text: e.username || '(no username)' }),
          note,
        ]),
        secret,
        el('span', { class: 'actions' }, [
          el('button', { class: 'btn small', text: 'Show', onclick: () => reveal(e, secret, note) }),
          el('button', { class: 'btn small', text: 'Copy user', onclick: () => copy(e, 'username') }),
          el('button', { class: 'btn small', text: 'Copy password', onclick: () => copy(e, 'password') }),
          Page.iconButton('edit', 'Edit', () => edit(e)),
          Page.iconButton('trash', 'Delete', () => remove(e)),
        ]),
      ]));
    });
  }

  // ---- payment methods (0.6.0) ----
  let cards = [];
  function loadCards() {
    LB.request('cardList').then(function (list) { cards = list; renderCards(); }, function () {});
  }
  function renderCards() {
    const box = $('cards');
    box.textContent = '';
    if (!cards.length) { box.appendChild(el('div', { class: 'empty', text: 'No payment methods yet. Krunk offers to save a card after a checkout, or click Add card.' })); return; }
    cards.forEach(function (c) {
      const secret = el('span', { class: 'secret', text: '•••• •••• •••• ' + c.last4 });
      const addr = el('span', { class: 'note', hidden: true });
      box.appendChild(el('div', { class: 'row pw-row card-row', 'data-id': c.id }, [
        el('span', { class: 'brand', text: '💳' }),
        el('span', { class: 'main' }, [
          el('span', { class: 'name', text: (c.nickname ? c.nickname + ' · ' : '') + c.brand }),
          el('span', { class: 'user', text: (c.name || '(no name)') + ' · expires ' + String(c.expMonth).padStart(2, '0') + '/' + c.expYear }),
          addr,
        ]),
        secret,
        el('span', { class: 'actions' }, [
          el('button', { class: 'btn small', text: 'Show', onclick: () => revealCard(c, secret, addr) }),
          el('button', { class: 'btn small', text: 'Copy number', onclick: () => copyCard(c) }),
          Page.iconButton('edit', 'Edit', () => editCard(c)),
          Page.iconButton('trash', 'Delete', () => removeCard(c)),
        ]),
      ]));
    });
  }
  function copyCard(c) {
    LB.request('cardCopy', { id: c.id }).then(function () { Page.toast('Card number copied – the clipboard is cleared in 30 s'); }, (err) => Page.toast(err.message));
  }
  function removeCard(c) {
    Page.confirm('Delete the ' + c.brand + ' card ending in ' + c.last4 + '?', { ok: 'Delete', danger: true })
      .then(function (yes) { if (yes) LB.request('cardDelete', { id: c.id }).then(loadCards, (err) => Page.toast(err.message)); });
  }
  let cardRevealTarget = null;
  function revealCard(c, secretEl, addrEl) {
    cardRevealTarget = { c: c, el: secretEl, addr: addrEl };
    revealTarget = null;
    $('reveal-error').textContent = '';
    $('reveal-pw').value = '';
    $('reveal-dialog').showModal();
  }
  let editingCard = null;
  function editCard(c) {
    editingCard = c || null;
    $('card-title').textContent = c ? 'Edit card' : 'Add card';
    $('k-number').value = '';
    $('k-number').placeholder = c ? '•••• •••• •••• ' + c.last4 + ' (unchanged)' : '';
    $('k-month').value = c ? c.expMonth : '';
    $('k-year').value = c ? c.expYear : '';
    $('k-name').value = c ? c.name : '';
    $('k-nick').value = c ? c.nickname : '';
    $('k-address').value = '';
    $('k-address').placeholder = c && c.hasAddress ? '(unchanged)' : '';
    $('k-error').textContent = '';
    $('card-dialog').showModal();
  }
  $('card-add').addEventListener('click', () => editCard(null));
  $('card-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    const number = $('k-number').value.trim();
    if (!editingCard && !number) { $('k-error').textContent = 'Enter the card number.'; return; }
    const card = {
      id: editingCard ? editingCard.id : null,
      number: number || null,
      expMonth: Number($('k-month').value) || 0,
      expYear: Number($('k-year').value) || 0,
      name: $('k-name').value.trim(),
      nickname: $('k-nick').value.trim(),
      address: editingCard && $('k-address').value === '' ? null : $('k-address').value,
    };
    LB.request('cardSave', { card: card }).then(function () {
      $('k-number').value = '';
      $('card-dialog').close();
      Page.toast('Card saved');
      loadCards();
    }, function (err) { $('k-error').textContent = err.message; });
  });

  function renderNever(list) {
    const box = $('never');
    box.textContent = '';
    if (!list.length) { box.appendChild(el('div', { class: 'empty', text: 'Sites where you chose “Never” appear here.' })); return; }
    list.forEach(function (origin) {
      box.appendChild(el('div', { class: 'row' }, [
        el('span', { class: 'main' }, [el('span', { class: 'name', text: origin })]),
        el('span', { class: 'actions' }, [Page.iconButton('close', 'Remove', function () {
          LB.request('neverSaveRemove', { origin: origin }).then(load);
        })]),
      ]));
    });
  }

  function copy(e, field) {
    LB.request('vaultCopy', { id: e.id, field: field }).then(function () {
      Page.toast(field === 'password' ? 'Password copied – the clipboard is cleared in 30 s' : 'Username copied');
    }, (err) => Page.toast(err.message));
  }

  function remove(e) {
    Page.confirm('Delete the saved login for ' + (e.username || '(no username)') + ' on ' + e.origin + '?', { ok: 'Delete', danger: true })
      .then(function (yes) { if (yes) LB.request('vaultDelete', { id: e.id }).then(load, (err) => Page.toast(err.message)); });
  }

  // Revealing needs the master password again; the value is shown for 20 s.
  let revealTarget = null;
  function reveal(e, secretEl, noteEl) {
    revealTarget = { e: e, el: secretEl, note: noteEl };
    $('reveal-error').textContent = '';
    $('reveal-pw').value = '';
    $('reveal-dialog').showModal();
  }
  $('reveal-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    const pw = $('reveal-pw').value;
    $('reveal-pw').value = '';
    if (cardRevealTarget) {
      const k = cardRevealTarget;
      LB.request('cardReveal', { id: k.c.id, password: pw }).then(function (r) {
        $('reveal-dialog').close();
        k.el.textContent = r.number.replace(/(\d{4})(?=\d)/g, '$1 ');
        k.el.classList.add('shown');
        if (r.address) { k.addr.textContent = 'Address: ' + r.address; k.addr.hidden = false; }
        setTimeout(function () { k.el.textContent = '•••• •••• •••• ' + k.c.last4; k.el.classList.remove('shown'); k.addr.textContent = ''; k.addr.hidden = true; }, 20000);
        cardRevealTarget = null;
      }, function (err) { $('reveal-error').textContent = err.message; });
      return;
    }
    const t = revealTarget;
    LB.request('vaultReveal', { id: t.e.id, password: pw }).then(function (r) {
      $('reveal-dialog').close();
      t.el.textContent = r.password;
      t.el.classList.add('shown');
      // The note is shown with the password, behind the same re-authentication.
      if (t.note && r.note) { t.note.textContent = 'Note: ' + r.note; t.note.hidden = false; }
      setTimeout(function () {
        t.el.textContent = '••••••••••'; t.el.classList.remove('shown');
        if (t.note) { t.note.textContent = ''; t.note.hidden = true; }
      }, 20000);
    }, function (err) { $('reveal-error').textContent = err.message; });
  });

  // ---- add / edit ----
  let editing = null;
  function edit(e) {
    editing = e || null;
    $('entry-title').textContent = e ? 'Edit login' : 'Add login';
    $('e-origin').value = e ? e.origin : '';
    $('e-user').value = e ? e.username : '';
    $('e-pw').value = '';
    $('e-pw').placeholder = e ? '(unchanged)' : '';
    $('e-pw').type = 'password';
    $('e-note').value = '';
    $('e-note').placeholder = e && e.hasNote ? '(unchanged)' : '';
    $('e-error').textContent = '';
    $('entry-dialog').showModal();
  }
  $('add').addEventListener('click', () => edit(null));
  $('e-show').addEventListener('click', function () { $('e-pw').type = $('e-pw').type === 'password' ? 'text' : 'password'; });
  $('e-gen').addEventListener('click', function () {
    LB.request('generatePassword', { length: 20, symbols: true }).then(function (r) {
      $('e-pw').value = r.password;
      $('e-pw').type = 'text';
    });
  });
  $('entry-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    let origin = $('e-origin').value.trim();
    if (origin && !/^https?:\/\//i.test(origin)) origin = 'https://' + origin;
    const pw = $('e-pw').value;
    if (!editing && !pw) { $('e-error').textContent = 'Enter or generate a password.'; return; }
    // Like the password, an empty note field keeps the stored note when editing.
    const entry = { id: editing ? editing.id : null, origin: origin, username: $('e-user').value, password: pw || null, note: editing && $('e-note').value === '' ? null : $('e-note').value };
    LB.request('vaultSave', { entry: entry }).then(function () {
      $('e-pw').value = '';
      $('entry-dialog').close();
      Page.toast('Saved');
      load();
    }, function (err) { $('e-error').textContent = err.message; });
  });

  // ---- change master password ----
  $('change').addEventListener('click', function () {
    ['c-old', 'c-new', 'c-new2'].forEach((id) => { $(id).value = ''; });
    $('c-error').textContent = '';
    $('change-dialog').showModal();
  });
  $('change-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    const a = $('c-new').value;
    if (a.length < 8) { $('c-error').textContent = 'Use at least 8 characters.'; return; }
    if (a !== $('c-new2').value) { $('c-error').textContent = 'The new passwords do not match.'; return; }
    const old = $('c-old').value;
    ['c-old', 'c-new', 'c-new2'].forEach((id) => { $(id).value = ''; });
    $('c-error').textContent = 'Re-encrypting…';
    LB.request('vaultChangePassword', { old: old, new: a }).then(function () {
      $('change-dialog').close();
      Page.toast('Master password changed');
    }, function (err) { $('c-error').textContent = err.message; });
  });

  $('lock').addEventListener('click', function () { LB.request('vaultLock').then(status); });
  $('q').addEventListener('input', function () { query = this.value.trim(); render(); });
  LB.on('vaultState', function () { status(); });

  LB.post('ready');
  status();
})();
