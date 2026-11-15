// krunk://sync (0.9.0, beta): turn sync on, set up the first device (passphrase and recovery
// key), restore or ask another device, answer devices that ask, the device list, turn off. The
// host decides everything and sends the `sync` view; this page renders it and posts the user's
// choices. Nothing here talks to the network.
(function () {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const el = LB.el;
  const VIEWS = ['private', 'noaccount', 'off', 'working', 'setup', 'join', 'restore', 'joining', 'alarm', 'on'];
  let view = null;
  let vault = { exists: false, locked: true };

  function show(state) {
    VIEWS.forEach(function (v) { $('v-' + v).hidden = v !== state; });
  }
  function setText(id, text) { const n = $(id); n.textContent = text || ''; n.hidden = !text; }

  function ago(ms) {
    if (!ms) return '';
    const s = Math.max(0, (Date.now() - ms) / 1000);
    if (s < 90) return 'just now';
    if (s < 3600) return Math.round(s / 60) + ' minutes ago';
    if (s < 86400) { const h = Math.round(s / 3600); return h === 1 ? '1 hour ago' : h + ' hours ago'; }
    return new Date(ms).toLocaleDateString();
  }

  function render() {
    const v = view;
    if (!v) return;
    const state = { private: 'private', noAccount: 'noaccount', off: 'off', working: 'working', setup: 'setup', join: 'join',
      restoreConfirm: 'restore', joining: 'joining', alarm: 'alarm', on: 'on' }[v.state] || 'off';
    const shown = VIEWS.find(function (x) { return !$('v-' + x).hidden; });
    show(state);
    if (v.state === 'off') {
      const canPw = vault.exists && !vault.locked;
      $('off-passwords').disabled = !canPw;
      $('off-passwords').checked = canPw;
      $('off-passwords-hint').textContent = !vault.exists ? '(set up a password vault first, in Passwords)'
        : vault.locked ? '(unlock your password vault first)' : '';
      const m = $('off-message-p');
      if (m) m.remove();
      if (v.message) $('v-off').insertBefore(el('p', { class: 'status', id: 'off-message-p', role: 'status', text: v.message }), $('v-off').querySelector('h2').nextSibling);
    }
    if (v.state === 'working') $('working-text').textContent = v.message;
    if (v.state === 'setup') {
      $('setup-novault').hidden = vault.exists;
      $('setup-form').hidden = !vault.exists;
      setText('setup-error', v.message);
    }
    if (v.state === 'join') {
      setText('join-error', v.message);
      $('restore-secret').value = '';
      if (shown !== 'join') $('restore-secret').focus();
      $('restore-vault-hint').textContent = !vault.exists
        ? 'Your passwords come along once this computer has a password vault: create it on the Passwords page with the same password.'
        : vault.locked ? 'Unlock your password vault first so your passwords come along too.'
          : 'Your passwords come into this computer\'s vault.';
    }
    if (v.state === 'restoreConfirm') {
      const list = $('restore-devices');
      list.textContent = '';
      v.devices.forEach(function (d) { list.appendChild(el('li', { text: d })); });
      $('restore-revoked').textContent = v.revoked ? v.revoked + ' removed device(s) no longer have access.' : '';
      // The next step is this button: focused, so Enter confirms (keyboard users).
      if (shown !== 'restore') $('restore-confirm').focus();
    }
    if (v.state === 'joining') {
      $('code-form').hidden = !v.enterCode;
      $('joining-wait').hidden = v.enterCode;
      $('joining-message').textContent = v.message || '';
      if (v.enterCode && document.activeElement !== $('code')) { $('code').value = ''; $('code').focus(); }
    }
    if (v.state === 'alarm') $('alarm-text').textContent = v.message;
    if (v.state === 'on') renderOn(v);
  }

  function renderOn(v) {
    $('on-status').textContent = v.status;
    $('on-message').textContent = v.message || '';
    $('on-passwords').textContent = v.passwords ? 'Passwords sync while your password vault is unlocked.'
      : v.passwordsLocked ? 'Passwords: unlock your password vault to include them.' : 'Passwords are not synced on this device.';
    $('on-rotation').hidden = !v.rotationPending;
    $('on-last').textContent = v.lastSyncMs ? 'Last synced ' + ago(v.lastSyncMs) : '';
    // A rotation waiting for the user, and "Finish securing" after one.
    $('confirm-rotation').hidden = !v.confirmDevices.length;
    const cl0 = $('confirm-list');
    cl0.textContent = '';
    v.confirmDevices.forEach(function (n) { cl0.appendChild(el('li', { text: n })); });
    $('secure').hidden = !v.secureNeeded;
    $('sync-now').disabled = !!v.syncing;
    $('sync-now').setAttribute('aria-busy', v.syncing ? 'true' : 'false');
    $('sync-now').textContent = v.syncing ? 'Syncing…' : 'Sync now';
    // Collections.
    const cl = $('collections');
    cl.textContent = '';
    v.collections.forEach(function (c) {
      const box = el('input', { type: 'checkbox', id: 'coll-' + c.name });
      box.checked = c.on;
      box.addEventListener('change', function () { LB.post('syncSetCollection', { name: c.name, on: box.checked }); });
      cl.appendChild(el('label', { class: 'check' }, [box, el('span', { text: c.label })]));
    });
    // Other devices' open tabs (read-only).
    const rl = $('remote-list');
    rl.textContent = '';
    $('remote').hidden = !v.remoteTabs.length;
    v.remoteTabs.forEach(function (d) {
      rl.appendChild(el('h3', { text: d.device + (d.updatedMs ? ' · ' + ago(d.updatedMs) : '') }));
      const ul = el('ul', { class: 'plain tabs' });
      d.tabs.forEach(function (t) {
        const a = el('a', { href: t[1], text: t[0] || t[1], title: t[1] });
        a.addEventListener('click', function (e) { e.preventDefault(); LB.post('openUrl', { url: t[1], newTab: true }); });
        ul.appendChild(el('li', null, [a]));
      });
      if (!d.tabs.length) ul.appendChild(el('li', { class: 'hint', text: 'No open tabs.' }));
      rl.appendChild(ul);
    });
    // Requests to join.
    const list = $('approval-list');
    list.textContent = '';
    $('approvals').hidden = !v.approvals.length || !!v.approving;
    // The share choice is for new devices; a device that syncs asks for the passwords anyway.
    $('share-row').hidden = !v.passwords || v.approvals.every(function (a) { return a.passwordsOnly; });
    v.approvals.forEach(function (a) {
      list.appendChild(el('li', { class: 'approval' }, [
        el('span', { class: 'who', text: a.label + (a.country ? ' · ' + a.country : '') +
          (a.passwordsOnly ? ' asks for your passwords' : '') }),
        el('button', { class: 'btn primary', text: 'Approve', onclick: function () {
          LB.post('syncApprove', { id: a.id, sharePasswords: $('share-passwords').checked });
        } }),
      ]));
    });
    // The approval in progress.
    const ap = v.approving;
    $('approving').hidden = !ap;
    if (ap) {
      if (ap.done) {
        $('approving-title').textContent = ap.label + ' is approved';
        $('approving-text').textContent = ap.passwordsOnly ? 'It now has your passwords key: its passwords sync.'
          : 'It now has your sync keys.';
        $('approving-code').textContent = '';
        $('approving-cancel').textContent = 'Close';
      } else {
        $('approving-title').textContent = (ap.passwordsOnly ? 'Give your passwords to ' : 'Approve ') + ap.label;
        $('approving-text').textContent = ap.code ? 'Type this code on the ' + (ap.passwordsOnly ? 'other' : 'new') +
          ' device. Only continue if it asked just now.' : 'Waiting for the ' + (ap.passwordsOnly ? 'other' : 'new') + ' device…';
        $('approving-code').textContent = ap.code || '';
        $('approving-cancel').textContent = 'Cancel';
      }
    }
    // Devices.
    const dl = $('device-list');
    dl.textContent = '';
    v.devices.forEach(function (d) {
      const meta = [d.platform === 'android' ? 'Android' : 'Windows'];
      meta.push(d.keys ? 'syncs' : 'linked, no keys yet');
      if (d.lastSync) meta.push('last synced ' + ago(Date.parse(d.lastSync)));
      dl.appendChild(el('li', { class: 'device' }, [
        el('div', { class: 'name' }, [d.name, d.me ? el('span', { class: 'badge', text: 'This device' }) : null]),
        el('div', { class: 'meta', text: meta.join(' · ') }),
        el('div', { class: 'device-acts' }, [
          el('button', { class: 'btn small', text: 'Rename', 'aria-label': 'Rename ' + d.name, onclick: function () {
            Page.prompt('Device name', d.name, { ok: 'Rename' }).then(function (name) {
              if (name && name.trim()) LB.post('syncRenameDevice', { id: d.id, name: name.trim().slice(0, 64) });
            });
          } }),
          d.me ? null : el('button', { class: 'btn danger small', text: 'Remove', 'aria-label': 'Remove ' + d.name, onclick: function () {
            Page.confirm('Remove ' + d.name + '? It is signed out at once and loses access to your synced data.', { ok: 'Remove', danger: true })
              .then(function (ok) { if (ok) LB.post('syncRemoveDevice', { id: d.id }); });
          } }),
        ]),
      ]));
    });
  }

  LB.on('sync', function (m) { view = m.view; render(); });
  LB.on('vaultState', function (m) { vault = { exists: m.exists, locked: m.locked }; render(); });

  $('go-account').addEventListener('click', function () { LB.post('openPage', { page: 'settings', section: 'account' }); });
  $('turn-on').addEventListener('click', function () { LB.post('syncTurnOn', { passwords: $('off-passwords').checked }); });
  // 0.21.1: the vault password is the one secret (first device and joining alike).
  $('setup-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const pw = $('setup-vault-pw').value;
    if (!pw) { $('setup-vault-pw').focus(); return; }
    $('setup-vault-pw').value = '';
    LB.post('syncSetupVault', { password: pw });
  });
  $('setup-cancel').addEventListener('click', function () { LB.post('syncCancel'); });
  $('restore-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const secret = $('restore-secret').value;
    if (!secret) { $('restore-secret').focus(); return; }
    $('restore-secret').value = '';
    LB.post('syncRestore', { secret: secret, vault: true });
  });
  $('join-cancel').addEventListener('click', function () { LB.post('syncCancel'); });
  $('restore-confirm').addEventListener('click', function () { LB.post('syncRestoreConfirm'); });
  $('restore-cancel').addEventListener('click', function () { LB.post('syncCancel'); });
  $('code-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const code = $('code').value.replace(/\D/g, '');
    if (code.length !== 6) { $('code').focus(); return; }
    LB.post('syncJoinCode', { code: code });
  });
  $('joining-cancel').addEventListener('click', function () { LB.post('syncCancel'); });
  $('approving-cancel').addEventListener('click', function () { LB.post('syncApproveCancel'); });
  $('sync-now').addEventListener('click', function () { LB.post('syncNow'); });
  $('confirm-go').addEventListener('click', function () { LB.post('syncConfirmRotation'); });
  $('alarm-off').addEventListener('click', function () { LB.post('syncTurnOff', { deleteData: false }); });
  $('turn-off').addEventListener('click', function () {
    const del = $('off-delete').checked;
    Page.confirm(del ? 'Turn off sync and remove the synced data from this device?' : 'Turn off sync on this device? Your other devices keep syncing.',
      { ok: 'Turn off', danger: true }).then(function (ok) { if (ok) LB.post('syncTurnOff', { deleteData: del }); });
  });

  LB.request('vaultStatus').then(function (st) { vault = { exists: st.exists, locked: st.locked }; render(); }, function () {});
  LB.post('syncState');
})();
