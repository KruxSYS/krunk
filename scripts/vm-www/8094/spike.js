'use strict';
// Krunk 0.10.0 (P3): see spike.html.
const q = new URLSearchParams(location.search);
const tag = q.get('tag') || 'spike';
const kase = q.get('case') || 'state';
const send = (line) => fetch('/m?' + encodeURIComponent(`${tag} ${line}`)).catch(() => {});
const extra = document.getElementById('extra');
document.getElementById('h').textContent = `Hidden-tab spike: ${kase}`;
document.title = `spike ${kase}`;

let frames = 0, ticks = 0;
(function f() { frames++; requestAnimationFrame(f); })();
setInterval(() => { ticks++; }, 16);
let caseInfo = () => '';
let last = performance.now();
setInterval(() => {
  const now = performance.now(), secs = (now - last) / 1000; last = now;
  send(`st=${document.visibilityState} raf=${(frames / secs).toFixed(1)} tim=${(ticks / secs).toFixed(1)} ${caseInfo()}`);
  frames = 0; ticks = 0;
}, 5000);
document.addEventListener('visibilitychange', () => send(`visibility=${document.visibilityState} at=${Date.now()} ${caseInfo()}`));
addEventListener('focus', () => send(`focus at=${Date.now()} ${caseInfo()}`));

if (kase === 'state') {
  // The harness types into the field; the page scrolls itself to the marker.
  addEventListener('load', () => setTimeout(() => scrollTo(0, 1400), 300));
  caseInfo = () => `value=${JSON.stringify(document.getElementById('field').value)} scroll=${Math.round(scrollY)}`;
}

if (kase === 'audio') {
  const a = Object.assign(document.createElement('audio'), { src: 'test-vp9-opus.webm', loop: true, controls: true });
  extra.append(a);
  addEventListener('keydown', (e) => { if (e.key === 'p') a.play().then(() => send('audio playing'), (err) => send(`audio error ${err.name}`)); });
  caseInfo = () => `audio t=${a.currentTime.toFixed(1)} paused=${a.paused}`;
}

if (kase === 'rtc') {
  // A local call: a playing video's stream (like a camera, it does not depend on the page's
  // timers) through two peer connections, shown in a second video element.
  const src = Object.assign(document.createElement('video'), { src: 'test-vp9-opus.webm', muted: true, loop: true, autoplay: true, playsInline: true });
  const v = Object.assign(document.createElement('video'), { autoplay: true, muted: true, playsInline: true });
  extra.append(src, v);
  // Krunk only gathers candidates through STUN: report-serve.py answers on the page's port.
  const a = new RTCPeerConnection({ iceServers: [{ urls: `stun:${q.get('stun') || '127.0.0.1'}:${location.port}` }] });
  const b = new RTCPeerConnection({ iceServers: [{ urls: `stun:${q.get('stun') || '127.0.0.1'}:${location.port}` }] });
  let cands = 0;
  a.onicecandidate = (e) => { if (e.candidate) { cands++; b.addIceCandidate(e.candidate); } };
  b.onicecandidate = (e) => e.candidate && a.addIceCandidate(e.candidate);
  b.ontrack = (e) => { v.srcObject = e.streams[0]; };
  (async () => {
    await src.play();
    src.captureStream().getVideoTracks().forEach((t) => a.addTrack(t, new MediaStream([t])));
    await a.setLocalDescription(await a.createOffer());
    await b.setRemoteDescription(a.localDescription);
    await b.setLocalDescription(await b.createAnswer());
    await a.setRemoteDescription(b.localDescription);
  })().catch((e) => send(`rtc error ${e.name}`));
  let decoded = 0, sent = 0;
  setInterval(async () => {
    try {
      (await b.getStats()).forEach((r) => { if (r.type === 'inbound-rtp' && r.kind === 'video') decoded = r.framesDecoded; });
      (await a.getStats()).forEach((r) => { if (r.type === 'outbound-rtp' && r.kind === 'video') sent = r.framesEncoded; });
    } catch (e) { /* closed */ }
  }, 1000);
  caseInfo = () => `rtc state=${a.connectionState} ice=${a.iceGatheringState}/${a.iceConnectionState} cands=${cands} encoded=${sent} decoded=${decoded}`;
}

if (kase === 'notify') {
  let shown = 'no';
  addEventListener('keydown', (e) => {
    if (e.key !== 'n') return;
    send(`notify scheduled permission=${Notification.permission}`);
    setTimeout(() => {
      try {
        const n = new Notification('Krunk spike', { body: `from a ${document.visibilityState} tab` });
        n.onshow = () => { shown = 'yes'; send(`notify shown st=${document.visibilityState} at=${Date.now()}`); };
        n.onerror = () => send('notify error');
      } catch (err) { send(`notify threw ${err.name}`); }
    }, 8000);
  });
  caseInfo = () => `notify shown=${shown}`;
}

if (kase === 'dl') {
  addEventListener('keydown', (e) => {
    if (e.key !== 'd') return;
    send('dl start');
    const link = Object.assign(document.createElement('a'), { href: '/slow?mb=20&secs=20', download: 'spike-slow.bin' });
    document.body.append(link);
    link.click();
  });
}
