$ErrorActionPreference="Continue"
New-Item -ItemType Directory -Force C:\cua | Out-Null
Start-Transcript -Path C:\cua\eval2.log -Force
Add-Type @"
using System;using System.Runtime.InteropServices;
public class Fg {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
"@
function FgInfo(){ $h=[Fg]::GetForegroundWindow(); $p=0; [Fg]::GetWindowThreadProcessId($h,[ref]$p)|Out-Null; @{hwnd=$h.ToInt64();pid=[int]$p} }
try {
  if(-not (Test-Path C:\cua\driver\cua-driver.exe)){
    Invoke-WebRequest -Uri "https://github.com/trycua/cua/releases/download/nightly-cua-driver-rs-v0.24.1-nightly.20260909.34311195045/cua-driver-rs-0.24.1-nightly.20260909.34311195045-windows-x86_64-binary.zip" -OutFile C:\cua\cua.zip -UseBasicParsing
    Expand-Archive C:\cua\cua.zip C:\cua\driver -Force
  }
  $drv=(Get-ChildItem C:\cua\driver -Recurse -Filter cua-driver.exe|Select-Object -First 1).FullName
  Remove-Item C:\cua\fx2.log -ErrorAction SilentlyContinue
  Start-Process powershell -ArgumentList "-NoProfile","-ExecutionPolicy","Bypass","-File","C:\cua\fixture2.ps1"
  Start-Sleep 5
  $ready=(Get-Content C:\cua\fx2.log|Where-Object {$_ -match '"hit":"ready"'}|Select-Object -First 1|ConvertFrom-Json)
  $targetHwnd=[int64]$ready.v
  # launch occluder ON TOP, seize foreground, cover the target
  Start-Process powershell -ArgumentList "-NoProfile","-ExecutionPolicy","Bypass","-File","C:\cua\occluder.ps1"
  Start-Sleep 4
  & $drv stop 2>&1|Out-Null
  Start-Process $drv -ArgumentList "serve" -WindowStyle Hidden
  Start-Sleep 6
  function Call($tool,$obj){ ($obj|ConvertTo-Json -Compress)|& $drv call $tool 2>$null }
  $res=[ordered]@{}
  $lw=(Call "list_windows" @{})|ConvertFrom-Json
  $wins=$lw.windows; if(-not $wins){$wins=$lw._legacy_windows}
  $tw=$wins|Where-Object {$_.title -match "WinTarget2"}|Select-Object -First 1
  $oc=$wins|Where-Object {$_.title -match "OccluderTop"}|Select-Object -First 1
  $res.target_hwnd=$targetHwnd
  $res.target=@{pid=$tw.pid;window_id=$tw.window_id;z_index=$tw.z_index}
  $res.occluder=@{pid=$oc.pid;window_id=$oc.window_id;z_index=$oc.z_index}
  $res.all_z=$wins|ForEach-Object {@{title=$_.title;z=$_.z_index;pid=$_.pid}}
  $gws=(Call "get_window_state" @{pid=$tw.pid;window_id=$tw.window_id;include_screenshot=$false;include_text=$true})|ConvertFrom-Json
  $els=$gws.elements; $sn=$gws.snapshot_id
  $btn=$els|Where-Object {$_.role -match "button"}|Select-Object -First 1
  $fld=$els|Where-Object {$_.role -match "edit|text"}|Select-Object -First 1
  # mark where fixture log currently ends, so we can see NEW activation events during actions
  $preCount=(Get-Content C:\cua\fx2.log|Measure-Object -Line).Lines
  $res.fg_at_start=(FgInfo)
  $fgb=FgInfo
  $r1=Call "click" @{pid=$tw.pid;window_id=$tw.window_id;element_index=$btn.element_index;snapshot_id=$sn}
  Start-Sleep 1
  $res.click=@{fg_before=$fgb;resp=($r1|ConvertFrom-Json);fg_after=(FgInfo)}
  $fgb=FgInfo
  $r2=Call "set_value" @{pid=$tw.pid;window_id=$tw.window_id;element_index=$fld.element_index;value="occluded hello";snapshot_id=$sn}
  Start-Sleep 1
  $res.set_value=@{fg_before=$fgb;resp=($r2|ConvertFrom-Json);fg_after=(FgInfo)}
  $fgb=FgInfo
  $r3=Call "type_text" @{pid=$tw.pid;window_id=$tw.window_id;text=" typed"}
  Start-Sleep 1
  $res.type_text=@{fg_before=$fgb;resp=($r3|ConvertFrom-Json);fg_after=(FgInfo)}
  $res.fx=Get-Content C:\cua\fx2.log
  # activation events that occurred AFTER the occluder took over (i.e. during actions)
  $res.fx_after_occluder=(Get-Content C:\cua\fx2.log|Select-Object -Skip $preCount)
  $res|ConvertTo-Json -Depth 8|Out-File C:\cua\results2.json -Encoding utf8
} catch { $_|Out-String|Out-File C:\cua\error2.txt }
Stop-Transcript
