param(
    [Parameter(Mandatory=$true)][ValidateSet('machine','user','clean')][string]$Mode,
    [string]$Version='0.1.0',
    [string]$Out='C:\ICUAudit\updinstall-out.txt'
)
# SYSTEM. Put the 0.1.0 (or given) updater-test build in place for the next scenario, from a clean slate.
#   clean   : uninstall whatever is installed (per machine and the user's per-user copy), remove its registry keys
#   machine : NSIS /S /AllUsers -> C:\Program Files\Copilot
#   user    : NSIS /S /CurrentUser as ICUAudit (through as-user) -> %LOCALAPPDATA%\Programs\Copilot
$ErrorActionPreference='Continue'
$setup="C:\ICUAudit\upd\build-$Version\Copilot_${Version}_x64-setup.exe"
function Clean {
    Get-Process app,cua-driver,cua-driver-uia -ErrorAction SilentlyContinue | Stop-Process -Force
    foreach($u in @('C:\Program Files\Copilot\uninstall.exe','C:\Users\ICUAudit\AppData\Local\Programs\Copilot\uninstall.exe')){
        if(Test-Path $u){ $p=Start-Process $u -ArgumentList '/S' -PassThru; [void]$p.WaitForExit(120000); "uninstalled via $u exit $($p.ExitCode)" }
    }
    Start-Sleep 4
    foreach($d in @('C:\Program Files\Copilot','C:\Users\ICUAudit\AppData\Local\Programs\Copilot')){ if(Test-Path $d){ cmd /c "rd /s /q `"$d`""; "removed leftover $d" } }
    reg delete "HKLM\SOFTWARE\nathanzhao" /f 2>&1 | Out-Null
    reg delete "HKU\.DEFAULT\Software\nathanzhao" /f 2>&1 | Out-Null
    $sid=(New-Object Security.Principal.NTAccount("$env:COMPUTERNAME\ICUAudit")).Translate([Security.Principal.SecurityIdentifier]).Value
    reg delete "HKU\$sid\Software\nathanzhao" /f 2>&1 | Out-Null
    foreach($k in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Copilot',"Registry::HKEY_USERS\$sid\Software\Microsoft\Windows\CurrentVersion\Uninstall\Copilot"){ Remove-Item $k -Recurse -ErrorAction SilentlyContinue }
}
$o=@()
switch($Mode){
  'clean'   { $o+=Clean; $o+="clean: machine dir $(Test-Path 'C:\Program Files\Copilot'), user dir $(Test-Path 'C:\Users\ICUAudit\AppData\Local\Programs\Copilot')" }
  'machine' {
      $o+=Clean
      $p=Start-Process $setup -ArgumentList '/S','/AllUsers' -PassThru; if(-not $p.WaitForExit(240000)){ $p.Kill() }
      $o+="NSIS /S /AllUsers $Version exit $($p.ExitCode)"
      $o+=@(Get-ChildItem 'C:\Program Files\Copilot' -File | ForEach-Object { "{0} {1} {2}" -f $_.Name,(Get-AuthenticodeSignature $_.FullName).Status,(Get-Item $_.FullName).VersionInfo.ProductVersion })
  }
  'user' {
      $o+=Clean
      Set-Content C:\ICUAudit\updinstall-user.ps1 "`$p=Start-Process '$setup' -ArgumentList '/S','/CurrentUser' -PassThru; if(-not `$p.WaitForExit(240000)){ `$p.Kill() }; 'NSIS /S /CurrentUser exit ' + `$p.ExitCode | Set-Content C:\Users\Public\updinstall-user.txt"
      & C:\ICUAudit\harness\as-user.ps1 -Script C:\ICUAudit\updinstall-user.ps1 -TimeoutSeconds 300 | Out-Null
      $o+=(Get-Content C:\Users\Public\updinstall-user.txt)
      $o+=@(Get-ChildItem 'C:\Users\ICUAudit\AppData\Local\Programs\Copilot' -File | ForEach-Object { "{0} {1} {2}" -f $_.Name,(Get-AuthenticodeSignature $_.FullName).Status,(Get-Item $_.FullName).VersionInfo.ProductVersion })
  }
}
$o | Set-Content $Out
$o
