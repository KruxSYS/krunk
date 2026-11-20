// 0.26.0 load harness: what the recorded corpus asks of the browser (preload hints, unload
// listeners, no-store documents). node corpus-scan.cjs corpus.wprgo
const fs = require('fs'), zlib = require('zlib');
const d = JSON.parse(zlib.gunzipSync(fs.readFileSync(process.argv[2])).toString());
const pat = { prerender: /rel=["']?prerender/i, speculationrules: /type=["']?speculationrules/i, preconnect: /rel=["']?preconnect/i,
  'dns-prefetch': /rel=["']?dns-prefetch/i, preload: /rel=["']?preload/i, unload: /addEventListener\(\s*["']unload["']|\bonunload\s*=/i,
  'no-store-doc': null };
const c = {}, hosts = {};
let n = 0, failed = 0;
for (const [host, byUrl] of Object.entries(d.Requests)) for (const [url, entries] of Object.entries(byUrl)) for (const e of entries) {
  n++;
  const raw = Buffer.from(e.SerializedResponse, 'base64');
  const i = raw.indexOf('\r\n\r\n');
  const head = raw.subarray(0, i).toString('latin1').toLowerCase();
  let body = raw.subarray(i + 4);
  try {
    if (/content-encoding: br/.test(head)) body = zlib.brotliDecompressSync(body);
    else if (/content-encoding: gzip/.test(head)) body = zlib.gunzipSync(body);
    else if (/content-encoding: zstd/.test(head)) body = zlib.zstdDecompressSync ? zlib.zstdDecompressSync(body) : Buffer.alloc(0);
  } catch { failed++; }
  const text = head + '\n' + body.toString('latin1');
  for (const [k, p] of Object.entries(pat)) {
    const hit = k === 'no-store-doc' ? (/content-type: text\/html/.test(head) && /cache-control:[^\n]*no-store/.test(head)) : p.test(text);
    if (hit) { c[k] = (c[k] || 0) + 1; (hosts[k] ||= new Set()).add(host); }
  }
}
console.log('responses', n, 'undecodable', failed);
for (const k of Object.keys(pat)) console.log(k, c[k] || 0, [...(hosts[k] || [])].slice(0, 10).join(' '));
