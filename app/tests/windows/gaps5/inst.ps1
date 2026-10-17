param(
    [Parameter(Mandatory=$true)][ValidateSet('clean','peruser','msi','machine')][string]$Mode,
    [string]$Version='0.1.0'
)
# Run as SYSTEM (SSM). Put the updater-test build in place for the next scenario, from a clean slate.
#   clean    : remove every install (the NSIS per-machine and per-user copies, the MSI product) and their registry keys
#   peruser  : the NSIS installer as ICUAudit, /S /CurrentUser  -> %LOCALAPPDATA%\Programs\Copilot
#   machine  : the NSIS installer /S /AllUsers                  -> C:\Program Files\Copilot
#   msi      : the MSI, msiexec /i /qn, per machine             -> C:\Program Files\Copilot
# <Version> names the folder C:\ICUAudit\upd\build-<Version> (0.1.0, 0.2.0 or 0.1.0old).
$ErrorActionPreference='Continue'
$build="C:\ICUAudit\upd\build-$Version"
$setup="$build\Copilot_${Version}_x64-setup.exe"
function Clean {
    Get-Process app,cua-driver,cua-driver-uia -ErrorAction SilentlyContinue | Stop-Process -Force
    # an update installer a scenario left waiting on a dialog, and the Windows Installer client that dialog belongs to
    Get-Process -Name 'Copilot-*-installer' -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process -Name msiexec -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -ne 0 } | Stop-Process -Force
    foreach($u in @('C:\Program Files\Copilot\uninstall.exe','C:\Users\ICUAudit\AppData\Local\Programs\Copilot\uninstall.exe')){
        if(Test-Path $u){ $p=Start-Process $u -ArgumentList '/S' -PassThru; [void]$p.WaitForExit(120000); "uninstalled via $u exit $($p.ExitCode)" }
    }
    # an MSI product named Copilot
    foreach($root in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'){
        foreach($k in @(Get-ChildItem $root -ErrorAction SilentlyContinue)){
            $p=Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue
            if($p.DisplayName -eq 'Copilot' -and $k.PSChildName -match '^\{[0-9A-F-]+\}$'){
                $x=Start-Process msiexec.exe -ArgumentList '/x',$k.PSChildName,'/qn','/norestart' -PassThru -Wait; "msiexec /x $($k.PSChildName) exit $($x.ExitCode)"
            }
        }
    }
    Start-Sleep 4
    foreach($d in @('C:\Program Files\Copilot','C:\Users\ICUAudit\AppData\Local\Programs\Copilot')){ if(Test-Path $d){ cmd /c "rd /s /q `"$d`""; "removed leftover $d" } }
    reg delete "HKLM\SOFTWARE\nathanzhao" /f 2>&1 | Out-Null
    reg delete "HKU\.DEFAULT\Software\nathanzhao" /f 2>&1 | Out-Null
    $sid=(New-Object Security.Principal.NTAccount("$env:COMPUTERNAME\ICUAudit")).Translate([Security.Principal.SecurityIdentifier]).Value
    reg delete "HKU\$sid\Software\nathanzhao" /f 2>&1 | Out-Null
    foreach($k in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Copilot',"Registry::HKEY_USERS\$sid\Software\Microsoft\Windows\CurrentVersion\Uninstall\Copilot"){ Remove-Item $k -Recurse -ErrorAction SilentlyContinue }
}
function Show { foreach($d in 'C:\Program Files\Copilot','C:\Users\ICUAudit\AppData\Local\Programs\Copilot'){ if(Test-Path "$d\app.exe"){ "{0}: app.exe {1} {2}" -f $d,(Get-Item "$d\app.exe").VersionInfo.ProductVersion,(Get-AuthenticodeSignature "$d\app.exe").Status } else { "${d}: none" } } }
Clean
switch($Mode){
  'clean' { }
  'machine' { $p=Start-Process $setup -ArgumentList '/S','/AllUsers' -PassThru; if(-not $p.WaitForExit(240000)){ $p.Kill() }; "NSIS /S /AllUsers $Version exit $($p.ExitCode)" }
  'msi' {
      $msi=@(Get-ChildItem $build -Filter '*.msi')[0]
      $p=Start-Process msiexec.exe -ArgumentList '/i',"`"$($msi.FullName)`"",'/qn','/norestart' -PassThru -Wait
      "msiexec /i $($msi.Name) exit $($p.ExitCode)"
  }
  'peruser' {
      Set-Content C:\ICUAudit\inst-peruser.ps1 "`$env:__COMPAT_LAYER='RunAsInvoker'; `$p=Start-Process '$setup' -ArgumentList '/S','/CurrentUser' -PassThru; if(-not `$p.WaitForExit(240000)){ `$p.Kill() }; 'NSIS /S /CurrentUser exit ' + `$p.ExitCode | Set-Content C:\Users\Public\inst-peruser.txt"
      & C:\ICUAudit\harness\as-user.ps1 -Script C:\ICUAudit\inst-peruser.ps1 -TimeoutSeconds 300 | Out-Null
      Get-Content C:\Users\Public\inst-peruser.txt
  }
}
Show
