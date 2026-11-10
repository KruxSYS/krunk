// Agent panel helpers for the Playwright tests (0.12.0). NOT embedded in the exe (ui/dev).
// Used as a setup function of openPage (it runs before the page's scripts, after mock-host.js):
//   const mockAi = require('./mock-ai.js'); openPage(browser, url, vp, 1, mockAi);
// In the page it adds window.__ai:
//   stream(text, size)  - agentDelta events of `size` characters each
//   config(extra)       - an agentConfig with three models of two providers
//   step(n, status, x)  - an agentStep
function mockAi() {
  'use strict';
  const h = () => window.__host;
  window.__ai = {
    models: [
      { provider: 'p1', providerName: 'OpenRouter', model: 'openai/gpt-4.1-mini' },
      { provider: 'p1', providerName: 'OpenRouter', model: 'anthropic/claude-sonnet-4' },
      { provider: 'p2', providerName: 'Local', model: 'llama3.1:8b' },
    ],
    config: function (extra) {
      h().emit(Object.assign({
        event: 'agentConfig', provider: 'p1', providerName: 'OpenRouter', model: 'openai/gpt-4.1-mini',
        models: window.__ai.models, vision: true, problem: null,
      }, extra || {}));
    },
    stream: function (text, size) {
      const n = size || 7;
      for (let i = 0; i < text.length; i += n) h().emit({ event: 'agentDelta', text: text.slice(i, i + n) });
    },
    step: function (n, status, extra) {
      h().emit({ event: 'agentStep', step: Object.assign({ n: n, tool: 'list_links', label: 'Read the links', status: status, summary: '', detail: '', ms: 0 }, extra || {}) });
    },
    state: function (state, steps) { h().emit({ event: 'agentState', state: state, steps: steps || 0 }); },
  };
}
if (typeof module !== 'undefined' && module.exports) module.exports = mockAi;
else mockAi();
