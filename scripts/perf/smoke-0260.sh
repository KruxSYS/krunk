#!/usr/bin/env bash
# Krunk 0.26.0 milestone 3: smoke and regressions as ONE VM batch (no build while it runs).
#   0.26.0 = app260rel (dist/, the release build) and app260r (the harness build of the same
#   commit: DevTools port for the CDP-driven VPN steps); 0.25.5 = app260h.
# Output: evidence-0.26.0/smoke/ (shots, netlogs, logs) and evidence-0.26.0/regress/ (tabs).
set -uo pipefail
cd "$(dirname "$0")/../.."
E=$(cd .. && pwd)/.pipeline/evidence-0.26.0
S=$E/smoke; R=$E/regress; K=C:/pipeline/krunk
mkdir -p "$S" "$R"
vm() { scripts/vm.sh "$@" </dev/null; }
ps() { printf '& C:\\pipeline\\krunk\\%s\n' "$*" > /tmp/w0260.ps1; vm ps1 /tmp/w0260.ps1; }
step() { echo "== $(date +%T) $*"; }
touch /tmp/krunk-vm-busy

step deploy
KRUNK_DIST=dist KRUNK_APP=app260rel scripts/vm.sh deploy </dev/null | tail -1
vm sh "'rel libcef ' + (Get-FileHash $K/app260rel/libcef.dll).Hash.Substring(0,8)"
for f in vm-lib.ps1:lib.ps1 vm-0260-ads.ps1:vm-0260-ads.ps1 vm-0260-omni.ps1:vm-0260-omni.ps1 \
         vm-0260-smoke.ps1:vm-0260-smoke.ps1 vm-0260-netlog.ps1:vm-0260-netlog.ps1 \
         vm-www/8092/preconnect.html:www/8092/preconnect.html vm-www/8092/ads.html:www/8092/ads.html \
         vm-www/dead-wg.conf:vm-ext/dead-wg.conf; do
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

step "omnibox: 0.25.5, 0.26.0"
ps "vm-0260-omni.ps1 -App app260h -Tag old"
ps "vm-0260-omni.ps1 -App app260rel -Tag new"

step "tabs (switch, cpu): 0.25.5, 0.26.0"
OUT=$R/tabs-old scripts/perf/vm-0250-run.sh t260old 40 switch,cpu "-App C:\\pipeline\\krunk\\app260h" > $R/tabs-old.log 2>&1
OUT=$R/tabs-new scripts/perf/vm-0250-run.sh t260new 40 switch,cpu "-App C:\\pipeline\\krunk\\app260rel" > $R/tabs-new.log 2>&1
touch /tmp/krunk-vm-busy

step "smoke (release): live sites, back/forward, private, ad blocker"
ps "vm-0260-smoke.ps1 -Step live -App app260rel"

step "VPN on (OpenVPN TCP testbed) + kill switch (dead WireGuard), harness build of the release"
docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml down -v >/dev/null 2>&1
OVPN_REMOTE=127.0.0.1 docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml up -d --build >/dev/null 2>&1
for i in $(seq 1 30); do docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml exec -T ovpn cat /state/ovpn/client-tcp.ovpn > /tmp/tb-tcp.ovpn 2>/dev/null && [ -s /tmp/tb-tcp.ovpn ] && break; sleep 2; done
grep -m1 "^remote" /tmp/tb-tcp.ovpn
vm put /tmp/tb-tcp.ovpn "$K/vm-ext/tb-tcp.ovpn"
command ssh -f -N -o ExitOnForwardFailure=yes -R 127.0.0.1:8194:dind:8194 -L 19870:127.0.0.1:9222 windows
tun=$(pgrep -f "ssh -f -N -o ExitOnForwardFailure=yes -R 127.0.0.1:8194" | head -1)
d() { node scripts/perf/d0254.mjs "$@" 2>&1 | tail -2; }
ps "vm-0260-smoke.ps1 -Step vpnstart -App app260r -Name vpn"
d import; ps "vm-0260-smoke.ps1 -Step dialog -Path C:\\pipeline\\krunk\\vm-ext\\tb-tcp.ovpn"
d creds krunk krunk-test
d connect; sleep 8; d state
d nav http://localhost:8092/preconnect.html
ps "vm-0260-smoke.ps1 -Step shot -Name vpn-on"
ps "vm-0260-smoke.ps1 -Step quit -Name vpn"
vm get "$K/out/netlog/nl-vpn.json" "$S/nl-vpn.json"
ps "vm-0260-smoke.ps1 -Step vpnstart -App app260r -Name ks"
d ks on
d import; ps "vm-0260-smoke.ps1 -Step dialog -Path C:\\pipeline\\krunk\\vm-ext\\dead-wg.conf"
d connect; sleep 5; d state
d nav http://localhost:8092/preconnect.html
ps "vm-0260-smoke.ps1 -Step shot -Name killswitch"
ps "vm-0260-smoke.ps1 -Step quit -Name ks"
vm get "$K/out/netlog/nl-ks.json" "$S/nl-ks.json"
[ -n "$tun" ] && kill "$tun"
docker compose -p vpn-testbed -f tools/vpn-testbed/compose.yaml down -v >/dev/null 2>&1

step "pull shots and logs"
vm sh "Compress-Archive -Force -Path $K/shots/s0260-*.png,$K/shots/ads-*.png,$K/shots/ads-*.log,$K/shots/omni-*.png,$K/shots/omni-*.log,$K/shots/nl-*.png,$K/shots/run.log -DestinationPath $K/out/smoke0260.zip" >/dev/null
vm get "$K/out/smoke0260.zip" "$S/smoke0260.zip" && (cd "$S" && unzip -oq smoke0260.zip && rm smoke0260.zip)
rm -f /tmp/krunk-vm-busy
step done
