// A colour picker for Krunk's pages (0.7.0, R3): Windows' colour dialog through
// <input type="color">, the hex field, optional preset swatches and "Reset to default".
// It wraps an existing text field (the value the page saves): every pick goes into that field
// and fires its `input` and `change` events, live while dragging (at most every `liveMs`).
//
//   ColorPicker.attach(textInput, { presets: ['#2563eb', …], liveMs: 120, resetLabel: 'Reset' })
(function () {
  'use strict';
  const HEX = /^#[0-9a-fA-F]{6}$/;

  function attach(input, options) {
    const opts = options || {};
    const el = LB.el;
    const wrap = el('span', { class: 'color-picker' });
    input.parentNode.insertBefore(wrap, input);
    const swatches = el('span', { class: 'swatches', role: 'group', 'aria-label': (opts.label || 'Colour') + ' presets' });
    (opts.presets || []).forEach(function (c) {
      const b = el('button', { type: 'button', class: 'swatch', title: c || 'Default', 'aria-label': c || 'Default', 'data-color': c });
      if (c) b.style.setProperty('--c', c);
      else b.classList.add('default');
      b.addEventListener('click', function () { set(c, true); });
      swatches.appendChild(b);
    });
    const picker = el('input', { type: 'color', class: 'color-input', 'aria-label': (opts.label || 'Colour') + ': custom colour' });
    const reset = el('button', { type: 'button', class: 'btn small reset-color', text: opts.resetLabel || 'Reset to default' });
    if (opts.presets && opts.presets.length) wrap.appendChild(swatches);
    wrap.appendChild(picker);
    wrap.appendChild(input);
    wrap.appendChild(reset);

    let timer = null, pending = null;
    function fire(final) {
      input.dispatchEvent(new Event('input', { bubbles: true }));
      if (final) input.dispatchEvent(new Event('change', { bubbles: true }));
    }
    function set(value, final) {
      input.value = value;
      sync();
      if (final) { clearTimeout(timer); timer = null; pending = null; fire(true); return; }
      // Live while the dialog's colour moves: a change at most every liveMs.
      pending = value;
      if (!timer) {
        timer = setTimeout(function () { timer = null; if (pending !== null) { pending = null; fire(true); } }, opts.liveMs || 120);
      }
    }
    function sync() {
      const v = input.value.trim();
      if (HEX.test(v)) picker.value = v.toLowerCase();
      wrap.classList.toggle('is-default', !v);
      swatches.querySelectorAll('.swatch').forEach(function (b) { b.classList.toggle('on', (b.dataset.color || '') === v.toLowerCase()); });
      reset.disabled = !v;
    }
    picker.addEventListener('input', function () { set(picker.value, false); });
    picker.addEventListener('change', function () { set(picker.value, true); });
    reset.addEventListener('click', function () { set('', true); });
    input.addEventListener('input', sync);
    input.addEventListener('change', sync);
    sync();
    return { sync: sync, wrap: wrap };
  }

  window.ColorPicker = { attach: attach };
})();
