$ErrorActionPreference="Continue"
Start-Transcript -Path C:\cua\eval3.log -Force
Add-Type @"
using System;using System.Runtime.InteropServices;
public class W {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
"@
function FgInfo(){ $h=[W]::GetForegroundWindow(); $p=0; [W]::GetWindowThreadProcessId($h,[ref]$p)|Out-Null; @{hwnd=$h.ToInt64();pid=[int]$p} }
try {
  $drv=(Get-ChildItem C:\cua\driver -Recurse -Filter cua-driver.exe|Select-Object -First 1).FullName
  Remove-Item C:\cua\fx3.log -ErrorAction SilentlyContinue
  Start-Process powershell -ArgumentList "-NoProfile","-ExecutionPolicy","Bypass","-File","C:\cua\fixture3.ps1"
  # robust wait for ready (up to 30s)
  $ready=$null
  for($i=0;$i -lt 30;$i++){ Start-Sleep 1; if(Test-Path C:\cua\fx3.log){ $ready=(Get-Content C:\cua\fx3.log|Where-Object {$_ -match '"hit":"ready"'}|Select-Object -First 1); if($ready){break} } }
  $targetHwnd=[int64](($ready|ConvertFrom-Json).v)
  Start-Sleep 1
  # MINIMIZE the target: relinquishes foreground + hides it
  [W]::ShowWindow([IntPtr]$targetHwnd,6)|Out-Null   # SW_MINIMIZE
  Start-Sleep 2
  $res=[ordered]@{}
  $res.target_hwnd=$targetHwnd
  $res.after_minimize=@{ is_iconic=[W]::IsIconic([IntPtr]$targetHwnd); is_visible=[W]::IsWindowVisible([IntPtr]$targetHwnd); foreground=(FgInfo) }
  & $drv stop 2>&1|Out-Null; Start-Process $drv -ArgumentList "serve" -WindowStyle Hidden; Start-Sleep 6
  function Call($tool,$obj){ ($obj|ConvertTo-Json -Compress)|& $drv call $tool 2>$null }
  $lw=(Call "list_windows" @{})|ConvertFrom-Json
  $wins=$lw.windows; if(-not $wins){$wins=$lw._legacy_windows}
  $tw=$wins|Where-Object {$_.title -match "WinTarget3"}|Select-Object -First 1
  $res.target_window=@{pid=$tw.pid;window_id=$tw.window_id;z_index=$tw.z_index;minimized=$tw.minimized;is_on_screen=$tw.is_on_screen}
  $gws=(Call "get_window_state" @{pid=$tw.pid;window_id=$tw.window_id;include_screenshot=$false;include_text=$true})|ConvertFrom-Json
  $els=$gws.elements; $sn=$gws.snapshot_id
  $res.nelem=$els.Count
  $btn=$els|Where-Object {$_.role -match "button"}|Select-Object -First 1
  $fld=$els|Where-Object {$_.role -match "edit|text"}|Select-Object -First 1
  $fgb=FgInfo
  $r1=Call "click" @{pid=$tw.pid;window_id=$tw.window_id;element_index=$btn.element_index;snapshot_id=$sn}
  Start-Sleep 1
  $res.click=@{fg_before=$fgb;target_was_fg=($fgb.hwnd -eq $targetHwnd);resp=($r1|ConvertFrom-Json);fg_after=(FgInfo);iconic_after=[W]::IsIconic([IntPtr]$targetHwnd)}
  $fgb=FgInfo
  $r2=Call "set_value" @{pid=$tw.pid;window_id=$tw.window_id;element_index=$fld.element_index;value="minimized hello";snapshot_id=$sn}
  Start-Sleep 1
  $res.set_value=@{fg_before=$fgb;target_was_fg=($fgb.hwnd -eq $targetHwnd);resp=($r2|ConvertFrom-Json);fg_after=(FgInfo);iconic_after=[W]::IsIconic([IntPtr]$targetHwnd)}
  $fgb=FgInfo
  $r3=Call "type_text" @{pid=$tw.pid;window_id=$tw.window_id;text=" typed"}
  Start-Sleep 1
  $res.type_text=@{fg_before=$fgb;resp=($r3|ConvertFrom-Json);fg_after=(FgInfo);iconic_after=[W]::IsIconic([IntPtr]$targetHwnd)}
  $res.fx=Get-Content C:\cua\fx3.log
  $res|ConvertTo-Json -Depth 8|Out-File C:\cua\results3.json -Encoding utf8
} catch { $_|Out-String|Out-File C:\cua\error3.txt }
Stop-Transcript
