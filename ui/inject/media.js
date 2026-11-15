// Krunk 0.6.0: the page's videos for the Picture-in-Picture button and chooser (R24).
// Runs in the main frame; same-origin frames are walked from here (a frame from another
// site keeps its own controls). Reports {lb:'media', videos:[…]} when the list changes and
// does the PiP request (and, 0.7.0, sets the volume of the video in it) on a `krunk-pip` DOM
// event: the browser fires it through DevTools with a user gesture (CEF allows PiP only with
// user activation; the shell's button is another window), so no global is exposed to the page.
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
// Event-driven (0.7.0, R24): a video only matters once it has metadata, and the media events
// (they reach a capturing listener on the document) say when; a frame says so with its own
// `load`. Up to 0.6.0 a MutationObserver on every same-origin document recorded every change
// of the page, which cost DOM-heavy pages a measurable share of their time.
function hook(doc) {
  if (docsHooked.has(doc)) return;
  docsHooked.add(doc);
  ['play', 'playing', 'pause', 'ended', 'emptied', 'loadedmetadata', 'resize', 'volumechange', 'enterpictureinpicture', 'leavepictureinpicture'].forEach(function (t) {
    doc.addEventListener(t, function (e) {
      if (!(e.target instanceof HTMLVideoElement)) return;
      if (t === 'leavepictureinpicture') left(e.target);
      schedule();
    }, true);
  });
  doc.addEventListener('load', function (e) {
    var f = e.target;
    if (!f || (f.tagName !== 'IFRAME' && f.tagName !== 'FRAME')) return;
    try { if (f.contentDocument) hook(f.contentDocument); } catch (err) { /* another site */ }
    schedule();
  }, true);
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
      info.push({ index: list.length, label: label(v), width: v.videoWidth, height: v.videoHeight, playing: !v.paused && !v.ended, pip: doc.pictureInPictureElement === v, volume: Math.round(v.volume * 100), muted: v.muted });
      list.push(v);
    }
  });
  videos = list;
  var json = JSON.stringify(info);
  if (json === last) return;
  last = json;
  host.postMessage({ lb: 'media', videos: info });
}
function videoAt(doc, x, y, depth) {
  var els = doc.elementsFromPoint(x, y);
  for (var i = 0; i < els.length; i++) {
    var el = els[i];
    if (el.tagName === 'VIDEO') return el;
    if ((el.tagName === 'IFRAME' || el.tagName === 'FRAME') && depth < 8) {
      var inner = null;
      try { inner = el.contentDocument; } catch (e) { /* another site */ }
      if (inner) {
        var r = el.getBoundingClientRect();
        return videoAt(inner, x - r.left - el.clientLeft, y - r.top - el.clientTop, depth + 1);
      }
      return null;
    }
  }
  return null;
}
function clickedVideo(x, y, src) {
  var v = null;
  try { v = videoAt(document, x, y, 0); } catch (e) { v = null; }
  if (v) return v;
  var all = [];
  docsOf().forEach(function (doc) { all.push.apply(all, doc.querySelectorAll('video')); });
  if (src) v = all.filter(function (x) { return (x.currentSrc || x.src) === src; })[0];
  return v || (all.length === 1 ? all[0] : null);
}
function schedule() { if (timer === null) timer = setTimeout(scan, 120); }

document.addEventListener('krunk-pip', function (e) {
  var d = e.detail;
  if (!d || typeof d.done !== 'function') return;
  var p;
  if (d.volume !== undefined) {
    // The volume bar next to the floating window (0.7.0, R8): the video in it.
    var pv = videos.filter(function (x) { return x.ownerDocument.pictureInPictureElement === x; })[0];
    if (pv) {
      pv.volume = Math.max(0, Math.min(100, Number(d.volume) || 0)) / 100;
      pv.muted = !!d.muted;
    }
    d.done(Promise.resolve({ set: !!pv }));
    return;
  }
  if (d.exit) {
    var cur = videos.filter(function (x) { return x.ownerDocument.pictureInPictureElement === x; })[0];
    expectedExit = !!cur;
    p = cur ? cur.ownerDocument.exitPictureInPicture().then(function () { return { exited: true }; }) : Promise.resolve({ exited: false });
  } else if (d.at) {
    // The context menu's item (0.22.0): the video under the menu's point (through
    // same-origin frames and overlays), else the one with the menu's address.
    var cv = clickedVideo(d.at.x, d.at.y, d.src);
    if (!cv) p = Promise.reject(new Error('no video at the point'));
    else if (cv.disablePictureInPicture) p = Promise.reject(new Error('disablePictureInPicture'));
    else p = cv.requestPictureInPicture().then(function (w) { return { width: w.width, height: w.height }; });
  } else {
    var t = videos[d.index];
    p = t ? t.requestPictureInPicture().then(function (w) { return { width: w.width, height: w.height }; }) : Promise.reject(new Error('no such video'));
  }
  d.done(p);
});
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', scan, { once: true });
else scan();
