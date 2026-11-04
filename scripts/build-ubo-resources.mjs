// Converts uBlock Origin's resources to the adblock crate's Resource list (JSON):
//   node build-ubo-resources.mjs <uBO source dir> <output.json> <uBO version>
// - Scriptlets: src/js/resources/scriptlets.js exports builtinScriptlets (registered with
//   name, aliases, fn, dependencies, requiresTrust, world). Each becomes a function-style
//   resource: the function's source, "application/javascript" for injectable scriptlets
//   (name.js) and "fn/javascript" for helpers (name.fn). requiresTrust sets permission bit 1,
//   which only uBO's own lists get. ISOLATED-world scriptlets run in the page's world too
//   (CEF gives us no isolated world); they only touch DOM, cookies and storage.
// - Redirect resources: src/js/redirect-resources.js (a Map of file name -> details) and the
//   files in src/web_accessible_resources. Entries with params are skipped (as the adblock
//   crate's own assembler does).
import { readFileSync, writeFileSync } from 'node:fs';
import { join, extname } from 'node:path';
import { pathToFileURL } from 'node:url';

const [src, out, version] = process.argv.slice(2);
if (!src || !out) { console.error('usage: build-ubo-resources.mjs <uBO dir> <out.json> <version>'); process.exit(2); }

const b64 = (s) => Buffer.from(s).toString('base64');
const MIME = { '.js': 'application/javascript', '.css': 'text/css', '.gif': 'image/gif', '.html': 'text/html',
  '.json': 'application/json', '.mp3': 'audio/mp3', '.mp4': 'video/mp4', '.png': 'image/png', '.txt': 'text/plain',
  '.xml': 'text/xml' };
const TRUSTED = 1;

const resources = [];
const { builtinScriptlets } = await import(pathToFileURL(join(src, 'src/js/resources/scriptlets.js')).href);
for (const s of builtinScriptlets) {
  const helper = s.name.endsWith('.fn');
  const r = {
    name: s.name,
    aliases: Array.isArray(s.aliases) ? s.aliases : [],
    kind: { mime: helper ? 'fn/javascript' : 'application/javascript' },
    content: b64(s.fn.toString()),
  };
  if (Array.isArray(s.dependencies) && s.dependencies.length) r.dependencies = s.dependencies;
  if (s.requiresTrust) r.permission = TRUSTED;
  resources.push(r);
}
const scriptlets = resources.length;

const redirects = (await import(pathToFileURL(join(src, 'src/js/redirect-resources.js')).href)).default;
let skipped = 0;
for (const [name, details] of redirects) {
  if (details.params) { skipped++; continue; }
  const file = join(src, 'src/web_accessible_resources', name);
  let bytes;
  try { bytes = readFileSync(file); } catch (e) { skipped++; continue; }
  const aliases = details.alias === undefined ? [] : [].concat(details.alias);
  const mime = MIME[extname(name)] || 'text/plain';
  resources.push({ name, aliases, kind: { mime }, content: bytes.toString('base64') });
}

resources.sort((a, b) => a.name.localeCompare(b.name));
writeFileSync(out, JSON.stringify({ ubo: version, resources }, null, 1) + '\n');
console.log(`uBO ${version}: ${scriptlets} scriptlets, ${resources.length - scriptlets} redirect resources (${skipped} skipped) -> ${out}`);
