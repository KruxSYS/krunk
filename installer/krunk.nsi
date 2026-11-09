; Krunk installer (NSIS 3, built on Linux by scripts/build-dist.sh):
;   makensis -DVERSION=0.2.0 -DSRC=dist/Krunk-win-x64 -DOUT=dist/Krunk-Setup-0.2.0.exe -DICON=... installer/krunk.nsi
;
; Per user, no administrator rights: installs to %LOCALAPPDATA%\Programs\Krunk, adds a Start
; Menu shortcut (and optionally one on the desktop) and an "Apps & features" entry under HKCU.
; /S installs silently (used by Krunk's updater, which also passes /RELAUNCH to start Krunk
; again afterwards). A running Krunk is closed, or waited for, before files are replaced.
; Since 0.7.0 nothing is written until every file the package replaces is writable (an engine
; process that outlived Krunk is ended, a file still in use is moved aside); otherwise the old
; version stays as it is, the exit code is 3 and %LOCALAPPDATA%\Krunk\update-error.txt says
; why (Krunk shows it at its next start). No file is ever skipped (AllowSkipFiles off).
; The uninstaller removes the program; Krunk's data (%LOCALAPPDATA%\Krunk) is deleted only if
; the user ticks that option (never with /S).

!ifndef VERSION
  !error "VERSION is required"
!endif
!ifndef SRC
  !error "SRC (the Krunk-win-x64 folder) is required"
!endif
!ifndef OUT
  !define OUT "Krunk-Setup-${VERSION}.exe"
!endif
!ifndef ICON
  !define ICON "..\assets\brand\krunk.ico"
!endif
!ifndef SIDE_IMAGE
  !define SIDE_IMAGE "..\assets\brand\installer-side.bmp"
!endif
!ifndef HOMEPAGE
  !define HOMEPAGE "https://krunk.209.wf"
!endif

Unicode true
Target amd64-unicode
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true
; A file that cannot be written stops the installation instead of leaving the old one (0.6.0:
; a silent update kept a locked libcef.dll of 0.5.1).
AllowSkipFiles off

!define APP "Krunk"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk"
!define MUTEX "Krunk.Running"
!define WINDOW_CLASS "KrunkWindow"

Name "${APP}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\Krunk"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
BrandingText "${APP} ${VERSION}"
UninstallCaption "${APP} Uninstall"
Icon "${ICON}"
UninstallIcon "${ICON}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "CompanyName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Krunk (MIT); bundled components under their own licences"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDE_IMAGE}"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "${SIDE_IMAGE}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\Krunk.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start Krunk"
!define MUI_COMPONENTSPAGE_NODESC

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ---------------------------------------------------------------------------- running Krunk

; Waits until no Krunk runs (the browser process holds the mutex). Silent: asks a running
; Krunk to close and waits up to 30 s. Interactive: offers Retry/Cancel.
!macro WaitForKrunk UN
Function ${UN}WaitForKrunk
  StrCpy $R9 0
  loop:
    System::Call 'kernel32::OpenMutexW(i 0x00100000, i 0, w "${MUTEX}") p .r0'
    ${If} $0 == 0
      Goto done
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p r0)'
    ${If} ${Silent}
      ${If} $R9 == 0
        FindWindow $1 "${WINDOW_CLASS}" ""
        ${If} $1 != 0
          SendMessage $1 ${WM_CLOSE} 0 0 /TIMEOUT=2000
        ${EndIf}
      ${EndIf}
      IntOp $R9 $R9 + 1
      ${If} $R9 > 60
        SetErrorLevel 2
        Abort "Krunk is still running."
      ${EndIf}
      Sleep 500
      Goto loop
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Krunk is running. Close Krunk, then click Retry." /SD IDCANCEL IDRETRY loop
    Abort "Krunk is running."
  done:
  ; Its engine processes end a moment after the browser process: wait (up to 30 s) until
  ; the files they load can be replaced.
  StrCpy $R9 0
  files:
    ${If} ${FileExists} "$INSTDIR\libcef.dll"
      ClearErrors
      FileOpen $2 "$INSTDIR\libcef.dll" a
      ${If} ${Errors}
        IntOp $R9 $R9 + 1
        ${If} $R9 < 60
          Sleep 500
          Goto files
        ${EndIf}
      ${Else}
        FileClose $2
      ${EndIf}
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\Krunk.dll"
      ClearErrors
      FileOpen $2 "$INSTDIR\Krunk.dll" a
      ${If} ${Errors}
        IntOp $R9 $R9 + 1
        ${If} $R9 < 60
          Sleep 500
          Goto files
        ${EndIf}
      ${Else}
        FileClose $2
      ${EndIf}
    ${EndIf}
FunctionEnd
!macroend
!insertmacro WaitForKrunk ""
!insertmacro WaitForKrunk "un."

; Every file of the package (build-dist.sh writes the list): !insertmacro CheckFile "<path>".
; An existing file must open for writing; one still in use is renamed to <path>.old-krunk (a
; loaded program image can be renamed, not overwritten; Krunk deletes these at its next start).
; $R8 counts the files that could be neither written nor moved, $R7 names the first.
!macro CheckFile REL
  ${If} ${FileExists} "$INSTDIR\${REL}"
    ClearErrors
    FileOpen $2 "$INSTDIR\${REL}" a
    ${If} ${Errors}
      Delete "$INSTDIR\${REL}.old-krunk"
      ClearErrors
      Rename "$INSTDIR\${REL}" "$INSTDIR\${REL}.old-krunk"
      ${If} ${Errors}
        IntOp $R8 $R8 + 1
        ${If} $R7 == ""
          StrCpy $R7 "${REL}"
        ${EndIf}
      ${EndIf}
    ${Else}
      FileClose $2
    ${EndIf}
  ${EndIf}
!macroend

Function CheckAllFiles
  StrCpy $R8 0
  StrCpy $R7 ""
  !ifdef FILES_NSH
    !include "${FILES_NSH}"
  !endif
FunctionEnd

; Leaves the old version as it is and tells Krunk why (it shows the reason at its next start).
Function StopUntouched
  CreateDirectory "$LOCALAPPDATA\Krunk"
  FileOpen $3 "$LOCALAPPDATA\Krunk\update-error.txt" w
  FileWrite $3 "$R7 is in use by another program and could not be replaced; nothing was changed"
  FileClose $3
  SetErrorLevel 3
  ${IfNot} ${Silent}
    MessageBox MB_ICONSTOP "$R7 is in use by another program, so Krunk ${VERSION} could not be installed. Nothing was changed. Close the program (or restart Windows) and run the installer again."
  ${EndIf}
  Quit
FunctionEnd

Function PrepareFiles
  Call CheckAllFiles
  ${If} $R8 > 0
    ; An engine process of the old Krunk can outlive the browser for a while: end the ones
    ; running from this folder, then try once more.
    DetailPrint "Ending Krunk processes that still use $R7"
    ; (The folder goes through the environment: a quote in it cannot break the command.)
    System::Call 'kernel32::SetEnvironmentVariable(t "KRUNK_DIR", t "$INSTDIR\")'
    nsExec::Exec `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process | Where-Object { $$_.Path -and $$_.Path.StartsWith($$env:KRUNK_DIR, 'OrdinalIgnoreCase') } | Stop-Process -Force"`
    Pop $0
    Sleep 2000
    Call CheckAllFiles
  ${EndIf}
  ${If} $R8 > 0
    Call StopUntouched
  ${EndIf}
FunctionEnd

; ---------------------------------------------------------------------------- install

Section "Krunk" SecMain
  SectionIn RO
  Call WaitForKrunk
  Call PrepareFiles
  SetOutPath "$INSTDIR"
  ; 0.2.x shipped README.md and SECURITY.md; the package has no Markdown any more. Only the
  ; install folder itself (non-recursive), never the user data in $LOCALAPPDATA\Krunk.
  Delete "$INSTDIR\*.md"
  ; Up to 0.5.1 the engine shipped v8_context_snapshot.bin; Krunk's own engine build (0.5.2+)
  ; ships snapshot_blob.bin instead. The stale file would be loaded in preference: remove it.
  Delete "$INSTDIR\v8_context_snapshot.bin"
  SetOverwrite on
  File /r "${SRC}\*.*"
  ; Files moved aside above: gone now if nothing holds them any more (else Krunk removes them).
  Delete "$INSTDIR\*.old-krunk"
  Delete "$INSTDIR\locales\*.old-krunk"
  Delete "$LOCALAPPDATA\Krunk\update-error.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  SetShellVarContext current
  CreateShortCut "$SMPROGRAMS\Krunk.lnk" "$INSTDIR\Krunk.exe" "" "$INSTDIR\Krunk.exe" 0

  ; Krunk's app identity (0.11.0): the name and icon Windows shows for it, e.g. in the media
  ; overlay. Krunk.exe sets this ID and puts it on its shortcuts.
  WriteRegStr HKCU "Software\Classes\AppUserModelId\Krunk.Browser" "DisplayName" "${APP}"
  WriteRegStr HKCU "Software\Classes\AppUserModelId\Krunk.Browser" "IconUri" "$INSTDIR\Krunk.exe,0"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\Krunk.exe,0"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKCU "${UNINST_KEY}" "URLInfoAbout" "${HOMEPAGE}"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Desktop shortcut" SecDesktop
  SetShellVarContext current
  CreateShortCut "$DESKTOP\Krunk.lnk" "$INSTDIR\Krunk.exe" "" "$INSTDIR\Krunk.exe" 0
SectionEnd

Function .onInit
  ; An update (/S) keeps the user's choice: a desktop shortcut only if there was one.
  ${If} ${Silent}
    SetShellVarContext current
    ${IfNot} ${FileExists} "$DESKTOP\Krunk.lnk"
      SectionSetFlags ${SecDesktop} 0
    ${EndIf}
  ${EndIf}
FunctionEnd

; A file that could not be written after all (AllowSkipFiles off stops the installation): say so.
Function .onInstFailed
  CreateDirectory "$LOCALAPPDATA\Krunk"
  FileOpen $3 "$LOCALAPPDATA\Krunk\update-error.txt" w
  FileWrite $3 "the installer stopped because a file could not be written; run the installer again"
  FileClose $3
  SetErrorLevel 3
FunctionEnd

Function .onInstSuccess
  ; Krunk's updater runs "Krunk-Setup.exe /S /RELAUNCH" and exits; start the new version.
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/RELAUNCH" $1
  ${IfNot} ${Errors}
    Exec '"$INSTDIR\Krunk.exe"'
  ${EndIf}
FunctionEnd

; ---------------------------------------------------------------------------- uninstall

Section "un.Krunk" UnSecMain
  SectionIn RO
  Call un.WaitForKrunk
  SetShellVarContext current
  Delete "$SMPROGRAMS\Krunk.lnk"
  Delete "$DESKTOP\Krunk.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Classes\AppUserModelId\Krunk.Browser"
  ; Only a folder that holds Krunk is removed.
  ${If} ${FileExists} "$INSTDIR\Krunk.exe"
    RMDir /r "$INSTDIR"
  ${Else}
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
  ${EndIf}
SectionEnd

Section /o "un.Also delete my Krunk data" UnSecData
  ; (history, bookmarks, passwords, cookies: everything in %LOCALAPPDATA%\Krunk)
  RMDir /r "$LOCALAPPDATA\Krunk"
SectionEnd
