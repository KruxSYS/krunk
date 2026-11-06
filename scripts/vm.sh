#!/usr/bin/env bash
# Drive Krunk on the Windows test VM (ssh host alias "windows",
# PowerShell remote shell). GUI work runs in the interactive desktop
# session through the scheduled task "KrunkTask", which executes
# C:\pipeline\krunk\task.ps1.
#
#   scripts/vm.sh deploy                 install dist/Krunk-win-x64 as C:\pipeline\krunk\app
#                                        (full zip on a new CEF version, else just our DLL)
#   scripts/vm.sh ps1 FILE.ps1           run a PowerShell script on the desktop (waits)
#   scripts/vm.sh start [ARGS...]        launch Krunk.exe on the desktop
#   scripts/vm.sh stop                   close Krunk (WM_CLOSE, then kill)
#   scripts/vm.sh shot NAME [DIR]        screenshot the desktop to DIR/NAME.png
#   scripts/vm.sh keys "SENDKEYS"        type with System.Windows.Forms.SendKeys
#   scripts/vm.sh click X Y              left click at screen coordinates
#   scripts/vm.sh drag X1 Y1 X2 Y2       left-button drag in small steps
#   scripts/vm.sh sh 'POWERSHELL'        run a command over ssh (non-interactive)
#   scripts/vm.sh put LOCAL REMOTE       copy a file to the VM (REMOTE like C:/pipeline/krunk/x)
#   scripts/vm.sh get REMOTE LOCAL       copy a file from the VM
# Batch work into one ps1 per round where possible (each call is a session on the VM).
set -euo pipefail
VM=windows
# One shared SSH connection for every call: the VM's sshd stops accepting
# logins when many short sessions are opened in quick succession.
SSHOPTS=(-o ControlMaster=auto -o "ControlPath=$HOME/.ssh/cm-krunk-%r@%h-%p" -o ControlPersist=10m)
ssh() { command ssh "${SSHOPTS[@]}" "$@"; }
scp() { command scp "${SSHOPTS[@]}" "$@"; }
DIR='C:\pipeline\krunk'
FWD='C:/pipeline/krunk'
APP="$DIR\\app"

run_ps1() { # local file
  scp -q "$1" "$VM:$FWD/task.ps1"
  ssh "$VM" "Start-ScheduledTask -TaskName KrunkTask; Start-Sleep -Milliseconds 500;
    \$t = 0; while ((Get-ScheduledTask -TaskName KrunkTask).State -eq 'Running' -and \$t -lt 2400) { Start-Sleep -Milliseconds 250; \$t++ }" >/dev/null
}

tmp=$(mktemp --suffix=.ps1)
trap 'rm -f "$tmp"' EXIT
case "${1:-}" in
  deploy)
    dist="$(dirname "$0")/../dist"
    # shellcheck source=/dev/null
    source "$(dirname "$0")/../cef.lock"
    ssh "$VM" "Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force" >/dev/null || true
    # The marker is the archive's SHA-256, not the CEF version: 0.5.2 switched to a self-built
    # engine of the same version.
    have=$(ssh "$VM" "Get-Content -ErrorAction SilentlyContinue '$FWD/app/.cef-version'" | tr -d '\r' || true)
    if [ "$have" = "$CEF_SHA256" ]; then
      for f in Krunk.dll Krunk.exe THIRD-PARTY-NOTICES.txt; do
        scp -q "$dist/Krunk-win-x64/$f" "$VM:$FWD/app/$f"
      done
      echo "deployed Krunk.dll (CEF $CEF_VERSION already installed)"
    else
      scp -q "$dist/Krunk-win-x64.zip" "$VM:$FWD/Krunk-win-x64.zip"
      ssh "$VM" "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '$FWD/app';
        Expand-Archive -Force '$FWD/Krunk-win-x64.zip' '$FWD/unpack';
        Move-Item '$FWD/unpack/Krunk-win-x64' '$FWD/app'; Remove-Item -Recurse -Force '$FWD/unpack';
        Remove-Item '$FWD/Krunk-win-x64.zip'; Set-Content '$FWD/app/.cef-version' '$CEF_SHA256'" >/dev/null
      echo "deployed the full folder (CEF $CEF_VERSION, archive $CEF_SHA256)"
    fi ;;
  ps1) run_ps1 "$2" ;;
  start)
    shift
    args=""; for a in "$@"; do args+="'$a',"; done
    if [ -n "$args" ]; then argline="-ArgumentList @(${args%,})"; else argline=""; fi
    printf 'Start-Process -FilePath "%s\\Krunk.exe" -WorkingDirectory "%s" %s\n' "$APP" "$APP" "$argline" > "$tmp"
    run_ps1 "$tmp" ;;
  stop)
    cat > "$tmp" <<'PS'
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep -Seconds 4
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
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
  put) scp -q "$2" "$VM:$3" ;;
  get) scp -q "$VM:$2" "$3" ;;
  *) sed -n '2,21p' "$0"; exit 1 ;;
esac
