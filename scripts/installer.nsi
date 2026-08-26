; Ryusei Windows installer. Build with /DAPP_VERSION and /DROOT_DIR.
!ifndef APP_VERSION
  !define APP_VERSION "0.1.0"
!endif
!ifndef ROOT_DIR
  !define ROOT_DIR ".."
!endif
!define APP_NAME "Ryusei"
!define APP_PUBLISHER "Ryusei"
!define APP_ID "dev.ryusei.app"
!define APP_EXE "ryusei.exe"

Name "${APP_NAME}"
OutFile "${ROOT_DIR}\dist\windows\ryusei-v${APP_VERSION}-windows-x86_64-setup.exe"
InstallDir "$PROGRAMFILES64\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_NAME}" "InstallDir"
RequestExecutionLevel admin
Unicode true
SetCompressor /SOLID lzma
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

!macro RegisterExtension EXT PROGID DESCRIPTION
  WriteRegStr HKLM "Software\Classes\.${EXT}" "" "${PROGID}"
  WriteRegStr HKLM "Software\Classes\${PROGID}" "" "${DESCRIPTION}"
  WriteRegStr HKLM "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKLM "Software\Classes\${PROGID}\shell\open\command" "" '$"$INSTDIR\${APP_EXE}$" $"%1$"'
!macroend
!macro UnregisterExtension EXT PROGID
  DeleteRegKey HKLM "Software\Classes\${PROGID}"
  DeleteRegKey HKLM "Software\Classes\.${EXT}"
!macroend

Section "Install"
  SetOutPath "$INSTDIR"
  File "${ROOT_DIR}\dist\windows\${APP_EXE}"
  File "${ROOT_DIR}\LICENSE.md"
  WriteRegStr HKCU "Software\${APP_NAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  !insertmacro RegisterExtension "sgf" "Ryusei.SGF" "Smart Game Format"
  !insertmacro RegisterExtension "ngf" "Ryusei.NGF" "CyberOro Go File"
  !insertmacro RegisterExtension "gib" "Ryusei.GIB" "Tygem Go File"
  !insertmacro RegisterExtension "ugf" "Ryusei.UGF" "PandaNet UGF File"
  System::Call 'Shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\LICENSE.md"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  !insertmacro UnregisterExtension "sgf" "Ryusei.SGF"
  !insertmacro UnregisterExtension "ngf" "Ryusei.NGF"
  !insertmacro UnregisterExtension "gib" "Ryusei.GIB"
  !insertmacro UnregisterExtension "ugf" "Ryusei.UGF"
  DeleteRegKey HKCU "Software\${APP_NAME}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
  System::Call 'Shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd
