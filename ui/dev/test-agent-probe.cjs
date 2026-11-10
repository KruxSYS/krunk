// Tests of the AI agent's page probe (ui/inject/agent-probe.js, 0.12.0, WP-B2). The probe is
// evaluated the way the browser does it: in an isolated world of the main frame created with
// DevTools (`Page.createIsolatedWorld`), called with `Runtime.evaluate` and a context id.
//
// Exported for ui/dev/test-ui.cjs: `(helpers) => ({ testAgentProbe })`.
'use strict';
const fs = require('fs');
const path = require('path');

// core's rules (agent::redact::probe_rules) in the same shape.
const RULES = {
  selector: 'input[type="password" i], input[autocomplete^="cc-" i], input[autocomplete*=" cc-" i], textarea[autocomplete^="cc-" i], textarea[autocomplete*=" cc-" i], select[autocomplete^="cc-" i], select[autocomplete*=" cc-" i]',
  namePattern: 'card.?num|cc.?(num|no)|credit.?card|cvv|cvc|csc|security.?code|card.?code|cc.?exp',
  nameAttributes: ['name', 'id', 'aria-label', 'placeholder'],
  luhn: true, minDigits: 13, maxDigits: 19, redacted: '[redacted]',
};

const PAGE = `<!doctype html><html><head><title>Probe fixture</title>
<script>
  // The page tries to tamper with the DOM API and to see the probe.
  window.__seen = [];
  const qs = Element.prototype.querySelectorAll;
  Document.prototype.querySelectorAll = function () { window.__seen.push('qsa'); return qs.apply(this.documentElement, arguments); };
  window.__inputs = 0;
  document.addEventListener('input', () => { window.__inputs++; }, true);
  var TOKEN = "SCRIPTBODY-42";
</script>
<style>.x { color: red }</style></head>
<body>
<h1>Checkout</h1>
<form id="f" action="/submit" method="post">
  <label for="name">Full name</label><input id="name" name="name" value="Ann">
  <label for="pw">Password</label><input id="pw" type="password" name="password" value="hunter2-SECRET">
  <label for="cc">Card</label><input id="cc" name="ccfield" autocomplete="billing cc-number" value="4111 1111 1111 1111">
  <input id="cvv" name="cvv" placeholder="CVV">
  <input id="plain" name="note" value="">
  <select id="color" name="color"><option value="r">Red</option><option value="g">Green</option></select>
  <textarea id="msg" name="msg">hello there</textarea>
  <button type="submit">Pay now</button>
</form>
<p>Call us at 555 0100 or order 12345.</p>
<a href="/one">First link</a> <a href="https://other.example/two" rel="noopener" target="_blank">Second link</a>
<iframe id="same" src="/frame.html" style="width:300px;height:120px"></iframe>
<iframe id="pay" src="https://js.stripe.example/card" title="Secure card payment" style="width:300px;height:80px"></iframe>
<video id="vid" src="/v.mp4" muted></video>
<div style="height:2000px"></div>
<p id="late-anchor">end</p>
</body></html>`;

const FRAME = `<!doctype html><body><form id="ff"><input id="fpw" type="password" value="frame-SECRET"><input id="fuser" name="user" value="ann"></form><a href="/framelink">Frame link</a><p>Frame text</p></body>`;

module.exports = (h) => {
  const { check } = h;

  async function testAgentProbe(browser) {
    console.log('agent probe (isolated world)');
    const src = fs.readFileSync(path.join(__dirname, '..', 'inject', 'agent-probe.js'), 'utf8');
    const context = await browser.newContext({ viewport: { width: 1000, height: 700 } });
    await context.route('**/*', (r) => r.abort());
    await context.route('https://shop.example/', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: PAGE }));
    await context.route('https://shop.example/frame.html', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: FRAME }));
    await context.route('https://js.stripe.example/card', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: '<body>card form</body>' }));
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.goto('https://shop.example/');
    await page.waitForFunction(() => { try { return document.getElementById('same').contentDocument.getElementById('fpw'); } catch (e) { return false; } });

    // The browser's way in: an isolated world of the main frame.
    const cdp = await context.newCDPSession(page);
    const tree = await cdp.send('Page.getFrameTree');
    const world = await cdp.send('Page.createIsolatedWorld', { frameId: tree.frameTree.frame.id, worldName: 'krunk-agent', grantUniveralAccess: false });
    const ctx = world.executionContextId;
    const inject = await cdp.send('Runtime.evaluate', { contextId: ctx, expression: '(' + src + ')(' + JSON.stringify(RULES) + ') && true', returnByValue: true });
    check(!inject.exceptionDetails && inject.result.value === true, 'the probe defines itself in the isolated world (' + JSON.stringify(inject.exceptionDetails || '') + ')');
    const call = async (fn, ...args) => {
      const r = await cdp.send('Runtime.evaluate', { contextId: ctx, expression: '__krunkProbe.' + fn + '(...' + JSON.stringify(args) + ')', returnByValue: true, awaitPromise: true });
      if (r.exceptionDetails) return { exception: r.exceptionDetails.exception ? r.exceptionDetails.exception.description : r.exceptionDetails.text };
      return r.result.value;
    };
    const mainSees = await page.evaluate(() => typeof window.__krunkProbe);
    check(mainSees === 'undefined', 'page scripts can\'t see the probe');
    const again = await cdp.send('Runtime.evaluate', { contextId: ctx, expression: '(' + src + ')(null) === __krunkProbe && Object.isFrozen(__krunkProbe)', returnByValue: true });
    check(again.result.value === true, 'a second injection keeps the first (frozen) probe');

    // ---- context ----
    const c = await call('context');
    check(c && c.headings && c.headings[0].text === 'Checkout' && c.forms.length === 2 && c.counts.links === 3, 'context: headings, forms (page and frame), link count (' + JSON.stringify(c && c.counts) + ')');
    const cs = JSON.stringify(c);
    check(!/hunter2|4111|frame-SECRET|Ann"/.test(cs), 'context holds no field values');
    check(c.forms[0].fields.some((f) => f.selector === '#pw' && f.sensitive) && c.forms[0].fields.some((f) => f.selector === '#cvv' && f.sensitive), 'context marks the password and the CVV field as sensitive');
    check(page && (await page.evaluate(() => window.__seen.length)) === 0, 'the page\'s patched querySelectorAll is not used by the probe');

    // ---- html ----
    const hAll = await call('html', null, false);
    check(hAll.found && hAll.html.includes('[redacted]') && hAll.html.includes('value="Ann"'), 'html: sensitive values blanked, others kept');
    check(!/hunter2|4111 1111|frame-SECRET/.test(hAll.html), 'html: no password or card value');
    check(!hAll.html.includes('SCRIPTBODY') && !hAll.html.includes('color: red'), 'html: script and style bodies dropped');
    check(!/\n|\s{2}/.test(hAll.html), 'html: whitespace collapsed');
    const hS = await call('html', 'head', true);
    check(hS.html.includes('SCRIPTBODY'), 'html with include_scripts keeps script bodies');
    const hNo = await call('html', '#nothing', false);
    check(hNo.found === false, 'html: a selector without a match');
    const hForm = await call('html', '#f', false);
    check(hForm.html.startsWith('<form') && hForm.html.includes('data-krunk-redacted') && !hForm.html.includes('hunter2'), 'html of a selector: the form, redacted');

    // ---- text ----
    const t = await call('text', null);
    check(t.text.includes('First link') && t.text.includes('Frame text') && !/hunter2|4111|hello there/.test(t.text), 'text: page and frame text, no form values (' + t.text.slice(0, 80) + ')');

    // ---- query ----
    const q = await call('query', 'input', 20, 0);
    const qpw = q.items.find((i) => i.selector === '#pw');
    const qname = q.items.find((i) => i.selector === '#name');
    check(q.total === 7 && qpw && qpw.attributes.value === '[redacted]' && qpw.value === '[redacted]' && qpw.sensitive, 'query: the password\'s value attribute and value redacted (' + q.total + ')');
    check(qname && qname.value === 'Ann' && qname.box.w > 0 && qname.visible === true, 'query: plain fields keep their value, box and visibility');
    check(!JSON.stringify(q).includes('frame-SECRET') && !JSON.stringify(q).includes('4111'), 'query: frame password and card value never appear');
    const qBad = await call('query', 'input[', 5, 0);
    check(qBad.exception && /valid selector/.test(qBad.exception), 'query: a bad selector comes back as an error');

    // ---- forms ----
    const f = await call('forms');
    const main = f.forms.find((x) => x.selector === '#f');
    const frameForm = f.forms.find((x) => x.inFrame);
    const field = (sel) => main.fields.find((x) => x.selector === sel);
    check(main && main.method === 'post' && /\/submit$/.test(main.action) && main.buttons[0].text === 'Pay now', 'forms: action, method, buttons');
    check(field('#pw').value === '[redacted]' && field('#cc').value === '[redacted]' && field('#name').value === 'Ann' && field('#name').label === 'Full name', 'forms: password and card redacted, labels and plain values kept');
    check(field('#cvv').sensitive === true && field('#cvv').value === '', 'forms: an empty CVV field is marked sensitive');
    check(field('#color').options.length === 2 && field('#color').value === 'Red', 'forms: select options and choice');
    check(frameForm && frameForm.fields.find((x) => x.type === 'password').value === '[redacted]', 'forms: same-origin frame walked, its password redacted');

    // ---- links ----
    const l = await call('links', null, 100, 0);
    check(l.total === 3 && l.links[1].href === 'https://other.example/two' && l.links[1].rel === 'noopener' && l.links[1].target === '_blank' && l.links[2].text === 'Frame link', 'links: text, absolute href, rel, target, frame links');
    const lf = await call('links', 'second', 100, 0);
    check(lf.total === 1 && lf.links[0].text === 'Second link', 'links: filter');
    const lp = await call('links', null, 1, 1);
    check(lp.links.length === 1 && lp.links[0].text === 'Second link' && lp.total === 3, 'links: limit and offset');

    // ---- media ----
    const m = await call('media');
    check(m.elements.length === 1 && /\/v\.mp4$/.test(m.elements[0].src), 'media: the video element');

    // ---- fill ----
    const before = await page.evaluate(() => window.__inputs);
    const fl = await call('fill', { '#name': 'Bob', '#pw': 'typed', '#cc': '1', '#cvv': '123', '#nope': 'x', '#color': 'Green', '#msg': 'hi' });
    check(fl.results['#name'] === 'ok' && fl.results['#msg'] === 'ok' && fl.results['#color'] === 'ok', 'fill: plain fields, textarea and select filled');
    check(/^refused/.test(fl.results['#pw']) && /^refused/.test(fl.results['#cc']) && /^refused/.test(fl.results['#cvv']) && fl.results['#nope'] === 'not found', 'fill: password, card and CVV refused; a missing field reported');
    const vals = await page.evaluate(() => ({ name: document.getElementById('name').value, pw: document.getElementById('pw').value, cc: document.getElementById('cc').value, color: document.getElementById('color').value, inputs: window.__inputs }));
    check(vals.name === 'Bob' && vals.pw === 'hunter2-SECRET' && vals.cc === '4111 1111 1111 1111' && vals.color === 'g', 'fill: the page sees the new values; the password and card are untouched');
    check(vals.inputs - before >= 3, 'fill: input events reach the page\'s listeners');

    // ---- select ----
    const s1 = await call('select', '#color', ['Red']);
    check(s1.selected && s1.selected[0] === 'Red' && (await page.evaluate(() => document.getElementById('color').value)) === 'r', 'select: by label');
    const s2 = await call('select', '#name', ['x']);
    check(!!s2.error, 'select: not a select');

    // ---- Luhn rule and secrets ----
    await page.evaluate(() => { document.getElementById('plain').value = '4242 4242 4242 4242'; });
    const ql = await call('query', '#plain', 1, 0);
    check(ql.items[0].sensitive === true && ql.items[0].value === '[redacted]', 'a plain field holding a Luhn-valid number counts as a card field');
    await page.evaluate(() => { document.getElementById('plain').value = '555 0100'; });
    const qn = await call('query', '#plain', 1, 0);
    check(!qn.items[0].sensitive && qn.items[0].value === '555 0100', 'a short number is not a card number');
    await page.evaluate(() => { document.getElementById('plain').value = '4242424242424242'; });
    const sec = await call('secrets');
    check(sec.includes('hunter2-SECRET') && sec.includes('4111 1111 1111 1111') && sec.includes('frame-SECRET') && sec.includes('4242424242424242') && !sec.includes('Bob'), 'secrets: password, card, frame password and the Luhn value; not plain values (' + sec.length + ')');
    await page.evaluate(() => { document.getElementById('plain').value = ''; });

    // ---- focus, box ----
    const fo = await call('focus', '#pw');
    const active = await page.evaluate(() => document.activeElement && document.activeElement.id);
    check(fo.sensitive === true && active !== 'pw', 'focus: a password field is reported and not focused');
    await call('focus', '#name');
    const fs1 = await call('focusedSensitive');
    check(fs1.sensitive === false && fs1.editable === true, 'focusedSensitive: a plain field');
    await page.evaluate(() => document.getElementById('cvv').focus());
    const fs2 = await call('focusedSensitive');
    check(fs2.sensitive === true, 'focusedSensitive: the CVV field');
    await page.evaluate(() => document.getElementById('same').contentDocument.getElementById('fpw').focus());
    const fs3 = await call('focusedSensitive');
    check(fs3.sensitive === true, 'focusedSensitive: a password inside a same-origin frame');
    const b = await call('box', '#late-anchor');
    const sy = await page.evaluate(() => scrollY);
    check(b.found && sy > 500 && b.y > 0 && b.y < 700 && b.sensitive === false, 'box: scrolled into view, centre inside the viewport (' + JSON.stringify(b) + ')');
    const bf = await call('box', '#fuser');
    const fb = await page.evaluate(() => { const r = document.getElementById('same').getBoundingClientRect(); return { x: r.left, y: r.top }; });
    check(bf.found && bf.x > fb.x && bf.y > fb.y, 'box: an element in a frame gets top-viewport coordinates');

    // ---- scroll ----
    const sc = await call('scroll', 0, 0, 'top', null);
    check(sc.y === 0 && sc.maxY > 1000, 'scroll: to the top, with the scroll range');
    const sc2 = await call('scroll', 0, 300, null, null);
    check(sc2.y === 300, 'scroll: by dy');

    // ---- waitFor ----
    const w1 = call('waitFor', '#later', 3000);
    await page.evaluate(() => setTimeout(() => { const d = document.createElement('div'); d.id = 'later'; document.body.appendChild(d); }, 200));
    check((await w1).found === true, 'waitFor: an element added later');
    const w2 = await call('waitFor', '#never', 300);
    check(w2.found === false, 'waitFor: timeout');

    // ---- edit ----
    const e1 = await call('edit', 'set_attribute', '#name', 'onclick', 'alert(1)', null, false);
    const e2 = await call('edit', 'set_attribute', '#pw', 'type', 'text', null, false);
    const e3 = await call('edit', 'set_attribute', '#f', 'id', 'g', null, false);
    const e4 = await call('edit', 'insert_css', null, null, null, 'input[value^="h"]{background:url(https://evil.example/h)}', false);
    check(e1.refused && e2.refused && e3.refused && e4.refused, 'edit: event handlers, a password field\'s type, renaming its form, and fetching CSS while a password holds a value are refused');
    const e5 = await call('edit', 'set_text', 'h1', null, 'Edited', null, false);
    const e6 = await call('edit', 'insert_css', null, null, null, 'h1 { color: rgb(1, 2, 3) }', false);
    const e7 = await call('edit', 'remove', 'video', null, null, null, true);
    const after = await page.evaluate(() => ({ h1: document.querySelector('h1').textContent, color: getComputedStyle(document.querySelector('h1')).color, video: !!document.querySelector('video') }));
    check(e5.ok && e6.ok && e7.ok && after.h1 === 'Edited' && after.color === 'rgb(1, 2, 3)' && !after.video, 'edit: set_text, insert_css and remove');

    // ---- setTitle ----
    await call('setTitle', 'EXAMPLE');
    check((await page.title()) === 'EXAMPLE', 'setTitle changes the document title');

    // ---- mask ----
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.evaluate(() => { document.getElementById('pw').style.color = 'rgb(9, 9, 9)'; });
    const mk = await call('mask', true);
    const masked = await page.evaluate(() => {
      const pw = document.getElementById('pw');
      const st = getComputedStyle(pw);
      const cover = document.querySelector('[data-krunk-mask-cover]');
      const fr = document.getElementById('pay').getBoundingClientRect();
      const cr = cover && cover.getBoundingClientRect();
      return { attr: pw.hasAttribute('data-krunk-mask'), sec: st.webkitTextSecurity, color: st.color, cc: document.getElementById('cc').hasAttribute('data-krunk-mask'), cover: !!cover, fits: !!cr && Math.abs(cr.left - fr.left) < 1 && Math.abs(cr.width - fr.width) < 1, name: document.getElementById('name').hasAttribute('data-krunk-mask'), framePw: document.getElementById('same').contentDocument.getElementById('fpw').hasAttribute('data-krunk-mask') };
    });
    if (h.SHOTS) await page.screenshot({ path: path.join(h.SHOTS, 'agent-probe-masked.png') });
    check(mk.masked >= 4 && mk.covered === 1, 'mask: sensitive fields and the payment frame counted (' + JSON.stringify(mk) + ')');
    check(masked.attr && masked.cc && masked.framePw && !masked.name && masked.sec === 'disc' && masked.color === 'rgba(0, 0, 0, 0)', 'mask: password and card fields marked, discs in transparent text; plain fields untouched (' + JSON.stringify(masked) + ')');
    check(masked.cover && masked.fits, 'mask: a grey cover over the payment frame');
    await call('mask', false);
    const unmasked = await page.evaluate(() => {
      const pw = document.getElementById('pw');
      const cc = document.getElementById('cc');
      return { attr: pw.hasAttribute('data-krunk-mask'), color: getComputedStyle(pw).color, sec: getComputedStyle(cc).webkitTextSecurity, ccStyle: cc.getAttribute('style'), cover: !!document.querySelector('[data-krunk-mask-cover]') };
    });
    check(!unmasked.attr && unmasked.color === 'rgb(9, 9, 9)' && unmasked.sec === 'none' && unmasked.ccStyle === null && !unmasked.cover, 'mask off: marks, styles and covers removed; the page\'s own inline style restored (' + JSON.stringify(unmasked) + ')');

    // ---- defaults when the browser sends no rules ----
    const ctx2 = (await cdp.send('Page.createIsolatedWorld', { frameId: tree.frameTree.frame.id, worldName: 'krunk-agent-2' })).executionContextId;
    const d = await cdp.send('Runtime.evaluate', { contextId: ctx2, expression: '(' + src + ')(null).secrets()', returnByValue: true });
    check(d.result.value && d.result.value.includes('hunter2-SECRET'), 'built-in rules apply without rules from the browser');

    const n = await call('now');
    check(/^\d{4}-\d\d-\d\d \d\d\.\d\d\.\d\d$/.test(n), 'now: a file-name timestamp (' + n + ')');
    check(errors.length === 0, 'no page errors (' + errors.join('; ') + ')');
    await context.close();
  }

  return { testAgentProbe };
};
