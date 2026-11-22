#!/bin/bash
# Krunk 0.28.0 harness: run a script of C:\pipeline\krunk\v0280 on the VM desktop with parameters.
#   scripts/vm-0280/run.sh start.ps1 -App app271r -Prof s1 -Level strict -Url https://…
set -e
here=$(dirname "$0")
s=$1; shift
w=$(mktemp --suffix=.ps1)
trap 'rm -f "$w"' EXIT
printf '& C:\\pipeline\\krunk\\v0280\\%s %s\n' "$s" "$*" > "$w"
"$here/../vm.sh" ps1 "$w"
