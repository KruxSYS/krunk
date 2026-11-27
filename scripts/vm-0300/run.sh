#!/bin/bash
# Krunk 0.30.0 harness: run a script of C:\pipeline\krunk\v0300 on the VM desktop with parameters.
#   scripts/vm-0300/run.sh start.ps1 -App app300 -Prof c1 -Url http://127.0.0.1:18112/cast.html -Dev
set -e
here=$(dirname "$0")
s=$1; shift
w=$(mktemp --suffix=.ps1)
trap 'rm -f "$w"' EXIT
printf '& C:\\pipeline\\krunk\\v0300\\%s %s\n' "$s" "$*" > "$w"
"$here/../vm.sh" ps1 "$w"
