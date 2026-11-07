// Krunk 0.6.0: the page's videos for the Picture-in-Picture button and chooser (R24).
// Runs in the main frame; same-origin frames are walked from here (a frame from another
// site keeps its own controls). Reports {lb:'media', videos:[…]} when the list changes and
// does the PiP request itself on a `krunk-pip` DOM event: the browser fires it through
// DevTools with a user gesture (CEF allows PiP only with user activation; the shell's button
// is another window), so no global is exposed to the page.
'use strict';
if (typeof __lbHost !== 'object' || !__lbHost) return;
if (location.protocol !== 'https:' && location.protocol !== 'http:') return;
if (/(^|\.)krunk\.invalid$/.test(location.hostname)) return;   // Krunk's own pages post their own messages
if (window.top !== window) return;
var host = __lbHost;
var videos = [];
var docsHooked = new WeakSet();
var timer = null, last = '', expectedExit = false;

// This document and every same-origin frame, breadth-first.
function docsOf() {
  var out = [document];
  for (var i = 0; i < out.length && out.length < 64; i++) {
    var frames = out[i].querySelectorAll('iframe, frame');
    for (var j = 0; j < frames.length; j++) {
      try { var d = frames[j].contentDocument; if (d && d.defaultView) out.push(d); } catch (e) { /* another site */ }
    }
  }
  return out;
}
function label(v) {
  var t = v.title || v.getAttribute('aria-label') || '';
  if (!t) {
    var fig = v.closest('figure'), cap = fig && fig.querySelector('figcaption');
    if (cap) t = cap.textContent;
  }
  if (!t) {
    var src = v.currentSrc || v.src || '';
    try { t = decodeURIComponent(src.split('?')[0].split('#')[0].split('/').pop()); } catch (e) { t = ''; }
    if (/^blob:|^data:/.test(src)) t = '';
  }
  return String(t || '').replace(/\s+/g, ' ').trim().slice(0, 80);
}
function hook(doc) {
  if (docsHooked.has(doc)) return;
  docsHooked.add(doc);
  ['play', 'playing', 'pause', 'ended', 'emptied', 'loadedmetadata', 'resize', 'enterpictureinpicture', 'leavepictureinpicture'].forEach(function (t) {
    doc.addEventListener(t, function (e) {
      if (!(e.target instanceof HTMLVideoElement)) return;
      if (t === 'leavepictureinpicture') left(e.target);
      schedule();
    }, true);
  });
  if (typeof MutationObserver === 'function') new MutationObserver(schedule).observe(doc.documentElement, { childList: true, subtree: true });
}
// The floating window's close button pauses the video; "Back to tab" leaves it playing
// (as does the browser's own exit, which announces itself first).
function left(v) {
  var expected = expectedExit;
  expectedExit = false;
  setTimeout(function () { if (!expected && !v.paused && !v.ended) host.postMessage({ lb: 'pipBackToTab' }); }, 150);
}
function scan() {
  timer = null;
  var list = [], info = [];
  docsOf().forEach(function (doc) {
    hook(doc);
    var vs = doc.querySelectorAll('video');
    for (var i = 0; i < vs.length && list.length < 32; i++) {
      var v = vs[i];
      if (v.readyState < 1 || v.disablePictureInPicture) continue;
      info.push({ index: list.length, label: label(v), width: v.videoWidth, height: v.videoHeight, playing: !v.paused && !v.ended, pip: doc.pictureInPictureElement === v });
      list.push(v);
    }
  });
  videos = list;
  var json = JSON.stringify(info);
  if (json === last) return;
  last = json;
  host.postMessage({ lb: 'media', videos: info });
}
function schedule() { if (timer === null) timer = setTimeout(scan, 120); }

document.addEventListener('krunk-pip', function (e) {
  var d = e.detail;
  if (!d || typeof d.done !== 'function') return;
  var p;
  if (d.exit) {
    var cur = videos.filter(function (x) { return x.ownerDocument.pictureInPictureElement === x; })[0];
    expectedExit = !!cur;
    p = cur ? cur.ownerDocument.exitPictureInPicture().then(function () { return { exited: true }; }) : Promise.resolve({ exited: false });
  } else {
    var t = videos[d.index];
    p = t ? t.requestPictureInPicture().then(function (w) { return { width: w.width, height: w.height }; }) : Promise.reject(new Error('no such video'));
  }
  d.done(p);
});
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', scan, { once: true });
else scan();
