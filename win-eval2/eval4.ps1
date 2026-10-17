$ErrorActionPreference="Continue"
Start-Transcript -Path C:\cua\eval4.log -Force
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
function Dump($name,$obj){ $obj|ConvertTo-Json -Depth 8|Out-File "C:\cua\o_$name.json" -Encoding utf8 }
try {
  Get-ChildItem C:\cua\o_*.json -EA SilentlyContinue|Remove-Item -Force
  $drv=(Get-ChildItem C:\cua\driver -Recurse -Filter cua-driver.exe|Select-Object -First 1).FullName
  Remove-Item C:\cua\fx4.log -ErrorAction SilentlyContinue
  Start-Process powershell -ArgumentList "-NoProfile","-ExecutionPolicy","Bypass","-File","C:\cua\fixture4.ps1"
  $ready=$null
  for($i=0;$i -lt 30;$i++){ Start-Sleep 1; if(Test-Path C:\cua\fx4.log){ $ready=(Get-Content C:\cua\fx4.log|Where-Object {$_ -match '"hit":"ready"'}|Select-Object -First 1); if($ready){break} } }
  $targetHwnd=[int64](($ready|ConvertFrom-Json).v)
  Start-Sleep 1
  # move foreground away: minimize, then show WITHOUT activating -> visible but not foreground
  [W]::ShowWindow([IntPtr]$targetHwnd,6)|Out-Null   # SW_MINIMIZE
  Start-Sleep 1
  [W]::ShowWindow([IntPtr]$targetHwnd,8)|Out-Null   # SW_SHOWNA (show, no activate)
  Start-Sleep 1
  Dump "minimize" @{ target_hwnd=$targetHwnd; is_iconic=[W]::IsIconic([IntPtr]$targetHwnd); is_visible=[W]::IsWindowVisible([IntPtr]$targetHwnd); foreground=(FgInfo); target_is_foreground=([W]::GetForegroundWindow().ToInt64() -eq $targetHwnd) }
  & $drv stop 2>&1|Out-Null; Start-Process $drv -ArgumentList "serve" -WindowStyle Hidden; Start-Sleep 6
  function Call($tool,$obj){ ($obj|ConvertTo-Json -Compress)|& $drv call $tool 2>$null }
  $lw=(Call "list_windows" @{})|ConvertFrom-Json; $wins=$lw.windows; if(-not $wins){$wins=$lw._legacy_windows}
  $tw=$wins|Where-Object {$_.title -match "WinTarget4"}|Select-Object -First 1
  $gws=(Call "get_window_state" @{pid=$tw.pid;window_id=$tw.window_id;include_screenshot=$false;include_text=$true})|ConvertFrom-Json
  $els=$gws.elements; $sn=$gws.snapshot_id
  $btn=$els|Where-Object {$_.role -match "button"}|Select-Object -First 1
  $fld=$els|Where-Object {$_.role -match "edit|text"}|Select-Object -First 1
  Dump "window" @{ target=@{pid=$tw.pid;window_id=$tw.window_id;z_index=$tw.z_index;minimized=$tw.minimized;is_on_screen=$tw.is_on_screen}; nelem=$els.Count; btn_idx=$btn.element_index; fld_idx=$fld.element_index }
  # CLICK
  $fgb=FgInfo; $r=Call "click" @{pid=$tw.pid;window_id=$tw.window_id;element_index=$btn.element_index;snapshot_id=$sn}; Start-Sleep 1
  Dump "click" @{ fg_before=$fgb; target_was_fg=($fgb.hwnd -eq $targetHwnd); resp=($r|ConvertFrom-Json); fg_after=(FgInfo); iconic_after=[W]::IsIconic([IntPtr]$targetHwnd); visible_after=[W]::IsWindowVisible([IntPtr]$targetHwnd) }
  # SET_VALUE
  $fgb=FgInfo; $r=Call "set_value" @{pid=$tw.pid;window_id=$tw.window_id;element_index=$fld.element_index;value="unfocused hello";snapshot_id=$sn}; Start-Sleep 1
  Dump "set_value" @{ fg_before=$fgb; target_was_fg=($fgb.hwnd -eq $targetHwnd); resp=($r|ConvertFrom-Json); fg_after=(FgInfo); iconic_after=[W]::IsIconic([IntPtr]$targetHwnd) }
  # TYPE_TEXT with 25s timeout job so a foreground-escalation hang can't stall the eval
  $fgb=FgInfo
  $j=Start-Job -ScriptBlock { param($d,$p,$w) (@{pid=$p;window_id=$w;text=" typed"}|ConvertTo-Json -Compress)|& $d call type_text 2>$null } -ArgumentList $drv,$tw.pid,$tw.window_id
  $done=Wait-Job $j -Timeout 25
  $tout = if($done){ (Receive-Job $j) } else { "TIMEOUT_BLOCKED" }
  Stop-Job $j -EA SilentlyContinue; Remove-Job $j -Force -EA SilentlyContinue
  Dump "type_text" @{ fg_before=$fgb; resp="$tout"; fg_after=(FgInfo); iconic_after=[W]::IsIconic([IntPtr]$targetHwnd) }
  Dump "fx" @{ log=(Get-Content C:\cua\fx4.log) }
  "DONE" | Out-File C:\cua\eval4.done
} catch { $_|Out-String|Out-File C:\cua\error4.txt }
Stop-Transcript
