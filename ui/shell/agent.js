// The AI agent panel (0.12.0). A placeholder for the spine; WP-C builds the real panel.
(function () {
  'use strict';
  const $ = function (id) { return document.getElementById(id); };
  let live = null;
  function add(cls, text) {
    const p = document.createElement('p');
    p.className = 'msg ' + cls;
    p.textContent = text;
    $('log').appendChild(p);
    $('log').scrollTop = $('log').scrollHeight;
    return p;
  }
  $('ask').addEventListener('submit', function (e) {
    e.preventDefault();
    const text = $('input').value.trim();
    if (!text) return;
    $('input').value = '';
    LB.post('agentSend', { text: text });
  });
  $('input').addEventListener('keydown', function (e) {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); $('ask').requestSubmit(); }
  });
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape') LB.post('agentStop'); });
  $('stop').addEventListener('click', function () { LB.post('agentStop'); });
  $('new').addEventListener('click', function () { LB.post('agentNew'); });
  $('close').addEventListener('click', function () { LB.post('agentClose'); });
  LB.on('agentConfig', function (m) { $('model').textContent = m.problem || (m.providerName + ' · ' + m.model); });
  LB.on('agentMessage', function (m) { live = null; add(m.role, m.text); });
  LB.on('agentDelta', function (m) { if (!live) live = add('assistant', ''); live.textContent += m.text; });
  LB.on('agentState', function (m) {
    const running = m.state === 'running' || m.state === 'asking';
    $('stop').hidden = !running;
    $('state').hidden = m.state === 'idle';
    $('state').textContent = m.state === 'stopped' ? 'Stopped' : m.state;
    if (!running) live = null;
  });
  LB.on('agentClear', function () { $('log').textContent = ''; live = null; });
  LB.post('ready');
})();
