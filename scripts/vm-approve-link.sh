#!/usr/bin/env bash
# Approves the account link a VM flow started (0.9.0): waits for C:\pipeline\krunk\code.txt,
# approves the code on staging as the test account (server/e2e/approve.cjs makes and confirms it
# first if needed), then writes C:\pipeline\krunk\approved for the VM script.
#   scripts/vm-approve-link.sh EMAIL PASSWORD [seconds]     (run in the background)
cd "$(dirname "$0")/.." || exit 1
email=$1; pw=$2; until=$(( $(date +%s) + ${3:-180} ))
while [ "$(date +%s)" -lt "$until" ]; do
  code=$(scripts/vm.sh sh "Get-Content C:/pipeline/krunk/code.txt -ErrorAction SilentlyContinue" </dev/null | tr -d '\r\n ')
  if [ -n "$code" ]; then
    scripts/vm.sh sh "Remove-Item C:/pipeline/krunk/code.txt -ErrorAction SilentlyContinue" </dev/null >/dev/null
    if NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
      node server/e2e/approve.cjs http://dind:10017 stage-browser "$email" "$pw" "$code" </dev/null; then
      scripts/vm.sh sh "Set-Content C:/pipeline/krunk/approved 1" </dev/null >/dev/null
      echo "approved $code"
      exit 0
    fi
    echo "approval of $code failed"
    exit 1
  fi
  sleep 3
done
echo "no code within the time"
exit 1
