# Krunk 0.28.1 end-to-end: acceptance pages in the running build (after start.ps1). -Part picks a set.
param([string]$Part = 'strict')
. C:\pipeline\krunk\lib.ps1
function ShotAs([string]$n) { Shot "e2e-$n" }
if ($Part -eq 'strict') {
  Click 640 30 300; Go 'https://plique.io/hack/plique-bf6' 9000
  Click 1100 400 300; Keys '{PGDN}' 2500; Click 319 321 5000; ShotAs 'a1-plique-strict'
  Click 640 30 300; Go 'http://127.0.0.1:18111/embeds-fixture.html' 7000
  Click 1100 400 300; Keys '{PGDN}' 1500; ShotAs 'a2-embeds-placeholder'
  Click 216 238 5000; ShotAs 'a3-embeds-after-load'
  Click 640 30 300; Go 'https://nopecha.com/demo/cloudflare' 15000; Click 213 441 12000; ShotAs 'a4-turnstile'
}
if ($Part -eq 'standard') {
  Click 640 30 300; Go 'https://plique.io/hack/plique-bf6' 9000
  Click 1100 400 300; Keys '{PGDN}' 2500; Click 319 321 5000; ShotAs 'a5-plique-standard'
  Click 640 30 300; Go 'https://abrahamjuliot.github.io/creepjs/' 20000
  Click 1100 400 300; Keys '{PGDN}' 300; Keys '{DOWN}{DOWN}{DOWN}' 300; Keys '{UP}{UP}{UP}{UP}' 1000; ShotAs 'a6-creepjs'
}
