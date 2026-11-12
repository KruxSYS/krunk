// Shared by the prompt test pages (p-*.html): shows the page's state in #s, one "k=v" per
// line, and reports every change to the test server (GET /report?page=..&k=v) as evidence.
const PAGE = location.pathname.replace(/^.*\/p-|\.html$/g, '');
const state = {};
function show(changes) {
  Object.assign(state, changes);
  document.getElementById('s').textContent = Object.entries(state).map(([k, v]) => k + '=' + v).join('\n');
  const q = Object.entries(state).map(([k, v]) => encodeURIComponent(k) + '=' + encodeURIComponent(v)).join('&');
  fetch('/report?page=' + PAGE + '&' + q).catch(() => {});
}
// A field with id "f": "open" shows its picker; value and the number of change events are shown.
function field() {
  const f = document.getElementById('f');
  let changes = 0;
  const now = () => ({ value: '[' + f.value + ']', changes });
  f.addEventListener('change', () => { changes++; show(now()); });
  document.getElementById('o').onclick = () => { try { f.showPicker(); } catch (e) { show({ error: e.name }); } };
  document.getElementById('r').onclick = () => show({ ...now(), reported: (state.reported || 0) + 1 });
  show(now());
}
