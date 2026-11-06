# Packages the Windows toolchain Chromium's Linux cross build needs, on a Windows machine with
# Visual Studio 2026 Build Tools (C++ workload, ATL/MFC) and the Windows 11 SDK 10.0.28000 with
# the Debugging Tools. Uses depot_tools' package_from_installed.py (copied next to this script
# with get_toolchain_if_necessary.py; its vswhere call gets "-products *" so Build Tools are
# seen, and several VS installs are allowed). Writes <hash>.zip into -Out and package.log.
param([string]$Out = 'C:\pipeline\krunk\toolchain', [string]$SdkVersion = '10.0.28000.0')
New-Item -ItemType Directory -Force $Out | Out-Null
$log = Join-Path $Out 'package.log'
"start $(Get-Date -Format o) user=$(whoami)" | Set-Content $log
Set-Location $Out
$env:GYP_MSVS_VERSION = '2026'
$py = (Get-Command py -ErrorAction SilentlyContinue).Source
if (-not $py) { $py = 'C:\Program Files\Python312\python.exe' }
"python: $py" | Add-Content $log
& cmd /c "`"$py`" -3 `"$Out\package_from_installed.py`" 2026 -w $SdkVersion --noarm --allow_multiple_vs_installs >> `"$log`" 2>&1"
"exit $LASTEXITCODE" | Add-Content $log
Get-ChildItem $Out -Filter *.zip | ForEach-Object { "$($_.Name) $($_.Length) bytes" } | Add-Content $log
"done $(Get-Date -Format o)" | Add-Content $log
