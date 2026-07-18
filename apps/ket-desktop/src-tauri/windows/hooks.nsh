!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Stopping the Ket tunnel service before upgrade"
  nsExec::ExecToLog 'sc.exe stop KetTunnel'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Installing the Ket tunnel service"
  nsExec::ExecToLog 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\install-tunnel-service.ps1" -ServiceBinary "$INSTDIR\ket-tunnel-service.exe" -HysteriaBinary "$INSTDIR\hysteria.exe"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "Ket could not install its tunnel service (exit code $0)."
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing the Ket tunnel service"
  nsExec::ExecToLog 'sc.exe stop KetTunnel'
  nsExec::ExecToLog 'sc.exe delete KetTunnel'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
