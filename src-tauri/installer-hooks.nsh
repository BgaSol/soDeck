; Keep user-disabled entries and their paths untouched during an in-place update.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    Push $R0
    ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "com.bgasol.sodeck"
    ${If} $R0 == '"$INSTDIR\${MAINBINARYNAME}.exe" --login-start'
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "com.bgasol.sodeck"
    ${EndIf}
    Pop $R0
  ${EndIf}
!macroend
