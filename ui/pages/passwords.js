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

  let vaultState = { recovery: null };
  function show(section) {
    ['setup', 'unlock', 'recover', 'list-section'].forEach((s) => { $(s).hidden = s !== section; });
    $('toolbar').hidden = section !== 'list-section';
    const focus = { setup: 'setup-pw', unlock: 'unlock-pw', recover: 'r-phrase' }[section];
    if (focus) setTimeout(() => { if (!$(focus).closest('[hidden]')) $(focus).focus(); }, 0);
  }

  function status() {
    return LB.request('vaultStatus').then(function (s) {
      vaultState = s;
      if (!s.exists) show('setup');
      else if (s.locked && location.hash === '#recover') {
        // From the unlock popup's "Forgot master password?".
        history.replaceState(null, '', location.pathname);
        showRecover();
      } else if (s.locked) { show('unlock'); $('unlock-remain').checked = s.remain !== false; rateLimit(s.waitMs, ''); }
      else {
        show('list-section');
        renderRecovery();
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

  // The host refuses attempts during the delay; this only shows it. Why the last attempt failed
  // ("Wrong master password.") stays until the next attempt or typing (0.7.1: the countdown
  // used to replace it and then clear it, so nothing said the password was wrong).
  let lastError = '';
  function rateLimit(waitMs, message) {
    clearInterval(countdown);
    const btn = $('unlock-btn');
    const err = $('unlock-error');
    if (message) lastError = message.replace(/\s*Try again in \d+ s\.?$/, '');
    err.textContent = lastError;
    let wait = waitMs || 0;
    btn.disabled = wait > 0;
    if (wait <= 0) return;
    const reason = lastError || 'Too many attempts.';
    const tick = () => {
      err.textContent = reason + ' Try again in ' + Math.ceil(wait / 1000) + ' s.';
      wait -= 1000;
      if (wait < 0) { clearInterval(countdown); btn.disabled = false; err.textContent = lastError; }
    };
    tick();
    countdown = setInterval(tick, 1000);
  }

  // Typing a new attempt: the old reason goes (a running countdown stays).
  $('unlock-pw').addEventListener('input', function () {
    lastError = '';
    if (!$('unlock-btn').disabled) $('unlock-error').textContent = '';
  });

  // ---- password fields (0.7.0, R14): show/hide and the strength meter ----
  function setShown(input, shown) {
    input.type = shown ? 'text' : 'password';
    const btn = input.parentNode.querySelector('.reveal');
    if (!btn) return;
    btn.setAttribute('aria-pressed', String(shown));
    btn.setAttribute('aria-label', shown ? 'Hide password' : 'Show password');
    btn.title = btn.getAttribute('aria-label');
  }
  function attachReveal(input, id) {
    const btn = el('button', { type: 'button', class: 'reveal', 'aria-label': 'Show password', title: 'Show password', 'aria-pressed': 'false' });
    if (id) btn.id = id;
    btn.addEventListener('click', function () {
      setShown(input, input.type === 'password');
      input.focus();
    });
    input.parentNode.appendChild(btn);
    return btn;
  }
  function hideAll(ids) {
    ids.forEach(function (id) {
      const i = $(id);
      i.type = 'password';
      const b = i.parentNode.querySelector('.reveal');
      if (b) { b.setAttribute('aria-pressed', 'false'); b.setAttribute('aria-label', 'Show password'); b.title = 'Show password'; }
    });
  }
  ['c-old', 'c-new', 'c-new2', 'r-new', 'r-new2', 'rd-pw', 'rr-pw'].forEach((id) => attachReveal($(id)));
  attachReveal($('e-pw'), 'e-show');
  // An error under a field (0.7.2): the field is marked invalid until it is edited.
  function fieldError(input, errorId, msg) {
    $(errorId).textContent = msg;
    input.setAttribute('aria-invalid', 'true');
  }
  function clearErrors(form) {
    form.querySelectorAll('.field-error, .error').forEach((p) => { p.textContent = ''; });
    form.querySelectorAll('[aria-invalid]').forEach((i) => i.removeAttribute('aria-invalid'));
  }
  ['entry-form', 'card-form'].forEach(function (id) {
    $(id).addEventListener('input', function (ev) {
      const t = ev.target;
      if (!t.hasAttribute('aria-invalid')) return;
      t.removeAttribute('aria-invalid');
      (t.getAttribute('aria-describedby') || '').split(' ').forEach((d) => { if (d && $(d)) $(d).textContent = ''; });
    });
  });
  const STRENGTH = ['Too short', 'Weak', 'Weak', 'Fair', 'Fair', 'Strong', 'Very strong'];
  function meter(input, bar, hint) {
    input.addEventListener('input', function () {
      const v = input.value;
      const s = v.length < 8 ? 0 : strength(v);
      bar.style.width = (v ? Math.max(1, s) / 6 * 100 : 0) + '%';
      bar.className = s >= 5 ? 'good' : s >= 3 ? 'ok' : '';
      if (hint) hint.textContent = v ? 'Strength: ' + STRENGTH[s] + (v.length < 8 ? ' (at least 8 characters)' : '') : 'At least 8 characters. A passphrase of 4–5 random words is strong and memorable.';
    });
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
    lastError = '';
    LB.request('vaultUnlock', { password: pw, remain: $('unlock-remain').checked }).then(function () {
      $('unlock-error').textContent = '';
      status();
    }, function (err) {
      LB.request('vaultStatus').then(function (s) { rateLimit(s.waitMs, err.message); });
    }).finally(function () { $('unlock-btn').textContent = 'Unlock'; if (!countdown) $('unlock-btn').disabled = false; });
  });

  // ---- forgotten master password (0.7.0, R13) ----
  $('forgot').addEventListener('click', showRecover);
  function showRecover() {
    const has = !!vaultState.recovery;
    $('recover-none').hidden = has;
    $('recover-form').hidden = !has;
    ['r-phrase', 'r-new', 'r-new2'].forEach((id) => { $(id).value = ''; });
    ['r-phrase-msg', 'r-pw-error', 'recover-error'].forEach((id) => { $(id).textContent = ''; });
    $('r-phrase-msg').className = 'field-msg';
    show('recover');
    if (!has) setTimeout(() => $('recover-none-back').focus(), 0);
  }
  function backToUnlock() { show('unlock'); }
  $('recover-back').addEventListener('click', backToUnlock);
  $('recover-none-back').addEventListener('click', backToUnlock);
  meter($('r-new'), $('r-meter'), null);
  // The words are checked as they are typed (word list and checksum; nothing is opened).
  let checkTimer = null, phraseOk = false;
  $('r-phrase').addEventListener('input', function () {
    clearTimeout(checkTimer);
    phraseOk = false;
    $('recover-error').textContent = '';
    const text = this.value;
    const words = text.split(/[^A-Za-z0-9]+/).filter((w) => w && !/^\d+$/.test(w)).length;
    $('r-phrase-msg').className = 'field-msg';
    $('r-phrase-msg').textContent = words ? words + ' of 24 words' : '';
    if (words < 24) return;
    checkTimer = setTimeout(function () {
      LB.request('vaultRecoveryCheck', { phrase: text }).then(function (r) {
        if ($('r-phrase').value !== text) return;
        phraseOk = r.ok;
        $('r-phrase-msg').className = 'field-msg ' + (r.ok ? 'ok' : 'bad');
        $('r-phrase-msg').textContent = r.ok ? 'The words are a valid recovery phrase.' : r.message + '.';
      }, function () {});
    }, 250);
  });
  $('recover-form').addEventListener('submit', function (ev) {
    ev.preventDefault();
    const a = $('r-new').value, b = $('r-new2').value;
    $('r-pw-error').textContent = '';
    $('recover-error').textContent = '';
    if (a.length < 8) { $('r-pw-error').textContent = 'Use at least 8 characters.'; $('r-new').focus(); return; }
    if (a !== b) { $('r-pw-error').textContent = 'The passwords do not match.'; $('r-new2').focus(); return; }
    const btn = $('recover-btn');
    btn.disabled = true;
    btn.textContent = 'Opening…';
    LB.request('vaultRecover', { phrase: $('r-phrase').value, new: a }).then(function () {
      ['r-phrase', 'r-new', 'r-new2'].forEach((id) => { $(id).value = ''; });
      hideAll(['r-new', 'r-new2']);
      Page.toast('Vault opened. Your new master password is set.');
      status();
    }, function (err) {
      $('recover-error').textContent = err.message;
    }).finally(function () { btn.disabled = false; btn.textContent = 'Open vault and set password'; });
  });

  // ---- the recovery phrase of an open vault (0.7.0, R13) ----
  const dateFmt = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' });
  function renderRecovery() {
    const has = !!vaultState.recovery;
    $('recovery-state').textContent = has ? 'Recovery phrase made on ' + dateFmt.format(new Date(vaultState.recovery)) : 'No recovery phrase';
    $('recovery-desc').textContent = has
      ? 'If you forget the master password, these 24 words open the vault (on this Windows account). A new phrase replaces this one.'
      : 'If you forget the master password, the saved passwords and cards cannot be recovered. A phrase of 24 words, kept offline, can open them.';
    $('recovery-make').textContent = has ? 'Make a new phrase' : 'Create recovery phrase';
    $('recovery-remove').hidden = !has;
    $('recovery').classList.toggle('missing', !has);
  }
  let asked = [], confirmed = false;
  function rdStep(n) {
    ['rd-step1', 'rd-step2', 'rd-step3'].forEach((id, i) => { $(id).hidden = i + 1 !== n; });
    const focus = { 1: 'rd-pw', 2: 'rd-next2', 3: 'rd-a0' }[n];
    setTimeout(() => { if ($(focus)) $(focus).focus(); }, 0);
  }
  function forgetWords() {
    $('rd-words').textContent = '';
    $('rd-asked').textContent = '';
    document.body.classList.remove('printing-phrase');
  }
  $('recovery-make').addEventListener('click', function () {
    confirmed = false;
    forgetWords();
    $('rd-pw').value = '';
    hideAll(['rd-pw']);
    $('rd-pw-error').textContent = '';
    $('rd-error').textContent = '';
    $('rd-title').textContent = vaultState.recovery ? 'Make a new recovery phrase' : 'Create a recovery phrase';
    rdStep(1);
    $('recovery-dialog').showModal();
  });
  // Closed any way but by confirming: the host forgets the words too.
  $('recovery-dialog').addEventListener('close', function () {
    if (!confirmed) LB.request('vaultRecoveryCancel').catch(function () {});
    forgetWords();
  });
  Array.prototype.forEach.call(document.querySelectorAll('.rd-cancel'), function (b) { b.addEventListener('click', () => $('recovery-dialog').close()); });
  $('rd-step1').addEventListener('submit', function (ev) {
    ev.preventDefault();
    const pw = $('rd-pw').value;
    if (!pw) { $('rd-pw-error').textContent = 'Enter your master password.'; return; }
    $('rd-next1').disabled = true;
    LB.request('vaultRecoveryCreate', { password: pw }).then(function (r) {
      $('rd-pw').value = '';
      $('rd-pw-error').textContent = '';
      const list = $('rd-words');
      list.textContent = '';
      r.words.forEach(function (w) { list.appendChild(el('li', { text: w })); });
      asked = r.confirm;
      const grid = $('rd-asked');
      grid.textContent = '';
      asked.forEach(function (pos, i) {
        grid.appendChild(el('label', { for: 'rd-a' + i, text: 'Word ' + pos }));
        grid.appendChild(el('input', { type: 'text', id: 'rd-a' + i, autocomplete: 'off', spellcheck: 'false', autocapitalize: 'off' }));
      });
      rdStep(2);
    }, function (err) { $('rd-pw-error').textContent = err.message; })
      .finally(function () { $('rd-next1').disabled = false; });
  });
  $('rd-copy').addEventListener('click', function () {
    LB.request('vaultRecoveryCopy').then(() => Page.toast('Phrase copied – the clipboard is cleared in 30 s'), (err) => Page.toast(err.message));
  });
  $('rd-save').addEventListener('click', function () {
    LB.request('vaultRecoverySave').then((r) => { if (r && r.saved) Page.toast('Phrase saved to the file you chose'); }, (err) => Page.toast(err.message));
  });
  $('rd-print').addEventListener('click', function () {
    document.body.classList.add('printing-phrase');
    window.print();
    setTimeout(() => document.body.classList.remove('printing-phrase'), 500);
  });
  $('rd-next2').addEventListener('click', () => rdStep(3));
  $('rd-back').addEventListener('click', () => rdStep(2));
  $('rd-step3').addEventListener('submit', function (ev) {
    ev.preventDefault();
    const answers = asked.map((pos, i) => ({ position: pos, word: $('rd-a' + i).value.trim() }));
    if (answers.some((a) => !a.word)) { $('rd-error').textContent = 'Type each word asked for.'; return; }
    $('rd-confirm').disabled = true;
    LB.request('vaultRecoveryConfirm', { answers: answers }).then(function (r) {
      confirmed = true;
      $('recovery-dialog').close();
      vaultState.recovery = r.created;
      renderRecovery();
      Page.toast('Recovery phrase saved in the vault');
    }, function (err) { $('rd-error').textContent = err.message + '.'; })
      .finally(function () { $('rd-confirm').disabled = false; });
  });
  $('recovery-remove').addEventListener('click', function () {
    $('rr-pw').value = '';
    hideAll(['rr-pw']);
    $('rr-error').textContent = '';
    $('recovery-remove-dialog').showModal();
  });
  $('rr-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    LB.request('vaultRecoveryRemove', { password: $('rr-pw').value }).then(function () {
      $('rr-pw').value = '';
      $('recovery-remove-dialog').close();
      vaultState.recovery = null;
      renderRecovery();
      Page.toast('Recovery phrase removed');
    }, function (err) { $('rr-error').textContent = err.message; });
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
    $('k-number').placeholder = c ? '•••• •••• •••• ' + c.last4 + ' (unchanged)' : '1234 5678 9012 3456';
    $('k-month').value = c ? c.expMonth : '';
    $('k-year').value = c ? c.expYear : '';
    $('k-name').value = c ? c.name : '';
    $('k-nick').value = c ? c.nickname : '';
    $('k-address').value = '';
    $('k-address').placeholder = c && c.hasAddress ? '(unchanged)' : 'Optional';
    clearErrors($('card-form'));
    $('card-dialog').showModal();
    setTimeout(() => $(c ? 'k-nick' : 'k-number').focus(), 0);
  }
  $('card-add').addEventListener('click', () => editCard(null));
  $('card-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    clearErrors($('card-form'));
    const number = $('k-number').value.trim();
    const month = $('k-month').value, year = $('k-year').value;
    let first = null;
    const fail = (input, errorId, msg) => { fieldError($(input), errorId, msg); first = first || input; };
    if (!editingCard && !number) fail('k-number', 'k-number-error', 'Enter the card number.');
    else if (number && !/^[\d -]+$/.test(number)) fail('k-number', 'k-number-error', 'Use digits only.');
    if (month && !(Number(month) >= 1 && Number(month) <= 12)) fail('k-month', 'k-exp-error', 'The month is 1 to 12.');
    else if (year && !(Number(year) >= 2000 && Number(year) <= 2100)) fail('k-year', 'k-exp-error', 'Enter the year with four digits.');
    if (first) { $(first).focus(); return; }
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
    }, function (err) {
      // The host's reasons name the field ("card number", "expiry").
      if (/card number/i.test(err.message)) { fieldError($('k-number'), 'k-number-error', err.message); $('k-number').focus(); }
      else if (/expir/i.test(err.message)) { fieldError($('k-month'), 'k-exp-error', err.message); $('k-month').focus(); }
      else $('k-error').textContent = err.message;
    });
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
    setShown($('e-pw'), false);
    $('e-note').value = '';
    $('e-note').placeholder = e && e.hasNote ? '(unchanged)' : 'Optional';
    clearErrors($('entry-form'));
    $('entry-dialog').showModal();
    setTimeout(() => $(e ? 'e-pw' : 'e-origin').focus(), 0);
  }
  $('add').addEventListener('click', () => edit(null));
  $('e-gen').addEventListener('click', function () {
    LB.request('generatePassword', { length: 20, symbols: true }).then(function (r) {
      $('e-pw').value = r.password;
      setShown($('e-pw'), true);
      $('e-pw').dispatchEvent(new Event('input', { bubbles: true }));
    });
  });
  $('entry-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    let origin = $('e-origin').value.trim();
    if (origin && !/^https?:\/\//i.test(origin)) origin = 'https://' + origin;
    const pw = $('e-pw').value;
    clearErrors($('entry-form'));
    if (!editing && !pw) { fieldError($('e-pw'), 'e-pw-error', 'Enter or generate a password.'); $('e-pw').focus(); return; }
    // Like the password, an empty note field keeps the stored note when editing.
    const entry = { id: editing ? editing.id : null, origin: origin, username: $('e-user').value, password: pw || null, note: editing && $('e-note').value === '' ? null : $('e-note').value };
    LB.request('vaultSave', { entry: entry }).then(function () {
      $('e-pw').value = '';
      $('entry-dialog').close();
      Page.toast('Saved');
      load();
    }, function (err) {
      if (/address|website|origin/i.test(err.message)) { fieldError($('e-origin'), 'e-origin-error', err.message); $('e-origin').focus(); }
      else $('e-error').textContent = err.message;
    });
  });

  // ---- change master password (0.7.0, R14: an error under each field; nothing typed is lost) ----
  meter($('c-new'), $('c-meter'), $('c-strength'));
  const changeErrors = ['c-old-error', 'c-new-error', 'c-new2-error', 'c-error'];
  ['c-old', 'c-new', 'c-new2'].forEach((id) => $(id).addEventListener('input', () => { $(id + '-error').textContent = ''; $('c-error').textContent = ''; }));
  $('change').addEventListener('click', function () {
    ['c-old', 'c-new', 'c-new2'].forEach((id) => { $(id).value = ''; });
    hideAll(['c-old', 'c-new', 'c-new2']);
    changeErrors.forEach((id) => { $(id).textContent = ''; });
    $('c-new').dispatchEvent(new Event('input'));
    $('change-dialog').showModal();
    setTimeout(() => $('c-old').focus(), 0);
  });
  $('change-dialog').addEventListener('close', function () { ['c-old', 'c-new', 'c-new2'].forEach((id) => { $(id).value = ''; }); });
  $('change-form').addEventListener('submit', function (ev) {
    if (ev.submitter && ev.submitter.value === 'cancel') return;
    ev.preventDefault();
    changeErrors.forEach((id) => { $(id).textContent = ''; });
    const old = $('c-old').value, a = $('c-new').value;
    let first = null;
    const fail = (id, msg) => { $(id + '-error').textContent = msg; first = first || id; };
    if (!old) fail('c-old', 'Enter your current master password.');
    if (a.length < 8) fail('c-new', 'Use at least 8 characters.');
    else if (a === old) fail('c-new', 'Choose a password different from the current one.');
    if (a.length >= 8 && a !== $('c-new2').value) fail('c-new2', 'The passwords do not match.');
    if (first) { $(first).focus(); return; }
    const btn = $('c-ok');
    btn.disabled = true;
    btn.textContent = 'Re-encrypting…';
    LB.request('vaultChangePassword', { old: old, new: a }).then(function () {
      $('change-dialog').close();
      Page.toast('Master password changed');
    }, function (err) {
      if (/wrong master password|too many attempts/i.test(err.message)) { $('c-old-error').textContent = err.message; $('c-old').select(); }
      else $('c-error').textContent = err.message;
    }).finally(function () { btn.disabled = false; btn.textContent = 'Change password'; });
  });

  $('lock').addEventListener('click', function () { LB.request('vaultLock').then(status); });
  $('q').addEventListener('input', function () { query = this.value.trim(); render(); });
  LB.on('vaultState', function () { status(); });

  LB.post('ready');
  status();
})();
