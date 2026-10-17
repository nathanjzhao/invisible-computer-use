$ErrorActionPreference="Continue"
New-Item -ItemType Directory -Force C:\cua | Out-Null
Start-Transcript -Path C:\cua\eval.log -Force
try {
  if(-not (Test-Path C:\cua\driver\cua-driver.exe)){
    Invoke-WebRequest -Uri "https://github.com/trycua/cua/releases/download/nightly-cua-driver-rs-v0.24.1-nightly.20260909.34311195045/cua-driver-rs-0.24.1-nightly.20260909.34311195045-windows-x86_64-binary.zip" -OutFile C:\cua\cua.zip -UseBasicParsing
    Expand-Archive C:\cua\cua.zip C:\cua\driver -Force
  }
  $drv=(Get-ChildItem C:\cua\driver -Recurse -Filter cua-driver.exe | Select-Object -First 1).FullName
  "driver=$drv" | Out-File C:\cua\where.txt
  Remove-Item C:\cua\fx.log -ErrorAction SilentlyContinue
  Start-Process powershell -ArgumentList "-NoProfile","-ExecutionPolicy","Bypass","-File","C:\cua\fixture.ps1"
  Start-Sleep 5
  & $drv stop 2>&1 | Out-Null
  Start-Process $drv -ArgumentList "serve" -WindowStyle Hidden
  Start-Sleep 6
  function Call($tool,$obj){ ($obj|ConvertTo-Json -Compress) | & $drv call $tool 2>&1 }
  $res=[ordered]@{}
  $res.version = (& $drv --version 2>&1)
  $res.perms = (Call "check_permissions" @{})
  $lw = Call "list_windows" @{}
  $res.list_windows = $lw
  $ws = ($lw | ConvertFrom-Json).windows
  $tw = $ws | Where-Object { $_.title -match "WinTarget" } | Select-Object -First 1
  "target: $($tw | ConvertTo-Json -Compress)" | Out-File C:\cua\target.txt
  if($tw){
    $pid2=$tw.pid; $wid=$tw.window_id
    $gws = Call "get_window_state" @{pid=$pid2;window_id=$wid;include_screenshot=$false;include_text=$true}
    $res.get_window_state_len = ($gws.Length)
    $els = ($gws | ConvertFrom-Json).elements
    $btn = $els | Where-Object { $_.role -match "button" } | Select-Object -First 1
    $fld = $els | Where-Object { $_.role -match "edit|text" } | Select-Object -First 1
    $sn = ($gws | ConvertFrom-Json).snapshot_id
    Clear-Content C:\cua\fx.log -ErrorAction SilentlyContinue
    $res.click = (Call "click" @{pid=$pid2;window_id=$wid;element_index=$btn.element_index;snapshot_id=$sn})
    Start-Sleep 1
    $res.set_value = (Call "set_value" @{pid=$pid2;window_id=$wid;element_index=$fld.element_index;value="hello windows";snapshot_id=$sn})
    Start-Sleep 1
    $res.type_text = (Call "type_text" @{pid=$pid2;window_id=$wid;text="typed"})
    Start-Sleep 1
    $res.fx_log = (Get-Content C:\cua\fx.log -ErrorAction SilentlyContinue) -join "; "
  }
  $res | ConvertTo-Json -Depth 6 | Out-File C:\cua\results.json -Encoding utf8
} catch { $_ | Out-File C:\cua\error.txt }
Stop-Transcript
