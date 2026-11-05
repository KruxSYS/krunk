; Krunk installer (NSIS 3, built on Linux by scripts/build-dist.sh):
;   makensis -DVERSION=0.2.0 -DSRC=dist/Krunk-win-x64 -DOUT=dist/Krunk-Setup-0.2.0.exe -DICON=... installer/krunk.nsi
;
; Per user, no administrator rights: installs to %LOCALAPPDATA%\Programs\Krunk, adds a Start
; Menu shortcut (and optionally one on the desktop) and an "Apps & features" entry under HKCU.
; /S installs silently (used by Krunk's updater, which also passes /RELAUNCH to start Krunk
; again afterwards). A running Krunk is closed, or waited for, before files are replaced.
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

; ---------------------------------------------------------------------------- install

Section "Krunk" SecMain
  SectionIn RO
  Call WaitForKrunk
  SetOutPath "$INSTDIR"
  ; 0.2.x shipped README.md and SECURITY.md; the package has no Markdown any more. Only the
  ; install folder itself (non-recursive), never the user data in $LOCALAPPDATA\Krunk.
  Delete "$INSTDIR\*.md"
  SetOverwrite on
  File /r "${SRC}\*.*"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  SetShellVarContext current
  CreateShortCut "$SMPROGRAMS\Krunk.lnk" "$INSTDIR\Krunk.exe" "" "$INSTDIR\Krunk.exe" 0

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
