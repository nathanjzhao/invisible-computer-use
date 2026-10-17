param(
    [Parameter(Mandatory=$true)][ValidateSet('msi-install','msi-uninstall','nsis-install','nsis-uninstall','list')][string]$Step,
    [string]$Bundle='C:\ICUAudit\out-A\bundle'
)
# SYSTEM. Install or remove the REAL-signed packages per machine, and list what landed and how each PE is signed.
$ErrorActionPreference='Continue'
$st=(Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' | Sort-Object FullName | Select-Object -Last 1).FullName
$dir='C:\Program Files\Copilot'
function Listing([string]$d){
    "--- $d"
    foreach($f in @(Get-ChildItem $d -Recurse -File -ErrorAction SilentlyContinue)){
        $isPE=$false; try{ $fs=[IO.File]::OpenRead($f.FullName); $b=New-Object byte[] 2; [void]$fs.Read($b,0,2); $fs.Dispose(); $isPE=($b[0] -eq 0x4D -and $b[1] -eq 0x5A) }catch{}
        if($isPE){
            $s=Get-AuthenticodeSignature $f.FullName
            $v=(& $st verify /pa /v $f.FullName 2>&1 | Out-String)
            $ts=[regex]::Match($v,'timestamped: ([^\r\n]+)').Groups[1].Value
            "{0} | PE | {1} | {2} | {3} | ts {4}" -f $f.Name,$s.Status,$s.SignerCertificate.Subject,$s.SignerCertificate.Thumbprint,$ts
        } else { "{0} | not a PE | {1} bytes" -f $f.Name,$f.Length }
    }
}
switch($Step){
  'msi-install' {
      $msi=@(Get-ChildItem "$Bundle\msi\*.msi")[0].FullName
      "installing $msi per machine"
      $p=Start-Process msiexec.exe -ArgumentList '/i',"`"$msi`"",'/qn','/norestart','/l*v','C:\ICUAudit\msi-install.log' -Wait -PassThru
      "msiexec exit $($p.ExitCode)"
      Listing $dir
      "uninstall entries:"; Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq 'Copilot' } | Select-Object DisplayName,DisplayVersion,InstallLocation,UninstallString,Publisher | Format-List | Out-String
  }
  'msi-uninstall' {
      $msi=@(Get-ChildItem "$Bundle\msi\*.msi")[0].FullName
      $p=Start-Process msiexec.exe -ArgumentList '/x',"`"$msi`"",'/qn','/norestart','/l*v','C:\ICUAudit\msi-uninstall.log' -Wait -PassThru
      "msiexec /x exit $($p.ExitCode); install dir exists after: $(Test-Path $dir)"
  }
  'nsis-install' {
      $exe=@(Get-ChildItem "$Bundle\nsis\*.exe")[0].FullName
      $p=Start-Process $exe -ArgumentList '/S','/AllUsers' -PassThru; if(-not $p.WaitForExit(240000)){ $p.Kill(); throw 'installer exceeded 4 min' }
      "NSIS /S /AllUsers exit $($p.ExitCode)"
      Listing $dir
  }
  'nsis-uninstall' {
      $u="$dir\uninstall.exe"
      if(Test-Path $u){ $p=Start-Process $u -ArgumentList '/S' -PassThru; [void]$p.WaitForExit(120000); Start-Sleep 5 }
      "uninstalled; install dir exists after: $(Test-Path $dir)"
  }
  'list' { Listing $dir }
}
