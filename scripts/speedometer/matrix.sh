#!/usr/bin/env bash
# Runs Speedometer 3.1 on the test VM, one run at a time:
#   matrix.sh OUTDIR TAG:BROWSER:PROFILE:BG[:OFF] [...]   (OFF: KRUNK_PERF_OFF for --probe builds)
# e.g. matrix.sh .pipeline/evidence-0.7.0/m0/speedometer A1:krunk:sp-A:0 C1:chrome:sp-chrome:0
# Each run: start (scripts/vm-070-sp.ps1), wait for results/latest.json (saved as <TAG>.json, up
# to 25 min) or a failed run (Speedometer's handleError in results/errors.txt, saved as
# <TAG>.errors.txt), stop the browser, fetch the screenshots. APP=<VM app folder> picks the build.
set -uo pipefail
cd "$(dirname "$0")/../.."
out=$1; shift
mkdir -p "$out"
V=scripts/vm.sh
for spec in "$@"; do
  IFS=: read -r tag browser prof bg off <<< "$spec"
  app=${APP:-'C:\pipeline\krunk\app'}
  app=${app//\\/\\\\}   # sed replacement: keep the backslashes
  sed "1s#.*#\$Browser = '$browser'; \$Prof = '$prof'; \$Tag = '$tag'; \$Bg = $bg; \$App = '$app'; \$Off = '${off:-}'#" scripts/vm-070-sp.ps1 > /tmp/sp-run.ps1
  $V sh "Remove-Item C:/pipeline/krunk/shots/* -Force -ErrorAction SilentlyContinue" >/dev/null </dev/null
  start=$(date +%s)
  $V ps1 /tmp/sp-run.ps1 </dev/null
  status=timeout
  for i in $(seq 1 150); do
    sleep 10
    st=$($V sh "if (Test-Path C:/pipeline/krunk/speedometer/results/latest.json) { 'done' } elseif ((Test-Path C:/pipeline/krunk/speedometer/results/errors.txt) -and (Select-String -Quiet -Pattern handleError -Path C:/pipeline/krunk/speedometer/results/errors.txt)) { 'failed' } else { 'running' }" </dev/null | tr -d '\r')
    if [ "$st" != running ]; then status=$st; break; fi
  done
  $V shot "sp-$tag-end" "$out" >/dev/null </dev/null
  $V sh "Get-Process Krunk, chrome, msedge -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null </dev/null
  for f in $($V sh "Get-ChildItem C:/pipeline/krunk/shots -Name" </dev/null | tr -d '\r'); do $V get "C:/pipeline/krunk/shots/$f" "$out/$f" </dev/null; done
  $V get "C:/pipeline/krunk/speedometer/results/errors.txt" "$out/$tag.errors.txt" </dev/null 2>/dev/null
  if [ "$status" = done ]; then
    $V get "C:/pipeline/krunk/speedometer/results/latest.json" "$out/$tag.json" </dev/null
    score=$(python3 -c "import json,sys; print(round(json.load(open(sys.argv[1]))['Score']['mean'],2))" "$out/$tag.json")
  else score=-; fi
  echo "$tag $browser $prof bg=$bg off=${off:-} $status score=$score $(( $(date +%s) - start ))s" | tee -a "$out/runs.txt"
  sleep 3
done
