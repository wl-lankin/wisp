!macro NSIS_HOOK_POSTINSTALL
  ; Let other Wisp devices on private networks reach this one.
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Wisp"'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Wisp" dir=in action=allow program="$INSTDIR\Wisp.exe" enable=yes profile=private protocol=UDP'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Wisp"'
!macroend
