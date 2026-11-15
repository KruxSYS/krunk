// Certificate interstitial. The query names what the tab hit: ?url= (https), err= (the engine's
// net error), sha= (the leaf certificate's SHA-256), ov=1 when the engine allows continuing,
// p=1 in a private tab. The host checks "Continue anyway" against the error it recorded for
// this tab, so nothing here is trusted.
(function () {
  'use strict';
  const q = new URLSearchParams(location.search);
  const target = q.get('url') || '';
  const sha = (q.get('sha') || '').toLowerCase();
  const overridable = q.get('ov') === '1' && /^[0-9a-f]{64}$/.test(sha);
  const isPrivate = q.get('p') === '1';
  // Chromium's ERR_CERT_* codes (net/base/net_error_list.h).
  const REASONS = {
    '-200': 'the certificate is for a different site',
    '-201': 'the certificate has expired or is not yet valid',
    '-202': 'the certificate is not issued by an authority Krunk trusts',
    '-203': 'the certificate contains errors',
    '-204': 'the certificate can’t be checked for revocation',
    '-205': 'the certificate’s revocation status could not be checked',
    '-206': 'the certificate was revoked',
    '-207': 'the certificate is invalid',
    '-208': 'the certificate uses a weak signature algorithm',
    '-210': 'the certificate’s name is not unique',
    '-211': 'the certificate uses a weak key',
    '-212': 'the certificate violates a name constraint',
    '-213': 'the certificate is valid for too long',
    '-214': 'the certificate is not publicly logged (Certificate Transparency)',
    '-215': 'the certificate authority is no longer trusted',
    '-217': 'the certificate was issued by known interception software',
  };
  let host = '';
  try {
    const u = new URL(target);
    if (u.protocol === 'https:') host = u.host;
  } catch (e) { /* invalid */ }
  const $ = (id) => document.getElementById(id);
  $('host').textContent = host || '(invalid address)';
  $('reason').textContent = REASONS[q.get('err')] || 'the certificate is not valid (error ' + (q.get('err') || 'unknown') + ')';
  $('url').textContent = target;
  // In groups of four, to compare by eye.
  $('sha').textContent = /^[0-9a-f]{64}$/.test(sha) ? sha.match(/.{4}/g).join(' ') : 'not available';
  const canContinue = !!host && overridable;
  $('continue').hidden = !canContinue;
  $('remember-row').hidden = !canContinue || isPrivate;
  $('remember-desc').hidden = !canContinue || isPrivate;
  $('private-desc').hidden = !canContinue || !isPrivate;
  $('no-continue').hidden = canContinue;
  $('continue').addEventListener('click', function () {
    LB.post('certProceed', { url: target, sha256: sha, remember: !isPrivate && $('remember').checked });
  });
  $('back').addEventListener('click', function () { LB.post('certBack', {}); });
  $('back').focus();
  LB.post('ready');
})();
