#!/usr/bin/env bash
# Runs VM scripts and collects shots: run.sh OUTDIR name|script|sed-expr ...
cd "$(dirname "$0")/.."
out=$1; shift
for spec in "$@"; do
  IFS='|' read -r name script sub <<< "$spec"
  d=$out/$name; mkdir -p $d; rm -f $d/*
  if [ -n "$sub" ]; then sed "$sub" scripts/$script > /tmp/vmrun/$name.ps1; else cp scripts/$script /tmp/vmrun/$name.ps1; fi
  scripts/vm.sh sh "Remove-Item C:/pipeline/krunk/shots/* -Force -ErrorAction SilentlyContinue" >/dev/null </dev/null
  start=$(date +%s)
  scripts/vm.sh ps1 /tmp/vmrun/$name.ps1 </dev/null
  for f in $(scripts/vm.sh sh "Get-ChildItem C:/pipeline/krunk/shots -Name" </dev/null | tr -d '\r'); do scripts/vm.sh get "C:/pipeline/krunk/shots/$f" $d/$f </dev/null; done
  [ -f $d/run.log ] && iconv -f cp1252 -t utf-8 $d/run.log > $d/r && mv $d/r $d/run.log
  echo "== $name ($(( $(date +%s) - start )) s): $(ls $d | wc -l) files"
done
