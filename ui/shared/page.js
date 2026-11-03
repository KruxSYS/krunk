// Helpers shared by the built-in pages.
(function () {
  'use strict';
  const el = LB.el;

  function host(url) {
    try { return new URL(url).hostname; } catch (e) { return ''; }
  }

  // Favicon image from the local cache, falling back to a letter badge.
  function favicon(url, cached) {
    const h = host(url);
    const letter = el('span', { class: 'fav letter', text: (h.replace(/^www\./, '')[0] || '?').toUpperCase() });
    if (!h) return letter;
    const img = el('img', { class: 'fav', alt: '', src: cached || ('favicon?host=' + encodeURIComponent(h)) });
    img.addEventListener('error', function () { img.replaceWith(letter); });
    return img;
  }

  function toast(text) {
    let t = document.querySelector('.toast');
    if (!t) { t = el('div', { class: 'toast', role: 'status' }); document.body.appendChild(t); }
    t.textContent = text;
    t.classList.add('show');
    clearTimeout(t._timer);
    t._timer = setTimeout(function () { t.classList.remove('show'); }, 2500);
  }

  function svg(pathD) {
    const ns = 'http://www.w3.org/2000/svg';
    const s = document.createElementNS(ns, 'svg');
    s.setAttribute('viewBox', '0 0 24 24');
    s.setAttribute('class', 'icon');
    const p = document.createElementNS(ns, 'path');
    p.setAttribute('d', pathD);
    s.appendChild(p);
    return s;
  }

  const ICONS = {
    close: 'M7 7l10 10M17 7L7 17',
    pin: 'M9 3h6l-1 6 3 3H7l3-3zM12 12v9',
    edit: 'M4 20h4L19 9l-4-4L4 16zM14 6l4 4',
    trash: 'M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13',
    folder: 'M3 6h6l2 2h10v11H3z',
    move: 'M5 12h14M13 6l6 6-6 6',
  };

  function iconButton(name, title, onclick) {
    return el('button', { class: 'icon-btn', title: title, 'aria-label': title, onclick: onclick }, [svg(ICONS[name])]);
  }

  function debounce(fn, ms) {
    let t;
    return function () {
      const args = arguments;
      clearTimeout(t);
      t = setTimeout(function () { fn.apply(null, args); }, ms);
    };
  }

  // In-page replacements for confirm()/prompt(): the native dialogs show the
  // internal origin ("lightbrowser.invalid says") and block the page.
  function dialog(message, opts) {
    return new Promise(function (resolve) {
      const input = opts.input ? el('input', { type: 'text', id: 'lb-dialog-input', 'aria-label': message }) : null;
      if (input) input.value = opts.value || '';
      const ok = el('button', { class: 'btn ' + (opts.danger ? 'danger-fill' : 'primary'), id: 'lb-dialog-ok', value: 'ok', text: opts.ok || 'OK' });
      const cancel = el('button', { class: 'btn', id: 'lb-dialog-cancel', value: 'cancel', type: 'button', text: opts.cancel || 'Cancel' });
      const form = el('form', { method: 'dialog' }, [
        el('p', { class: 'lb-dialog-text', text: message }),
        input,
        el('div', { class: 'buttons' }, [cancel, ok]),
      ]);
      const d = el('dialog', { class: 'lb-dialog', role: 'alertdialog', 'aria-label': message }, [form]);
      cancel.addEventListener('click', function () { d.close('cancel'); });
      d.addEventListener('close', function () {
        const value = d.returnValue === 'ok' ? (input ? input.value : true) : (input ? null : false);
        d.remove();
        resolve(value);
      });
      document.body.appendChild(d);
      d.showModal();
      (input || ok).focus();
      if (input) input.select();
    });
  }
  function confirmDialog(message, opts) { return dialog(message, Object.assign({}, opts || {})); }
  function promptDialog(message, value, opts) { return dialog(message, Object.assign({ input: true, value: value }, opts || {})); }

  window.Page = { host: host, favicon: favicon, toast: toast, svg: svg, ICONS: ICONS, iconButton: iconButton, debounce: debounce,
    confirm: confirmDialog, prompt: promptDialog };
})();
