// Krunk 0.25.0 harness: the tab marker. With ?m=<index> in the page's URL a band of one solid
// colour (24 CSS px, full width, fixed at the very top, above everything) shows which tab this
// is, so a screen recording tells which tab is on screen in every frame
// (scripts/frames/analyze.py switch). The palette comes from scripts/frames/markers.py (run it
// to regenerate this list); analyze.py selftest checks that the two agree.
'use strict';
(function () {
  const PALETTE = [
    '#1e1e55', '#558cfa', '#c31e8c', '#fac355', '#5555fa', '#8cfa8c', '#fa8c55', '#551ec3',
    '#8cc38c', '#fa5555', '#1efac3', '#8c8c55', '#fa1e55', '#1ec3c3', '#8c5555', '#c3fa1e',
    '#1e8cc3', '#8c1e55', '#c38cfa', '#1e55c3', '#55fa55', '#c355fa', '#1e1ec3', '#55c355',
    '#c31efa', '#fac3c3', '#558c55', '#8cfafa', '#fa8cc3', '#55551e', '#8cc3fa', '#fa55c3',
    '#551e1e', '#8c8cfa', '#fa1ec3', '#1efa1e', '#8c55c3', '#c3fa8c', '#1ec31e', '#8c1ec3',
    '#c3c355', '#1e8c1e', '#55fac3', '#c38c55', '#1e551e', '#55c3c3', '#c35555', '#fafa1e',
    '#558cc3', '#c31e55', '#fac31e', '#5555c3', '#8cfa55', '#fa8c1e', '#551e8c', '#8cc355',
    '#fa551e', '#1efa8c', '#8c8c1e', '#fa1e1e', '#1ec38c', '#8c551e', '#c3c3fa', '#1e8c8c',
    '#8c1e1e', '#c38cc3', '#1e558c', '#55fa1e', '#c355c3', '#1e1e8c', '#55c31e', '#c31ec3',
    '#fac38c', '#558c1e', '#8cfac3', '#fa8c8c', '#551efa', '#8cc3c3', '#fa558c', '#1efafa',
    '#8c8cc3', '#fa1e8c', '#1ec3fa', '#8c558c', '#c3fa55', '#1e8cfa', '#8c1e8c', '#c3c31e',
    '#1e55fa', '#55fa8c', '#c38c1e', '#1e1efa', '#55c38c', '#c3551e', '#fac3fa', '#558c8c',
    '#c31e1e', '#fa8cfa', '#55558c', '#8cfa1e', '#fa55fa', '#551e55', '#8cc31e', '#fa1efa',
    '#1efa55', '#8c55fa', '#c3fac3', '#1ec355', '#8c1efa', '#c3c38c', '#1e8c55', '#55fafa',
    '#c38c8c', '#1e5555', '#55c3fa', '#c3558c'
  ];
  const color = (i) => PALETTE[((i % PALETTE.length) + PALETTE.length) % PALETTE.length];
  window.KrunkMarker = { PALETTE, color };
  const m = new URLSearchParams(location.search).get('m');
  if (m === null || m === '' || isNaN(Number(m))) return;
  const band = document.createElement('div');
  band.id = 'krunk-marker';
  band.style.cssText = 'position:fixed;left:0;top:0;width:100%;height:24px;margin:0;padding:0;border:0;' +
    'z-index:2147483647;pointer-events:none;background:' + color(Number(m));
  (document.body || document.documentElement).appendChild(band);
})();
