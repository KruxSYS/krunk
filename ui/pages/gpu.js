// krunk://gpu: what the page can probe itself (WebGL, WebGPU, codecs) plus what the host
// knows (the setting, the switches the engine started with, the GPU process). The verdict:
// "Hardware accelerated" when the WebGL renderer names a real GPU, "Software rendering"
// when it is SwiftShader or Windows' Basic Render Driver (WARP), or the GPU is disabled.
(function () {
  'use strict';
  const $ = (id) => document.getElementById(id);

  function gl(kind) {
    const c = document.createElement('canvas');
    const ctx = c.getContext(kind, { failIfMajorPerformanceCaveat: false });
    if (!ctx) return null;
    const dbg = ctx.getExtension('WEBGL_debug_renderer_info');
    return {
      renderer: dbg ? ctx.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : ctx.getParameter(ctx.RENDERER),
      vendor: dbg ? ctx.getParameter(dbg.UNMASKED_VENDOR_WEBGL) : ctx.getParameter(ctx.VENDOR),
      version: ctx.getParameter(ctx.VERSION),
    };
  }
  const SOFTWARE = /swiftshader|basic render driver|llvmpipe|softpipe|microsoft basic|warp/i;
  function backendOf(renderer) {
    const m = /\b(D3D11|D3D9|D3D11on12|OpenGL|Vulkan|Metal)\b\)?\s*$/i.exec(renderer || '');
    return m ? 'ANGLE on ' + m[1] : (renderer ? 'unknown' : 'none');
  }

  const g1 = gl('webgl'), g2 = gl('webgl2');
  $('webgl').textContent = g1 ? g1.renderer : 'not available';
  $('webgl-vendor').textContent = g1 ? g1.vendor : '–';
  $('webgl2').textContent = g2 ? g2.version : 'not available';
  $('backend').textContent = backendOf(g1 && g1.renderer);
  if (navigator.gpu) {
    navigator.gpu.requestAdapter().then((a) => {
      if (!a) { $('webgpu').textContent = 'no adapter'; return; }
      const i = a.info || {};
      $('webgpu').textContent = [i.vendor, i.architecture, i.device, i.description].filter(Boolean).join(' · ') + (a.isFallbackAdapter ? ' (fallback)' : '');
    }).catch((e) => { $('webgpu').textContent = 'error: ' + e.message; });
  } else { $('webgpu').textContent = 'not available'; }

  // Codecs: what the engine says it can decode (MSE or <video>), the same probes as the 0.5.0
  // diagnosis plus E-AC-3. The paragraph above the table is written from these answers and
  // from which engine libcef.dll is (0.7.0; it used to be a fixed text about the old engine).
  const types = [['H.264', 'video/mp4; codecs="avc1.42E01E"'], ['HEVC', 'video/mp4; codecs="hev1.1.6.L93.B0"'], ['AAC', 'audio/mp4; codecs="mp4a.40.2"'],
    ['AC-3', 'audio/mp4; codecs="ac-3"'], ['E-AC-3', 'audio/mp4; codecs="ec-3"'], ['VP9', 'video/webm; codecs="vp9"'], ['AV1', 'video/mp4; codecs="av01.0.05M.08"'],
    ['Opus', 'audio/webm; codecs="opus"'], ['MP3', 'audio/mpeg'], ['FLAC', 'audio/mp4; codecs="flac"']];
  const v = document.createElement('video');
  const codecs = {};
  types.forEach(([name, type]) => {
    const ok = ('MediaSource' in window && MediaSource.isTypeSupported(type)) || v.canPlayType(type) === 'probably';
    codecs[name] = ok;
    const tr = document.createElement('tr');
    const th = document.createElement('th'); th.textContent = name;
    const td = document.createElement('td'); td.textContent = ok ? 'yes' : 'no'; td.className = ok ? 'yes' : 'no';
    tr.append(th, td);
    $('codecs').appendChild(tr);
  });
  const list = (names) => names.length > 1 ? names.slice(0, -1).join(', ') + ' and ' + names[names.length - 1] : names.join('');

  function codecText(integrity) {
    const yes = Object.keys(codecs).filter((n) => codecs[n]);
    const no = Object.keys(codecs).filter((n) => !codecs[n]);
    const flavor = integrity && integrity.flavor;
    let text = 'This engine decodes ' + list(yes) + '.';
    if (flavor === 'release') {
      text = "This is Krunk's own engine build with the proprietary codecs: it decodes " + list(yes) + '.';
      const platform = no.filter((n) => n === 'HEVC' || n === 'AC-3' || n === 'E-AC-3');
      if (platform.length) text += ' ' + list(platform) + (platform.length > 1 ? ' depend' : ' depends') + ' on Windows: HEVC needs a hardware decoder (a GPU that decodes it, or the HEVC Video Extensions), AC-3 and E-AC-3 need the Dolby decoder that Windows 11 24H2 and later no longer include.';
    } else if (flavor === 'minimal') {
      text = 'This is the old engine without H.264, AAC, HEVC or AC-3 (Krunk 0.5.1 and earlier): an update did not replace libcef.dll. It decodes ' + list(yes) + '.';
    } else if (no.length) {
      text += ' It does not decode ' + list(no) + '.';
    }
    $('codec-text').textContent = text;
    // The release engine decodes H.264 and AAC on every Windows: a "no" means other files.
    const broken = flavor === 'minimal' || (flavor === 'release' && (!codecs['H.264'] || !codecs.AAC)) || (integrity && integrity.checked && !integrity.healthy);
    $('codec-warning').hidden = !broken;
    if (broken) $('codec-warning').textContent = "Engine files don't match this release: run Repair (above) to reinstall them.";
  }
  codecText(null);

  const FLAVORS = { release: "Krunk's engine with proprietary codecs (this release's)", minimal: 'the old engine without H.264/AAC (Krunk 0.5.1 or earlier)', unknown: 'not the engine this release ships', missing: 'libcef.dll is missing' };
  function engineFiles(i) {
    if (!i || !i.checked) {
      $('engine-build').textContent = 'checking (runs in the background after start)…';
      return;
    }
    $('engine-build').textContent = (FLAVORS[i.flavor] || i.flavor) + (i.libcef ? ' · libcef.dll ' + i.libcef + '…' : '');
    $('engine-files').textContent = i.summary + ' (' + i.engineFiles + ' engine files' + (i.releaseChecked ? ', and the release list from the update server' : '') + ')';
    $('repair').hidden = i.healthy;
    $('repair').textContent = i.installed ? 'Repair' : 'Download Krunk';
    codecText(i);
  }
  $('repair').addEventListener('click', () => { $('repair').disabled = true; LB.post('repairEngine'); });

  function verdict(info) {
    const renderer = g1 ? g1.renderer : '';
    let text, why, cls;
    if (!info.runningWithGpu) {
      text = 'Software rendering'; cls = 'off';
      why = 'The engine was started with --disable-gpu' + (info.hardwareAcceleration ? ' (the setting is on: restart Krunk)' : ' (hardware acceleration is off in the settings)') + '.';
    } else if (!g1) {
      text = 'Software rendering'; cls = 'off'; why = 'WebGL is not available.';
    } else if (SOFTWARE.test(renderer)) {
      text = 'Software rendering'; cls = 'off';
      why = 'The renderer is a software device (' + renderer + '). On a computer with a GPU driver this says the GPU\'s name.';
    } else {
      text = 'Hardware accelerated'; cls = 'on'; why = 'The renderer is ' + renderer + '.';
    }
    $('verdict-text').textContent = text;
    $('verdict').className = 'verdict ' + cls;
    $('verdict-why').textContent = why;
  }

  LB.post('ready');
  LB.request('getGpuInfo').then((info) => {
    $('setting').textContent = info.hardwareAcceleration ? 'on' : 'off';
    $('running').textContent = info.runningWithGpu ? 'GPU enabled (no --disable-gpu)' : '--disable-gpu (software)';
    $('arguments').textContent = info.arguments;
    $('command-line').textContent = info.commandLine || '–';
    engineFiles(info.integrity);
    if (!info.integrity || !info.integrity.checked) setTimeout(() => LB.request('getGpuInfo').then((i) => engineFiles(i.integrity)), 25000);
    $('gpu-process').textContent = info.gpuProcess ? 'running' : 'none';
    $('engine').textContent = 'Chromium ' + info.chromium + ' (CEF ' + info.cef + ')';
    $('widevine').textContent = info.widevine ? info.widevine + ' (downloaded by the engine into the profile; not VMP-signed)' : info.protectedContent ? 'not downloaded yet (the engine fetches it a minute or two after start)' : 'not installed (Settings → Privacy & security → Protected content)';
    verdict(info);
  }, (e) => { $('verdict-text').textContent = 'Unavailable: ' + e.message; });
  $('open-internals').addEventListener('click', () => LB.post('openGpuInternals'));
})();
