// Krunk 0.12.0: Settings → AI and scripts, on the mock host. Hooked into test-ui.cjs
// (ONLY=AiSettings). The host side (keys, provider requests) is mocked here: the page must send
// the right commands, never keep a key, and show what the host answers.
'use strict';
const fs = require('fs');
const path = require('path');

// The settings page's host, cut down to what the page needs, plus the ai* commands. Like the
// real host, saveSettings keeps its own provider list (providers change through aiProviderSave).
function aiSettingsMock(defaults) {
  const h = window.__host.handlers;
  const clone = (x) => JSON.parse(JSON.stringify(x));
  const st = { settings: clone(defaults), keys: {}, nextId: 0xa1b2c300 };
  window.__aiSt = st;
  h.getSettings = () => ({ settings: st.settings, restartRequired: false, dnsPresets: [], wallpapers: [], userAgents: [], defaultUserAgent: 'Mozilla/5.0',
    about: { version: '0.12.0', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk' } });
  h.saveSettings = (m) => {
    const s = m.settings;
    s.ai.providers = clone(st.settings.ai.providers);
    if (!s.ai.providers.some((p) => p.id === s.ai.activeProvider)) s.ai.activeProvider = '';
    const errs = [];
    if (!Number.isInteger(s.ai.maxSteps) || s.ai.maxSteps < 1 || s.ai.maxSteps > 200) errs.push({ field: 'ai.maxSteps', message: 'Must be between 1 and 200' });
    if (s.ai.activeModel.length > 200) errs.push({ field: 'ai.activeModel', message: 'A model id is at most 200 printable characters' });
    if (s.ai.systemPromptExtra.length > 4000) errs.push({ field: 'ai.systemPromptExtra', message: 'At most 4000 characters' });
    if (errs.length) { const e = new Error('Some settings are not valid'); e.data = errs; throw e; }
    st.settings = s;
    return { settings: s, restartRequired: false };
  };
  h.resetSettings = () => ({ settings: st.settings, restartRequired: false });
  h.getAllowlist = () => [];
  h.bounceStatus = () => ({ watched: 0, cleanedTotal: 0, lastCleanupMs: 0 });
  h.permissionExceptions = () => [];
  h.importDetect = () => [];
  h.getFilterStatus = () => ({ enabled: true, lastCheck: 0, lastError: null, busy: false, userRules: 0, lists: [] });
  // ---- Settings → AI ----
  h.aiStatus = () => ({ keys: clone(st.keys) });
  h.aiProviderSave = (m) => {
    const p = clone(m.provider);
    if (p.kind === 'custom' && !/^https:\/\/|^http:\/\/(localhost|127\.|192\.168\.)/.test(p.baseUrl)) throw new Error('Use https://, or http:// only for this computer or your local network');
    if (!p.id) p.id = (st.nextId++).toString(16);
    const list = st.settings.ai.providers;
    const i = list.findIndex((x) => x.id === p.id);
    if (i >= 0) list[i] = p; else list.push(p);
    return { providers: clone(list), id: p.id };
  };
  h.aiProviderRemove = (m) => {
    st.settings.ai.providers = st.settings.ai.providers.filter((p) => p.id !== m.id);
    if (st.settings.ai.activeProvider === m.id) st.settings.ai.activeProvider = '';
    delete st.keys[m.id];
    return { providers: clone(st.settings.ai.providers) };
  };
  h.aiKeySet = (m) => { st.keys[m.id] = true; return { hasKey: true }; };
  h.aiKeyRemove = (m) => { delete st.keys[m.id]; return { hasKey: false }; };
  const provider = (id) => st.settings.ai.providers.find((p) => p.id === id);
  h.aiTest = (m) => {
    if (!st.settings.ai.enabled) throw new Error('AI features are off');
    const p = provider(m.id);
    if (p.kind === 'custom') throw new Error('Could not connect to ' + p.baseUrl + ' (connection refused)');
    return { ok: true, models: 4, message: '4 models' };
  };
  h.aiModels = (m) => {
    if (!provider(m.id)) throw new Error('No such provider');
    return { models: [
      { id: 'openai/gpt-5-mini', name: 'GPT-5 Mini', vision: true, tools: true, context: 400000 },
      { id: 'anthropic/claude-sonnet-4.5', name: 'Claude Sonnet 4.5', vision: true, tools: true, context: 200000 },
      { id: 'meta-llama/llama-4-scout', name: 'Llama 4 Scout', vision: false, tools: true, context: 128000 },
      { id: 'mistral/tiny-chat', name: 'Tiny Chat', vision: false, tools: false, context: 32000 },
    ] };
  };
}

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot } = h;

  async function testAiSettings(browser) {
    console.log('settings: AI and scripts');
    const defaults = JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
    const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html#ai', { width: 1000, height: 1300 }, 1, aiSettingsMock, defaults);
    await page.waitForFunction(() => document.querySelector('[data-key="ai.maxSteps"]').value === '30');
    await page.waitForTimeout(50);
    const KEY = 'sk-or-v1-TESTKEY-4f9a0c1d2e3b';
    const KEY2 = 'sk-or-v1-CHANGED-77aa88bb99cc';
    const saved = async () => (await lastSent(page, 'saveSettings')).settings;
    const count = async (cmd) => (await sent(page)).filter((m) => m.cmd === cmd).length;
    // Nothing the user typed as a key may be in the DOM: markup, attributes or field values.
    const keyInPage = (k) => page.evaluate((key) => document.documentElement.outerHTML.includes(key) ||
      Array.from(document.querySelectorAll('input, textarea')).some((i) => i.value.includes(key)), k);
    const row = (name) => page.locator('#ai-providers .ai-provider', { hasText: name });

    // Deep link and the ninth tab.
    check(await page.locator('.panel.active[data-tab=ai]').count() === 1 && await page.locator('#tabs a.active[data-tab=ai]').count() === 1, '#ai opens the AI and scripts tab');
    check(await page.locator('.panel').count() === 9, 'nine tabs with AI and scripts');

    // Off by default: no provider controls to use, the explanation visible.
    check(!(await page.isChecked('[data-key="ai.enabled"]')), 'AI features are off by default');
    check(await page.evaluate(() => document.getElementById('ai-body').disabled) && await page.locator('#ai-kind').isDisabled() && await page.locator('#ai-key').isDisabled() &&
      await page.locator('#ai-load-models').isDisabled() && await page.locator('[data-key="ai.activeModel"]').isDisabled(), 'while off the provider and model controls are disabled');
    check(await page.locator('#ai-off-hint').isVisible() && (await page.textContent('#ai-card')).includes('nothing is sent to any provider'), 'the card explains what off and on mean');
    check(await page.isChecked('[data-key="userscripts.enabled"]') && !(await page.isChecked('[data-key="userscripts.privateTabs"]')) && await page.isChecked('[data-key="macros.enabled"]'),
      'userscripts on, not in private tabs, macros on (defaults)');
    await shot(page, 'ai-settings-off.png');

    // Search finds the AI rows from any tab.
    await page.click('#tabs a[data-tab="general"]');
    for (const q of ['OpenRouter', 'API key', 'Anthropic', 'Ollama', 'model', 'agent', 'Tampermonkey', 'hotkey', 'userscript', 'macro', 'AI']) {
      await page.fill('#filter', q);
      await page.waitForTimeout(20);
      const r = await page.evaluate(() => ({
        ai: document.querySelector('.panel[data-tab=ai]').classList.contains('has-matches'),
        shown: Array.from(document.querySelectorAll('.panel[data-tab=ai] .row.setting')).filter((x) => x.offsetParent !== null).length,
      }));
      check(r.ai && r.shown > 0, 'search "' + q + '" shows AI and scripts rows (' + r.shown + ')');
    }
    // A two-letter word matches word starts only: "AI" is not the "ai" inside "email".
    await page.fill('#filter', 'AI');
    await page.waitForTimeout(20);
    const short = await page.evaluate(() => Array.from(document.querySelectorAll('.row.setting')).filter((x) => x.offsetParent !== null)
      .map((x) => x.closest('.panel').dataset.tab));
    check(short.length > 0 && short.every((t) => t === 'ai'), '"AI" matches only AI and scripts rows (' + [...new Set(short)].join(',') + ')');
    await page.fill('#filter', 'openrouter');
    await page.waitForTimeout(20);
    check(await page.locator('#ai-providers-row').isVisible() && await page.locator('.panel.has-matches').count() === 1, '"openrouter": only the providers row\'s tab matches');
    await page.fill('#filter', '');
    await page.click('#tabs a[data-tab="ai"]');
    await page.waitForTimeout(20);

    // Turn it on.
    await page.click('[data-key="ai.enabled"]');
    await page.waitForTimeout(60);
    check((await saved()).ai.enabled === true, 'turning AI on saves ai.enabled');
    check(!(await page.evaluate(() => document.getElementById('ai-body').disabled)) && !(await page.locator('#ai-kind').isDisabled()) && await page.locator('#ai-off-hint').isHidden(), 'the provider controls are usable');

    // Add OpenRouter with a key.
    check(await page.inputValue('#ai-name') === 'OpenRouter' && await page.locator('#ai-base-url').isHidden(), 'the name starts as the kind; no base URL for a named provider');
    const keyAttrs = await page.evaluate(() => { const k = document.getElementById('ai-key'); return [k.type, k.getAttribute('autocomplete'), k.getAttribute('spellcheck')]; });
    check(keyAttrs.join() === 'password,off,false', 'the key field is masked, no autocomplete, no spellcheck (' + keyAttrs.join() + ')');
    await page.click('#ai-add-save');
    await page.waitForTimeout(40);
    check(await count('aiProviderSave') === 0 && (await page.textContent('#ai-add-error')).includes('needs an API key'), 'OpenRouter without a key: an error, nothing sent');
    await page.fill('#ai-key', KEY);
    await page.click('#ai-add-save');
    await row('OpenRouter').waitFor();
    await page.waitForTimeout(80);
    {
      const all = await sent(page);
      const iSave = all.findIndex((m) => m.cmd === 'aiProviderSave');
      const iKey = all.findIndex((m) => m.cmd === 'aiKeySet');
      const ps = all[iSave];
      check(ps && JSON.stringify(ps.provider) === JSON.stringify({ id: '', kind: 'openrouter', name: 'OpenRouter', baseUrl: '', vision: null }), 'aiProviderSave{provider} with an empty id and no key');
      check(iKey > iSave && all[iKey].key === KEY && all[iKey].id === 'a1b2c300', 'then aiKeySet{id, key} for the new id');
      check(!JSON.stringify(ps).includes(KEY), 'the key is not part of the provider');
    }
    check(await page.inputValue('#ai-key') === '' && !(await keyInPage(KEY)), 'the key field is empty and the key is nowhere in the page');
    check((await row('OpenRouter').textContent()).includes('Key saved'), 'the row says "Key saved"');
    check((await saved()).ai.activeProvider === 'a1b2c300' && await page.inputValue('[data-key="ai.activeProvider"]') === 'a1b2c300', 'the first provider becomes the agent\'s');

    // Add a custom (OpenAI-compatible) server without a key.
    await page.selectOption('#ai-kind', 'custom');
    check(await page.locator('#ai-base-url').isVisible() && await page.inputValue('#ai-name') === 'Custom (OpenAI-compatible)' &&
      await page.getAttribute('#ai-base-url', 'placeholder') === 'http://localhost:11434/v1', 'Custom shows the base URL field (placeholder for Ollama)');
    await page.fill('#ai-name', 'Ollama');
    await page.click('#ai-add-save');
    await page.waitForTimeout(40);
    check(await count('aiProviderSave') === 1 && (await page.textContent('#ai-add-error')).includes('address'), 'Custom without an address: an error, nothing sent');
    await page.fill('#ai-base-url', 'http://203.0.113.9/v1');
    await page.click('#ai-add-save');
    await page.waitForTimeout(60);
    check((await page.textContent('#ai-add-error')).includes('https://'), 'the host\'s refusal is shown (' + (await page.textContent('#ai-add-error')) + ')');
    await page.fill('#ai-base-url', 'http://localhost:11434/v1');
    await page.click('#ai-add-save');
    await row('Ollama').waitFor();
    await page.waitForTimeout(60);
    {
      const ps = await lastSent(page, 'aiProviderSave');
      check(ps.provider.kind === 'custom' && ps.provider.baseUrl === 'http://localhost:11434/v1' && ps.provider.name === 'Ollama', 'aiProviderSave for Custom carries the base URL');
      check(await count('aiKeySet') === 1, 'no aiKeySet without a key');
      const t = await row('Ollama').textContent();
      check(t.includes('No key') && t.includes('http://localhost:11434/v1'), 'the Custom row shows its address and "No key"');
      check(await page.textContent('#ai-add-error') === '', 'the error is gone');
      check((await saved()).ai.activeProvider === 'a1b2c300', 'the active provider stays');
    }

    // Test.
    await row('OpenRouter').locator('.ai-test').click();
    await page.waitForTimeout(60);
    check((await lastSent(page, 'aiTest')).id === 'a1b2c300' && (await row('OpenRouter').textContent()).includes('Connected: 4 models'), 'Test → "Connected: 4 models"');
    await row('Ollama').locator('.ai-test').click();
    await page.waitForTimeout(60);
    check((await row('Ollama').locator('.ai-test-result.bad').textContent()).includes('connection refused'), 'a failed Test shows the host\'s message');

    // 0.15.0: Max output tokens per provider (0 = auto), saved on change.
    {
      const input = () => row('OpenRouter').locator('.ai-max-output');
      check(await input().inputValue() === '32000', 'Max output tokens shows the default 32000');
      await input().fill('0');
      await input().blur();
      await page.waitForTimeout(60);
      const ps = await lastSent(page, 'aiProviderSave');
      check(ps.provider.id === 'a1b2c300' && ps.provider.kind === 'openrouter' && ps.provider.maxOutputTokens === 0, 'a change saves the provider with maxOutputTokens 0 (auto)');
      check(await input().inputValue() === '0', 'the row shows 0 after the save');
      const n = await count('aiProviderSave');
      await input().fill('2000000');
      await input().blur();
      await page.waitForTimeout(60);
      check(await count('aiProviderSave') === n && await input().inputValue() === '0', 'out of range: nothing sent, the old value is back');
      await input().fill('64000');
      await input().blur();
      await page.waitForTimeout(60);
      check((await lastSent(page, 'aiProviderSave')).provider.maxOutputTokens === 64000 && await row('Ollama').locator('.ai-max-output').inputValue() === '32000', '64000 is saved for that provider only');
      await page.locator('#ai-providers').screenshot(SHOTS_DIR(h, 'ai-settings-max-output.png'));
    }

    // The model list.
    await page.click('#ai-load-models');
    await page.waitForSelector('#ai-models .ai-model');
    {
      const m = await lastSent(page, 'aiModels');
      check(m.id === 'a1b2c300' && m.refresh === false, 'Load models → aiModels{id, refresh:false}');
      check(await page.locator('#ai-models .ai-model').count() === 4, 'four models listed');
      const tags = await page.evaluate(() => Array.from(document.querySelectorAll('#ai-models .ai-model')).map((b) => Array.from(b.querySelectorAll('.cap')).map((c) => c.textContent).join('+')));
      check(tags.join() === 'tools+images,tools+images,tools,', 'tags "tools" and "images" where the model has them (' + tags.join() + ')');
    }
    await page.fill('#ai-model-filter', 'claude');
    await page.waitForTimeout(20);
    check(await page.locator('#ai-models .ai-model').count() === 1, 'the search narrows the list');
    await page.fill('#ai-model-filter', 'zzz');
    await page.waitForTimeout(20);
    check(await page.locator('#ai-models .ai-model').count() === 0 && (await page.textContent('#ai-models')).includes('No model matches'), 'no match: a note');
    await page.fill('#ai-model-filter', 'llama');
    await page.waitForTimeout(20);
    await page.click('#ai-models .ai-model[data-id="meta-llama/llama-4-scout"]');
    await page.waitForTimeout(60);
    check((await saved()).ai.activeModel === 'meta-llama/llama-4-scout' && await page.inputValue('[data-key="ai.activeModel"]') === 'meta-llama/llama-4-scout', 'clicking a model saves it and fills the model id');
    check(await page.locator('#ai-models .ai-model.on[data-id="meta-llama/llama-4-scout"]').count() === 1, 'the chosen model is marked');
    await page.fill('#ai-model-filter', '');
    await page.waitForTimeout(20);
    await shot(page, 'ai-settings-light.png');
    await page.click('#ai-load-models');
    await page.waitForTimeout(60);
    check((await lastSent(page, 'aiModels')).refresh === true, 'Reload models asks for a fresh list');
    // A typed model id works without the list.
    await page.fill('[data-key="ai.activeModel"]', 'my-org/model-x');
    await page.press('[data-key="ai.activeModel"]', 'Tab');
    await page.waitForTimeout(60);
    check((await saved()).ai.activeModel === 'my-org/model-x', 'a typed model id is saved');

    // Extra instructions and the step limit.
    await page.fill('[data-key="ai.systemPromptExtra"]', 'Answer in German.');
    check(await page.textContent('#ai-extra-count') === '17 / 4000', 'the instructions counter (' + await page.textContent('#ai-extra-count') + ')');
    await page.press('[data-key="ai.systemPromptExtra"]', 'Tab');
    await page.waitForTimeout(60);
    check((await saved()).ai.systemPromptExtra === 'Answer in German.', 'extra instructions are saved');
    await page.fill('[data-key="ai.maxSteps"]', '500');
    await page.press('[data-key="ai.maxSteps"]', 'Tab');
    await page.waitForTimeout(60);
    check(await page.textContent('[data-error-for="ai.maxSteps"]') === 'Must be between 1 and 200' && await page.locator('[data-key="ai.maxSteps"].invalid').count() === 1, 'max steps 500: the field error is shown');
    check((await lastSent(page, 'saveSettings')).settings.ai.maxSteps === 500 && (await page.evaluate(() => window.__aiSt.settings.ai.maxSteps)) === 30, 'nothing invalid was stored');
    await page.fill('[data-key="ai.maxSteps"]', '');
    await page.press('[data-key="ai.maxSteps"]', 'Tab');
    await page.waitForTimeout(60);
    check((await lastSent(page, 'saveSettings')).settings.ai.maxSteps === 0 && await page.textContent('[data-error-for="ai.maxSteps"]') !== '', 'an empty step limit is reported, not sent as null');
    await page.fill('[data-key="ai.maxSteps"]', '40');
    await page.press('[data-key="ai.maxSteps"]', 'Tab');
    await page.waitForTimeout(60);
    check((await saved()).ai.maxSteps === 40 && await page.textContent('[data-error-for="ai.maxSteps"]') === '', 'max steps 40 is saved and the error is gone');
    await page.locator('#ai-card').screenshot(SHOTS_DIR(h, 'ai-settings-card-light.png'));

    // Change the key.
    await row('OpenRouter').locator('.ai-change-key').click();
    await page.waitForSelector('.ai-key-form input[type=password]');
    {
      const a = await page.evaluate(() => { const i = document.querySelector('.ai-key-form input'); return [i.type, i.getAttribute('autocomplete'), i.value, document.activeElement === i]; });
      check(a[0] === 'password' && a[1] === 'off' && a[2] === '' && a[3], 'Change key: an empty masked field, focused (never re-filled)');
    }
    await page.fill('.ai-key-form input', KEY2);
    await page.click('.ai-key-form button[type=submit]');
    await page.waitForTimeout(60);
    check((await lastSent(page, 'aiKeySet')).key === KEY2 && await page.locator('.ai-key-form').count() === 0, 'Save key → aiKeySet, the form closes');
    check(!(await keyInPage(KEY2)) && !(await keyInPage(KEY)), 'neither key is in the page');
    // Remove a key.
    await row('OpenRouter').locator('.ai-change-key').click();
    await page.click('.ai-key-form .ai-remove-key');
    await page.waitForTimeout(60);
    check((await lastSent(page, 'aiKeyRemove')).id === 'a1b2c300' && (await row('OpenRouter').textContent()).includes('No key'), 'Remove key → aiKeyRemove, "No key"');
    await row('OpenRouter').locator('.ai-change-key').click();
    await page.fill('.ai-key-form input', KEY);
    await page.press('.ai-key-form input', 'Enter');
    await page.waitForTimeout(60);
    check((await row('OpenRouter').textContent()).includes('Key saved') && await count('aiKeySet') === 3, 'Enter saves a key too');

    // Remove a provider (asks first).
    await row('Ollama').locator('.ai-remove').click();
    await page.waitForSelector('#lb-dialog-cancel');
    await page.click('#lb-dialog-cancel');
    await page.waitForTimeout(40);
    check(await count('aiProviderRemove') === 0 && await row('Ollama').count() === 1, 'Cancel keeps the provider');
    await row('Ollama').locator('.ai-remove').click();
    await page.waitForSelector('#lb-dialog-ok');
    await page.click('#lb-dialog-ok');
    await page.waitForTimeout(60);
    check((await lastSent(page, 'aiProviderRemove')).id === 'a1b2c301' && await row('Ollama').count() === 0, 'Remove → aiProviderRemove, the row is gone');
    check(await page.locator('[data-key="ai.activeProvider"] option').count() === 2, 'the provider choice lists the rest');

    // Userscripts and macros.
    await page.click('[data-key="userscripts.privateTabs"]');
    await page.waitForTimeout(60);
    check((await saved()).userscripts.privateTabs === true, 'Also in private tabs is saved');
    await page.click('[data-key="macros.enabled"]');
    await page.waitForTimeout(60);
    check((await saved()).macros.enabled === false, 'Macros off is saved');
    await page.click('[data-key="userscripts.enabled"]');
    await page.waitForTimeout(60);
    check((await saved()).userscripts.enabled === false, 'Run userscripts off is saved');
    await page.click('#manage-scripts');
    await page.waitForTimeout(30);
    {
      const m = await lastSent(page, 'openPage');
      check(m && m.page === 'scripts', 'Manage scripts opens krunk://scripts');
    }
    // Saving the page never sends providers the host does not have.
    check(JSON.stringify((await saved()).ai.providers.map((p) => p.id)) === '["a1b2c300"]', 'the providers stay the host\'s');

    // Dark.
    await emit(page, { event: 'theme', theme: 'dark' });
    await page.waitForTimeout(60);
    await page.evaluate(() => window.scrollTo(0, 0));
    await shot(page, 'ai-settings-dark.png');
    await emit(page, { event: 'theme', theme: 'light' });

    // Off again: Test is disabled and says why.
    await page.click('[data-key="ai.enabled"]');
    await page.waitForTimeout(60);
    {
      const b = row('OpenRouter').locator('.ai-test');
      check((await saved()).ai.enabled === false && await b.isDisabled() && (await b.getAttribute('title')).includes('Turn on AI'), 'AI off: Test is disabled with a hint');
      check(await page.locator('#ai-off-hint').isVisible() && await page.evaluate(() => document.getElementById('ai-body').disabled), 'and the rest of the card is disabled again');
    }
    check(!(await keyInPage(KEY)) && !(await keyInPage(KEY2)), 'no key in the page at the end');

    check(errors.length === 0, 'no console errors' + (errors.length ? ': ' + errors.join(' | ') : ''));
    check(external.length === 0, 'no external requests' + (external.length ? ': ' + external.join(' ') : ''));
    await page.context().close();
  }

  return { testAiSettings };
};

// An element screenshot's options (a no-op path when screenshots are off).
function SHOTS_DIR(h, name) {
  return h.SHOTS ? { path: path.join(h.SHOTS, name) } : {};
}
