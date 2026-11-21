#!/usr/bin/env bash
# Krunk 0.26.0: re-run of the failed/unproven smoke checks after the two blocker fixes, ONE VM
# batch on the fixed builds (app260rel = dist/ release, app260r = harness build of the same
# commit for the CDP-driven VPN steps; 0.25.5 = app260h for the ad blocker comparison).
# Output: evidence-0.26.0/smoke2/ and regress/tabs-fix/.
set -uo pipefail
cd "$(dirname "$0")/../.."
E=$(cd .. && pwd)/.pipeline/evidence-0.26.0
S=$E/smoke2; R=$E/regress; K=C:/pipeline/krunk
mkdir -p "$S" "$R"
vm() { scripts/vm.sh "$@" </dev/null; }
ps() { printf '& C:\\pipeline\\krunk\\%s\n' "$*" > /tmp/w0260.ps1; vm ps1 /tmp/w0260.ps1; }
step() { echo "== $(date +%T) $*"; }
touch /tmp/krunk-vm-busy

step deploy
KRUNK_DIST=dist KRUNK_APP=app260rel scripts/vm.sh deploy </dev/null | tail -1
KRUNK_DIST=target/harness-dist KRUNK_APP=app260r scripts/vm.sh deploy </dev/null | tail -1
vm sh "Get-Process firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Remove-Item -Force $K/shots/*.png -ErrorAction SilentlyContinue; Remove-Item -Force $K/shots/run.log -ErrorAction SilentlyContinue"
for f in vm-lib.ps1:lib.ps1 vm-0260-ads.ps1:vm-0260-ads.ps1 vm-0260-omni.ps1:vm-0260-omni.ps1 \
         vm-0260-smoke.ps1:vm-0260-smoke.ps1 vm-0260-netlog.ps1:vm-0260-netlog.ps1 \
         vm-www/8092/preconnect.html:www/8092/preconnect.html vm-www/8092/ads.html:www/8092/ads.html; do
  vm put "scripts/${f%%:*}" "$K/${f##*:}"
done

step "netlog (release): on, private"
for c in on private; do
  ps "vm-0260-netlog.ps1 -Case $c -App app260rel"
  vm get "$K/out/netlog/nl-$c.json" "$S/nl-rel-$c.json"
done

step "ad blocker: 0.25.5, 0.26.0"
ps "vm-0260-ads.ps1 -App app260h -Tag old"
ps "vm-0260-ads.ps1 -App app260rel -Tag new"

step "omnibox: 0.26.0"
ps "vm-0260-omni.ps1 -App app260rel -Tag new"

step "tabs (switch, cpu): 0.26.0 release"
OUT=$R/tabs-fix scripts/perf/vm-0250-run.sh t260fix 40 switch,cpu "-App C:\\pipeline\\krunk\\app260rel" > $R/tabs-fix.log 2>&1
touch /tmp/krunk-vm-busy

step "smoke (release): live sites, back/forward, private, ad blocker"
ps "vm-0260-smoke.ps1 -Step live -App app260rel"

step "VPN on (OpenVPN TCP testbed, pre-imported) + kill switch engaged, harness build"
docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml down -v >/dev/null 2>&1
OVPN_REMOTE=127.0.0.1 docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml up -d --build >/dev/null 2>&1
for i in $(seq 1 30); do docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml exec -T ovpn cat /state/ovpn/client-tcp.ovpn > /tmp/tb-tcp.ovpn 2>/dev/null && [ -s /tmp/tb-tcp.ovpn ] && break; sleep 2; done
grep -m1 "^remote" /tmp/tb-tcp.ovpn
vm put /tmp/tb-tcp.ovpn "$K/vm-ext/tb-tcp.ovpn"
command ssh -f -N -o ExitOnForwardFailure=yes -R 127.0.0.1:8194:dind:8194 -L 127.0.0.1:19870:127.0.0.1:9222 windows
tun=$(ps -C ssh -o pid=,args= | awk '/127.0.0.1:8194:dind:8194/ {print $1}' | head -1)
d() { timeout 120 node scripts/perf/d0254.mjs "$@" 2>&1 | tail -2; }
ps "vm-0260-smoke.ps1 -Step vpnstart -App app260r -Name vpn -Path C:\\pipeline\\krunk\\vm-ext\\tb-tcp.ovpn"
d state
d creds krunk krunk-test
d connect; sleep 8; d state
d nav http://localhost:8092/preconnect.html
ps "vm-0260-smoke.ps1 -Step shot -Name vpn-on"
ps "vm-0260-smoke.ps1 -Step quit -Name vpn"
vm get "$K/out/netlog/nl-vpn.json" "$S/nl-vpn.json"
ps "vm-0260-smoke.ps1 -Step vpnstart -App app260r -Name ks"
d ks on; sleep 3; d state
d nav http://localhost:8092/preconnect.html
ps "vm-0260-smoke.ps1 -Step shot -Name killswitch"
ps "vm-0260-smoke.ps1 -Step quit -Name ks"
vm get "$K/out/netlog/nl-ks.json" "$S/nl-ks.json"
[ -n "$tun" ] && kill "$tun"
docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml down -v >/dev/null 2>&1

step "pull shots and logs"
vm sh "Compress-Archive -Force -Path $K/shots/*.png,$K/shots/ads-*.log,$K/shots/omni-*.log,$K/shots/run.log -DestinationPath $K/out/smoke0260b.zip" >/dev/null
vm get "$K/out/smoke0260b.zip" "$S/smoke0260b.zip" && (cd "$S" && unzip -oq smoke0260b.zip && rm smoke0260b.zip)
vm sh "Get-ChildItem C:/pipeline/krunk/lp-*/Krunk/krunk.log | Select-String 'blocked host\(s\) handed' | % { \$_.Path + ': ' + \$_.Line }" > "$S/push-times.txt"
rm -f /tmp/krunk-vm-busy
step done
