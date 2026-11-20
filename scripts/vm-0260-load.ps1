$Config = 'C:\pipeline\krunk\load\config.json'
# Krunk 0.26.0 page-load harness: starts the driver (scripts/perf/load/load.mjs, portable node)
# in the interactive desktop session and returns; scripts/perf/load-run.sh waits for OUT\done.
$L = 'C:\pipeline\krunk\load'
Get-Process node -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$L\*" } | Stop-Process -Force
Start-Process -FilePath "$L\node.exe" -ArgumentList "$L\load.mjs", $Config -WorkingDirectory $L -WindowStyle Minimized
