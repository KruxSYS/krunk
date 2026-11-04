#!/usr/bin/env bash
# Drive LightBrowser on the Windows test VM (ssh host alias "windows",
# PowerShell remote shell). GUI work runs in the interactive desktop
# session through the scheduled task "LightBrowserTask", which executes
# C:\pipeline\lightbrowser\task.ps1.
#
#   scripts/vm.sh deploy                 install dist/LightBrowser-win-x64 as C:\pipeline\lightbrowser\app
#                                        (full zip on a new CEF version, else just our DLL)
#   scripts/vm.sh ps1 FILE.ps1           run a PowerShell script on the desktop (waits)
#   scripts/vm.sh start [ARGS...]        launch LightBrowser.exe on the desktop
#   scripts/vm.sh stop                   close LightBrowser (WM_CLOSE, then kill)
#   scripts/vm.sh shot NAME [DIR]        screenshot the desktop to DIR/NAME.png
#   scripts/vm.sh keys "SENDKEYS"        type with System.Windows.Forms.SendKeys
#   scripts/vm.sh click X Y              left click at screen coordinates
#   scripts/vm.sh drag X1 Y1 X2 Y2       left-button drag in small steps
#   scripts/vm.sh sh 'POWERSHELL'        run a command over ssh (non-interactive)
set -euo pipefail
VM=windows
DIR='C:\pipeline\lightbrowser'
FWD='C:/pipeline/lightbrowser'
APP="$DIR\\app"

run_ps1() { # local file
  scp -q "$1" "$VM:$FWD/task.ps1"
  ssh "$VM" "Start-ScheduledTask -TaskName LightBrowserTask; Start-Sleep -Milliseconds 500;
    \$t = 0; while ((Get-ScheduledTask -TaskName LightBrowserTask).State -eq 'Running' -and \$t -lt 120) { Start-Sleep -Milliseconds 250; \$t++ }" >/dev/null
}

tmp=$(mktemp --suffix=.ps1)
trap 'rm -f "$tmp"' EXIT
case "${1:-}" in
  deploy)
    dist="$(dirname "$0")/../dist"
    # shellcheck source=/dev/null
    source "$(dirname "$0")/../cef.lock"
    ssh "$VM" "Get-Process LightBrowser -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null || true
    have=$(ssh "$VM" "Get-Content -ErrorAction SilentlyContinue '$FWD/app/.cef-version'" | tr -d '\r' || true)
    if [ "$have" = "$CEF_VERSION" ]; then
      for f in LightBrowser.dll LightBrowser.exe README.md SECURITY.md THIRD-PARTY-NOTICES.txt; do
        scp -q "$dist/LightBrowser-win-x64/$f" "$VM:$FWD/app/$f"
      done
      echo "deployed LightBrowser.dll (CEF $CEF_VERSION already installed)"
    else
      scp -q "$dist/LightBrowser-win-x64.zip" "$VM:$FWD/LightBrowser-win-x64.zip"
      ssh "$VM" "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '$FWD/app';
        Expand-Archive -Force '$FWD/LightBrowser-win-x64.zip' '$FWD/unpack';
        Move-Item '$FWD/unpack/LightBrowser-win-x64' '$FWD/app'; Remove-Item -Recurse -Force '$FWD/unpack';
        Remove-Item '$FWD/LightBrowser-win-x64.zip'; Set-Content '$FWD/app/.cef-version' '$CEF_VERSION'" >/dev/null
      echo "deployed the full folder (CEF $CEF_VERSION)"
    fi ;;
  ps1) run_ps1 "$2" ;;
  start)
    shift
    args=""; for a in "$@"; do args+="'$a',"; done
    if [ -n "$args" ]; then argline="-ArgumentList @(${args%,})"; else argline=""; fi
    printf 'Start-Process -FilePath "%s\\LightBrowser.exe" -WorkingDirectory "%s" %s\n' "$APP" "$APP" "$argline" > "$tmp"
    run_ps1 "$tmp" ;;
  stop)
    cat > "$tmp" <<'PS'
Get-Process LightBrowser -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep -Seconds 4
Get-Process LightBrowser -ErrorAction SilentlyContinue | Stop-Process -Force
PS
    run_ps1 "$tmp" ;;
  shot)
    name=$2; out=${3:-.}
    cat > "$tmp" <<PS
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
\$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
\$bmp = New-Object System.Drawing.Bitmap \$b.Width, \$b.Height
\$g = [System.Drawing.Graphics]::FromImage(\$bmp)
\$g.CopyFromScreen(\$b.Left, \$b.Top, 0, 0, \$bmp.Size)
\$bmp.Save("$DIR\\shots\\$name.png", [System.Drawing.Imaging.ImageFormat]::Png)
PS
    run_ps1 "$tmp"
    mkdir -p "$out"
    scp -q "$VM:$FWD/shots/$name.png" "$out/$name.png"
    echo "$out/$name.png" ;;
  keys)
    printf 'Add-Type -AssemblyName System.Windows.Forms\n[System.Windows.Forms.SendKeys]::SendWait(@"\n%s\n"@)\n' "$2" > "$tmp"
    run_ps1 "$tmp" ;;
  click)
    cat > "$tmp" <<PS
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class M { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f,int x,int y,uint d,UIntPtr e); }'
[M]::SetCursorPos($2, $3) | Out-Null; Start-Sleep -Milliseconds 150
[M]::mouse_event(2,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [M]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
PS
    run_ps1 "$tmp" ;;
  drag)
    cat > "$tmp" <<PS
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class M { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f,int x,int y,uint d,UIntPtr e); }'
[M]::SetCursorPos($2, $3) | Out-Null; Start-Sleep -Milliseconds 150
[M]::mouse_event(2,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 150
for (\$i = 1; \$i -le 20; \$i++) { [M]::SetCursorPos($2 + ($4 - $2) * \$i / 20, $3 + ($5 - $3) * \$i / 20) | Out-Null; Start-Sleep -Milliseconds 30 }
Start-Sleep -Milliseconds 150; [M]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
PS
    run_ps1 "$tmp" ;;
  sh) ssh "$VM" "$2" ;;
  *) sed -n '2,17p' "$0"; exit 1 ;;
esac
