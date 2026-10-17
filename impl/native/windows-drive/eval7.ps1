# Windows drive for the cross-platform lineage patch (runs at interactive logon via a scheduled task).
# Downloads the xwin-built cua-driver.exe (EXE_URL is substituted by the launcher), starts the fixture,
# and verifies: full_first -> no_change -> `~` after the fixture rewrites its own entry (its log proves it) -> disableDiff -> cumulative,
# then the host gate: LockWorkStation -> health_report host_interactive fails (screen_locked) and click is refused with
# host_not_interactive while get_window_state still answers. The session stays locked afterwards (the instance is transient).
# A completed run is not repeated at the next logon (eval7.done); --rerun removes the marker first.
if(Test-Path C:\cua\eval7.done){ exit }
$ErrorActionPreference="Continue"
New-Item -ItemType Directory -Force C:\cua | Out-Null
Remove-Item C:\cua\progress.log -EA SilentlyContinue
Start-Transcript -Path C:\cua\eval7.log -Force
function Trunc($x,$n){ $t="$x" -replace "\s+"," "; if($t.Length -gt $n){ $t.Substring(0,$n) } else { $t } }
try {
  if(-not (Test-Path C:\cua\cua-driver.exe)){ Invoke-WebRequest -Uri "__EXE_URL__" -OutFile C:\cua\cua-driver.exe -UseBasicParsing }
  $drv="C:\cua\cua-driver.exe"
  Remove-Item C:\cua\fx2.log -ErrorAction SilentlyContinue
  Start-Process powershell -ArgumentList "-NoProfile","-ExecutionPolicy","Bypass","-File","C:\cua\fixture.ps1"
  Start-Sleep 6
  Add-Content C:\cua\progress.log "STEP serve"
  # Reuse a running daemon: restarting it (and the CLI auto-launching daemons while the new one
  # comes up) splits the per-process lineage store across daemons and yields copies of results.
  function Running(){ ((& $drv status 2>&1 | Out-String) -match "is running") }
  if(-not (Running)){
    Start-Process $drv -ArgumentList "serve","--no-permissions-gate" -WindowStyle Hidden -RedirectStandardOutput C:\cua\serve.out -RedirectStandardError C:\cua\serve.err
    for($k=0;$k -lt 15 -and -not (Running);$k++){ Start-Sleep 2 }
  }
  if(-not (Running)){
    # a freshly downloaded exe has been seen to not come up through Start-Process on first launch; cmd works
    Add-Content C:\cua\progress.log "STEP serve retry via cmd"
    Start-Process cmd -ArgumentList "/c C:\cua\cua-driver.exe serve --no-permissions-gate > C:\cua\serve.out 2>&1" -WindowStyle Hidden
    for($k=0;$k -lt 15 -and -not (Running);$k++){ Start-Sleep 2 }
  }
  Add-Content C:\cua\progress.log ("STATUS " + ((& $drv status 2>&1 | Out-String) -replace "\s+"," "))
  Add-Content C:\cua\progress.log ("SERVE " + ((Get-Content C:\cua\serve.err -Raw -EA SilentlyContinue)+"").Substring(0,[Math]::Min(300,((Get-Content C:\cua\serve.err -Raw -EA SilentlyContinue)+"").Length)))
  # Every call runs with a 40 s timeout through cmd (stdin from a file) so a hang is reported, not waited on.
  function Call($tool,$obj){
    Add-Content C:\cua\progress.log "STEP call $tool"
    # Back-to-back `call` CLI processes collide on the daemon pipe (a stuck one starves the next);
    # kill any straggler that is not the daemon, and space calls by a few seconds.
    $st=(& $drv status 2>&1 | Out-String); $daemonPid=0; if($st -match "pid: (\d+)"){ $daemonPid=[int]$Matches[1] }
    Get-Process cua-driver -EA SilentlyContinue | Where-Object { $_.Id -ne $daemonPid } | Stop-Process -Force -EA SilentlyContinue
    Start-Sleep 5
    ($obj|ConvertTo-Json -Compress -Depth 6) | Out-File C:\cua\in.json -Encoding ascii
    Remove-Item C:\cua\out.json -EA SilentlyContinue
    $p=Start-Process cmd -ArgumentList "/c type C:\cua\in.json | C:\cua\cua-driver.exe call $tool > C:\cua\out.json 2> C:\cua\err.txt" -PassThru -WindowStyle Hidden
    Wait-Process -Id $p.Id -Timeout 40 -EA SilentlyContinue
    if(-not $p.HasExited){ Stop-Process -Id $p.Id -Force; Add-Content C:\cua\progress.log "TIMEOUT $tool"; return "{}" }
    $o=(Get-Content C:\cua\out.json -Raw -EA SilentlyContinue)+""
    $in=(Get-Content C:\cua\in.json -Raw -EA SilentlyContinue)+""
    $dk=""; try { $j=$o|ConvertFrom-Json; if($j.diff){ $dk="diff=$($j.diff.kind)/$($j.diff.baseline)/$($j.diff.accessibility_revision)/$($j.diff.accessibility_snapshot_count)" } elseif($j.effect){ $dk="effect=$($j.effect) route=$($j.route)" } } catch { $dk="nonjson" }
    Add-Content C:\cua\progress.log ("RESULT $tool IN " + $in.Trim() + " -> " + $dk + " | " + ($o -replace "\s+"," ").Substring(0,[Math]::Min(120,$o.Length)) + " ERR " + ((Get-Content C:\cua\err.txt -Raw -EA SilentlyContinue)+"").Substring(0,[Math]::Min(160,((Get-Content C:\cua\err.txt -Raw -EA SilentlyContinue)+"").Length)))
    return $o
  }
  $res=[ordered]@{}
  Add-Content C:\cua\progress.log "STEP describe"
  $p=Start-Process cmd -ArgumentList "/c C:\cua\cua-driver.exe describe get_window_state > C:\cua\describe.txt 2>&1" -PassThru -WindowStyle Hidden
  Wait-Process -Id $p.Id -Timeout 40 -EA SilentlyContinue
  if(-not $p.HasExited){ Stop-Process -Id $p.Id -Force; Add-Content C:\cua\progress.log "TIMEOUT describe" }
  $desc=(Get-Content C:\cua\describe.txt -Raw -EA SilentlyContinue)+""
  $res.describe_has = (@("disable_diff","disableDiff","cumulative_diff") | Where-Object { $desc -match $_ }) -join ","
  Add-Content C:\cua\progress.log ("DESCRIBE has " + ($res.describe_has -join ","))
  $lwRaw=Call "list_windows" @{}
  $lw=$lwRaw|ConvertFrom-Json
  $wins=@($lw.windows); if($wins.Count -eq 0){$wins=@($lw._legacy_windows)}
  Add-Content C:\cua\progress.log ("WINDOWS " + (($wins|ForEach-Object{ "$($_.pid)|$($_.window_id)|$($_.title)" }) -join " ; "))
  $tw=$wins|Where-Object {"$($_.title)" -like "*WinTarget2*"}|Select-Object -First 1
  if(-not $tw){ $fp=(Get-Process powershell|Where-Object{$_.MainWindowTitle -eq "WinTarget2"}|Select-Object -First 1).Id; $tw=$wins|Where-Object{$_.pid -eq $fp}|Select-Object -First 1 }
  Add-Content C:\cua\progress.log ("TARGET pid=$($tw.pid) wid=$($tw.window_id) title=$($tw.title)")
  if(-not $tw){ throw "fixture window WinTarget2 not found in list_windows" }
  $res.target="pid=$($tw.pid) wid=$($tw.window_id)"
  function Gws($extra){ $a=@{pid=[int]$tw.pid;window_id=[int64]$tw.window_id;include_screenshot=$false;include_text=$true}; if($extra){$extra.GetEnumerator()|ForEach-Object{$a[$_.Key]=$_.Value}}; (Call "get_window_state" $a)|ConvertFrom-Json }
  # Slim, string-only summaries: PS 5.1 ConvertTo-Json over deep object graphs takes minutes.
  function Summ($s){ $d=$s.diff; if(-not $d){ return @{kind="MISSING"} }; $t="$($d.text)"; @{kind="$($d.kind)";baseline="$($d.baseline)";rev=[int]$d.accessibility_revision;count=[int]$d.accessibility_snapshot_count;text=$t.Substring(0,[Math]::Min(200,$t.Length));elements=@($s.elements).Count;stable=((@($s.elements)|Select-Object -First 8|ForEach-Object{ "$($_.element_index)>$($_.stable_id)" }) -join ",")} }
  # The daemon answers with a window listing until the fixture window is targetable: wait for a real tree first.
  $warm=$null
  for($k=0;$k -lt 15;$k++){ $warm=Gws $null; if($warm.diff){ break }; Start-Sleep 2 }
  Add-Content C:\cua\progress.log ("WARM elements=" + @($warm.elements).Count + " diff=" + "$($warm.diff.kind)")
  $res.warmup=Summ $warm
  $s1=Gws $null; $res.turn1=Summ $s1
  $s2=Gws $null; $res.turn2=Summ $s2
  $fld=@($s2.elements|Where-Object {$_.role -match "edit|text"})[0]
  # the fixture changes its own entry text on a timer (no driver input involved); wait for its log to say so
  for($k=0;$k -lt 30;$k++){ if((Get-Content C:\cua\fx2.log -Raw -EA SilentlyContinue) -match "textChanged"){ break }; Start-Sleep 2 }
  $res.fx=@(Get-Content C:\cua\fx2.log) -join " | "
  $s3=Gws $null; $res.turn3_after_fixture_change=Summ $s3
  $res.entry_stable_kept = (@($s3.elements|Where-Object{$_.element_index -eq $fld.element_index})[0].stable_id -eq $fld.stable_id)
  $s4=Gws @{disableDiff=$true}; $res.turn4_disableDiff=Summ $s4
  $s5=Gws @{cumulative_diff=$true}; $res.turn5_cumulative=Summ $s5
  $res|ConvertTo-Json -Depth 3|Out-File C:\cua\results7.json -Encoding utf8
  # --- host gate
  function HostChk($tag){ $h=(Call "health_report" @{include=@("host_interactive")})|ConvertFrom-Json; $c=@($h.checks|Where-Object{$_.name -eq "host_interactive"})[0]; Add-Content C:\cua\progress.log ("HOST $tag " + ($c|ConvertTo-Json -Compress -Depth 4)); if($c){ @{status="$($c.status)";screen_locked="$($c.data.host.screen_locked)";on_console="$($c.data.host.on_console)";display_asleep="$($c.data.host.display_asleep)";lid_closed="$($c.data.host.lid_closed)";display_count="$($c.data.display_count)";hint="$($c.hint)";message=Trunc $c.message 160} } else { @{status="MISSING"} } }
  $res.host_before=HostChk "before"
  # display sleep: SC_MONITORPOWER off (2) then on (-1); the daemon's power listener must see GUID_CONSOLE_DISPLAY_STATE
  Add-Type -Namespace W -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern int SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l); [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);'
  [W.U]::SendMessage([IntPtr]0xffff, 0x112, [IntPtr]0xF170, [IntPtr]2) | Out-Null
  Start-Sleep 4
  $res.host_display_off=HostChk "display off"
  # a headless console does not light up on SC_MONITORPOWER -1 alone; real input activity does (MOUSEEVENTF_MOVE)
  [W.U]::SendMessage([IntPtr]0xffff, 0x112, [IntPtr]0xF170, [IntPtr](-1)) | Out-Null
  [W.U]::mouse_event(1, 1, 0, 0, [UIntPtr]::Zero); Start-Sleep 1; [W.U]::mouse_event(1, [uint32]4294967295, 0, 0, [UIntPtr]::Zero)
  Start-Sleep 4
  $res.host_display_on=HostChk "display on"
  $res.display_gate = if(($res.host_display_off.display_asleep -eq "True") -and ($res.host_display_off.status -eq "pass") -and ($res.host_display_on.display_asleep -eq "False")){ "PASS" } else { "FAIL" }
  Add-Content C:\cua\progress.log "STEP lock"
  rundll32.exe user32.dll,LockWorkStation
  Start-Sleep 8
  $res.host_locked=HostChk "locked"
  $ck=Call "click" @{pid=[int]$tw.pid;window_id=[int64]$tw.window_id;x=30;y=30}
  $res.click_locked=Trunc $ck 400
  $res.observe_locked=Summ (Gws $null)
  $res.host_gate = if(($res.host_before.status -eq "pass") -and ($res.host_before.screen_locked -eq "False") -and ($res.host_locked.status -eq "fail") -and ($res.host_locked.screen_locked -eq "True") -and ($ck -match '"code": *"host_not_interactive"') -and ($ck -match '"screen_locked"') -and ($res.observe_locked.kind -ne "MISSING")){ "PASS" } else { "FAIL" }
  $res|ConvertTo-Json -Depth 3|Out-File C:\cua\results7.json -Encoding utf8
} catch { $_|Out-String|Out-File C:\cua\error7.txt }
"done" | Out-File C:\cua\eval7.done
Stop-Transcript
