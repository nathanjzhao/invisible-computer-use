# Slice 12 (Windows): drive the icu PRODUCT on a real app (Notepad) through one `icu.exe mcp`
# process, spoken to as a stdio MCP client. One process = one cua-driver child = one snapshot
# store, so per-snapshot element state persists across calls. Runs at interactive logon.
# The bundle zip (icu.exe + cua-driver.exe) is fetched from __EXE_URL__ (launcher-substituted).
# A completed run is not repeated (icu-eval.done); --rerun clears it.
if(Test-Path C:\cua\icu-eval.done){ exit }
$ErrorActionPreference="Continue"
New-Item -ItemType Directory -Force C:\cua | Out-Null
Start-Transcript -Path C:\cua\icu-eval.log -Force
function Log($m){ Add-Content C:\cua\icu-progress.log "$((Get-Date).ToString('o')) $m" }
try {
  if(-not (Test-Path C:\cua\icu.exe)){
    Log "download bundle"
    Invoke-WebRequest -Uri "__EXE_URL__" -OutFile C:\cua\bundle.zip -UseBasicParsing
    Expand-Archive -Path C:\cua\bundle.zip -DestinationPath C:\cua -Force
  }
  $env:ICU_DRIVER="C:\cua\cua-driver.exe"
  $env:ICU_HOME="C:\cua\icuhome"
  $env:ICU_REQUIRE_APP_APPROVAL="0"
  $env:ICU_MEMORY_ROOT="C:\cua\icumem"
  # a WinForms app with a full UIA tree: a button and a textbox whose Names become UIA
  # AutomationIds (pressbtn / entry), so this drive exercises the AutomationId -> identity fix.
  # It reliably presents and foregrounds a titled window in the auto-logon session, which the
  # store Notepad did not (its MainWindowTitle stayed empty and list_windows saw no window).
  Log "launch WinForms fixture"
  $fx = @'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$f=New-Object Windows.Forms.Form; $f.Text="WinTarget2"; $f.StartPosition="Manual"
$f.ClientSize=New-Object Drawing.Size(340,220); $f.Location=New-Object Drawing.Point(160,160); $f.TopMost=$true
$b=New-Object Windows.Forms.Button; $b.Text="Press me"; $b.Name="pressbtn"; $b.Location=New-Object Drawing.Point(20,20); $b.Size=New-Object Drawing.Size(260,32)
$t=New-Object Windows.Forms.TextBox; $t.Name="entry"; $t.Location=New-Object Drawing.Point(20,70); $t.Size=New-Object Drawing.Size(300,26)
$f.Controls.AddRange(@($b,$t))
$f.Add_Shown({ $f.Activate(); ("ready " + $f.Handle.ToInt64()) | Out-File C:\cua\fx2.log -Encoding utf8 })
[Windows.Forms.Application]::Run($f)
'@
  Set-Content C:\cua\winfx.ps1 $fx -Encoding utf8
  Start-Process powershell -ArgumentList "-NoProfile","-ExecutionPolicy","Bypass","-File","C:\cua\winfx.ps1"
  for($i=0;$i -lt 30 -and -not (Test-Path C:\cua\fx2.log);$i++){ Start-Sleep 1 }
  Start-Sleep 2

  # start one icu.exe mcp process; speak JSON-RPC over its stdio
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = "C:\cua\icu.exe"; $psi.Arguments = "mcp"
  $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
  $psi.UseShellExecute = $false
  foreach($k in "ICU_DRIVER","ICU_HOME","ICU_REQUIRE_APP_APPROVAL","ICU_MEMORY_ROOT"){ $psi.EnvironmentVariables[$k] = (Get-Item Env:$k).Value }
  $p = [System.Diagnostics.Process]::Start($psi)
  $global:rid = 0
  function Rpc($method, $params){
    $global:rid++; $id = $global:rid
    $msg = @{ jsonrpc="2.0"; id=$id; method=$method; params=$params } | ConvertTo-Json -Depth 12 -Compress
    $p.StandardInput.WriteLine($msg); $p.StandardInput.Flush()
    while($true){
      $line = $p.StandardOutput.ReadLine()
      if($null -eq $line){ return $null }
      if($line.Trim() -eq ""){ continue }
      try { $o = $line | ConvertFrom-Json } catch { continue }
      if($o.method){ continue }              # a server-initiated request/notification: skip
      if($o.id -eq $id){ return $o }
    }
  }
  function Notify($method, $params){ $m = @{ jsonrpc="2.0"; method=$method; params=$params } | ConvertTo-Json -Depth 8 -Compress; $p.StandardInput.WriteLine($m); $p.StandardInput.Flush() }
  function Call($name, $targs){ (Rpc "tools/call" @{ name=$name; arguments=$targs }).result }
  function Structured($r){ if($r.structuredContent){ $r.structuredContent } else { try { $r.content[0].text | ConvertFrom-Json } catch { $r.content[0].text } } }

  $res = [ordered]@{}
  $init = Rpc "initialize" @{ protocolVersion="2025-06-18"; capabilities=@{}; clientInfo=@{ name="win-eval"; version="0" } }
  $res.server = $init.result.serverInfo.name
  Log ("INIT " + $res.server)
  Notify "notifications/initialized" @{}
  $tools = (Rpc "tools/list" @{}).result.tools
  $res.tool_count = $tools.Count
  $res.window2_first = (@($tools[0..12] | ForEach-Object { $_.name }) -join ",")
  Log ("TOOLS " + $res.tool_count)

  $lw = Structured (Call "list_windows" @{})
  $np = @($lw | Where-Object { "$($_.title)" -match "WinTarget2" })[0]
  Log ("WINDOWS " + (@($lw | ForEach-Object { "$($_.app)|$($_.id)|$($_.title)" }) -join " ; ").Substring(0,[Math]::Min(300, (@($lw | ForEach-Object { "$($_.app)|$($_.id)|$($_.title)" }) -join " ; ").Length)))
  if(-not $np){ throw "WinTarget2 fixture window not found via icu list_windows" }
  $res.target = "$($np.app)|$($np.id)"
  $win = @{ app="$($np.app)"; id=$np.id }
  function Gws(){ Structured (Call "get_window_state" @{ window=$win; include_text=$true; include_screenshot=$false }) }
  $s1 = Gws
  $res.turn1_rev = $s1.accessibilityRevision
  $res.turn1_has_tree = [bool]$s1.accessibility.tree
  Log ("TURN1 rev=" + $s1.accessibilityRevision)
  $s2 = Gws
  $res.turn2_count = $s2.accessibilitySnapshotCount
  $res.turn2_nochange = [bool]("$($s2.accessibility.tree)" -match "no change in the accessibility tree")
  # type into Notepad through the product (gate disabled)
  $tw = Structured (Call "type_text" @{ window=$win; text="icu drove windows" })
  $res.type_effect = "$($tw.effect)$($tw.code)"
  Log ("TYPE " + ($tw | ConvertTo-Json -Compress -Depth 4))
  # Windows background text delivery needs the UIAccess worker (not in this build); fall back to
  # set_value on the edit element (UIA ValuePattern.SetValue), a different path.
  $t1 = "$($s1.accessibility.tree)"
  $res.tree1 = ($t1 -replace "`r?`n"," | ").Substring(0,[Math]::Min(800,($t1 -replace "`r?`n"," | ").Length))
  # The fixture's controls carry UIA AutomationIds (pressbtn / entry). Those feed
  # RevisionNode.identity for the cross-turn diff, not the rendered tree text, so the
  # evidence they work is that the elements keep stable ids across turns (turn2 = no change).
  Log ("TREE " + ($t1 -replace "`r?`n"," | ").Substring(0,[Math]::Min(600,$t1.Length)))
  # try set_value (UIA ValuePattern, no worker needed) on each element until one accepts it —
  # Notepad's edit control implements ValuePattern; the window/root does not.
  $ids = @(); foreach($l in ($t1 -split "`n")){ if($l -match '^\s*\[(\d+)\]'){ $ids += [int]$Matches[1] } }
  foreach($e in $ids){
    $sv = Structured (Call "set_value" @{ window=$win; element_index=$e; value="icu drove windows" })
    if("$($sv.message)" -notmatch "does not implement ValuePattern"){
      $res.set_value_effect = "$($sv.effect)$($sv.code)"; $res.set_value_element = $e
      Log ("SETVALUE elem=$e " + ($sv | ConvertTo-Json -Compress -Depth 4)); break
    }
  }
  if(-not $res.set_value_element){ $res.set_value_effect = "no ValuePattern element"; Log "SETVALUE no ValuePattern element among $($ids.Count) ids" }
  Start-Sleep 1
  $s3 = Gws
  $res.turn3_rev = $s3.accessibilityRevision
  $res.text_present = [bool]("$($s3.accessibility.tree)$($s3.accessibility.document_text)" -match "icu drove windows")
  $res.host = Structured (Call "host_state" @{})
  $res.icu_win_drive = if($res.window2_first -and $res.turn1_has_tree -and $res.turn2_nochange -and $res.text_present){ "PASS" } elseif($res.window2_first -and $res.turn1_has_tree -and $res.turn2_nochange){ "OBSERVE_OK_INPUT_NEEDS_WORKER" } else { "PARTIAL" }
  $res | ConvertTo-Json -Depth 6 | Out-File C:\cua\icu-results.json -Encoding utf8
  Log ("DONE " + $res.icu_win_drive)
  $p.StandardInput.Close(); Start-Sleep 1; if(-not $p.HasExited){ $p.Kill() }
} catch { $_ | Out-String | Out-File C:\cua\icu-error.txt }
"done" | Out-File C:\cua\icu-eval.done
Stop-Transcript
