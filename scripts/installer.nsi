; Saba.rs Windows installer (NSIS).
;
; Build: makensis /DAPP_VERSION=0.1.0 scripts/installer.nsi
; The script expects the release binary and license at:
;   dist/windows/saba-rs.exe
;   LICENSE.md

!ifndef APP_VERSION
  !define APP_VERSION "0.1.0"
!endif

; File/OutFile paths resolve relative to the repository root, passed in by
; the build as ROOT_DIR (the script itself lives in scripts/).
!ifndef ROOT_DIR
  !define ROOT_DIR ".."
!endif

!define APP_NAME "Saba.rs"
!define APP_PUBLISHER "Saba.rs"
!define APP_ID "dev.saba-rs.app"

Name "${APP_NAME}"
OutFile "${ROOT_DIR}\dist\windows\saba-rs-setup-${APP_VERSION}.exe"
InstallDir "$PROGRAMFILES64\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_NAME}" "InstallDir"
RequestExecutionLevel admin
Unicode true
SetCompressor /SOLID lzma

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  File "${ROOT_DIR}\dist\windows\saba-rs.exe"
  File "${ROOT_DIR}\LICENSE.md"

  WriteRegStr HKCU "Software\${APP_NAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\saba-rs.exe"
  CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\saba-rs.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$INSTDIR\saba-rs.exe"
  Delete "$INSTDIR\LICENSE.md"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  DeleteRegKey HKCU "Software\${APP_NAME}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
SectionEnd
