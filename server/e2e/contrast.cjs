// WCAG contrast of the website's colour tokens (dist/server/site/style.css), light and dark:
// text pairs need 4.5:1, UI parts (input borders, focus ring, outline buttons) 3:1.
//   node server/e2e/contrast.cjs [style.css] > contrast.md
'use strict';
const fs = require('fs');
const path = require('path');

const css = fs.readFileSync(process.argv[2] || path.join(__dirname, '../../dist/server/site/style.css'), 'utf8');
function tokens(block) {
  const t = {};
  for (const m of block.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{6})/g)) t[m[1]] = m[2].toLowerCase();
  return t;
}
// Since the KruxSYS restyle (2026-11-24) dark is the default (:root) and light overrides it.
const dark = tokens(css.slice(css.indexOf(':root {'), css.indexOf('}', css.indexOf(':root {'))));
const lightStart = css.indexOf(':root {', css.indexOf('@media (prefers-color-scheme: light)'));
const light = Object.assign({}, dark, tokens(css.slice(lightStart, css.indexOf('}', lightStart))));

function lum(hex) {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}
function ratio(a, b) {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}
const TEXT = [
  ['fg', 'bg'], ['fg', 'page'], ['fg', 'card'], ['fg', 'sunken'],
  ['muted', 'card'], ['muted', 'sunken'], ['muted', 'page'],
  ['link', 'card'], ['link', 'page'],
  ['on-primary', 'primary'], ['on-primary', 'primary-hover'],
  ['danger', 'card'], ['danger', 'danger-bg'], ['ok', 'ok-bg'], ['warn', 'warn-bg'], ['info', 'info-bg'],
];
const UI = [['border-strong', 'card'], ['border-strong', 'bg'], ['focus', 'card'], ['focus', 'page'], ['danger', 'card'], ['primary', 'card'], ['primary', 'page']];
let failed = 0;
const out = ['# Website colour tokens: contrast (WCAG 2.1)', '', '| Pair | Need | Light | Dark |', '|---|---|---|---|'];
for (const [list, need] of [[TEXT, 4.5], [UI, 3]]) {
  for (const [a, b] of list) {
    const l = ratio(light[a], light[b]);
    const d = ratio(dark[a], dark[b]);
    const mark = (r) => `${r.toFixed(2)}${r >= need ? '' : ' **FAIL**'}`;
    if (l < need || d < need) failed++;
    out.push(`| \`--${a}\` on \`--${b}\` | ${need}:1 | ${mark(l)} (${light[a]} / ${light[b]}) | ${mark(d)} (${dark[a]} / ${dark[b]}) |`);
  }
}
// 0.10.0: the translucent sticky header (90 % --card, 10 % whatever scrolls behind it; style.css
// .site-bar). The header text and links over the strongest colours of the page content.
function mix(a, b, wa) {
  const ch = (h, i) => parseInt(h.slice(i, i + 2), 16);
  return '#' + [1, 3, 5].map((i) => Math.round(ch(a, i) * wa + ch(b, i) * (1 - wa)).toString(16).padStart(2, '0')).join('');
}
out.push('', '| Header text over content (90 % card) | Need | Light | Dark |', '|---|---|---|---|');
for (const behind of ['fg', 'primary', 'link', 'danger', 'muted', 'page', '#000000', '#ffffff']) {
  for (const text of ['fg', 'link']) {
    const pick = (t, k) => (k.startsWith('#') ? k : t[k]);
    const l = ratio(light[text], mix(light.card, pick(light, behind), 0.9));
    const d = ratio(dark[text], mix(dark.card, pick(dark, behind), 0.9));
    if (l < 4.5 || d < 4.5) failed++;
    const mark = (r) => `${r.toFixed(2)}${r >= 4.5 ? '' : ' **FAIL**'}`;
    out.push(`| \`--${text}\` over \`${behind}\` | 4.5:1 | ${mark(l)} | ${mark(d)} |`);
  }
}
out.push('', failed ? `${failed} pair(s) below the minimum.` : 'Every pair meets its minimum.');
console.log(out.join('\n'));
process.exit(failed ? 1 : 0);
