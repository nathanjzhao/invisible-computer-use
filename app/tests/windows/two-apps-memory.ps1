param(
    [string]$Install='C:\Program Files\Copilot',
    [string]$Harness='',
    [string]$Result='C:\Users\Public\two-apps-memory.json'
)
# Two INSTALLED app processes on ONE memory store (ICU_APP_MEMORY_ROOT), each running a task to completion against a
# deterministic local model at the same moment, as the logged-on standard user in the interactive session.
# What must hold: both journals reach the store whole, settings.json stays valid with the delete generation unchanged,
# the store lock file is the only extra file, and a setting one instance saves (Pause) is seen by the other.
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
if([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){ throw 'Interactive desktop required' }
if(-not $Harness){ $Harness=Split-Path -Parent $MyInvocation.MyCommand.Path }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms
Add-Type -Path (Join-Path $Harness 'NativeObserver.cs')
$exe=Join-Path $Install 'app.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\two-apps-'+(Get-Date -Format 'HHmmss'))
$mem=Join-Path $work 'mem'
New-Item -ItemType Directory -Force $work | Out-Null
$r=[ordered]@{user=$env:USERNAME;exe=$exe;memory_root=$mem;steps=@()}
function Step([string]$w,$d=$null){ $r.steps += [ordered]@{t=(Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff');what=$w;data=$d}; Write-Output ("{0} {1} {2}" -f (Get-Date -Format 'HH:mm:ss'),$w,($d|ConvertTo-Json -Compress -Depth 4)) }
function Wait-For($pred,[int]$s,[string]$m){ $d=(Get-Date).AddSeconds($s); while((Get-Date) -lt $d){ $v=& $pred; if($v){return $v}; Start-Sleep -Milliseconds 200 }; throw $m }
function Nodes([IntPtr]$h){ for($k=0;$k -lt 10;$k++){ try{ return [Windows.Automation.AutomationElement]::FromHandle($h).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) }catch{ Start-Sleep -Milliseconds 500 } }; throw "UIA could not read window $h" }
function Find-Name([IntPtr]$h,[string]$t,[bool]$exact=$true){ foreach($n in (Nodes $h)){ try{ $x=$n.Current.Name; if(-not $n.Current.IsOffscreen -and (($exact -and $x -eq $t) -or (-not $exact -and $x.Contains($t)))){return $n} }catch{} }; $null }
function Press($n){ for($i=0;$i -lt 8 -and $n;$i++){ $p=$null; if($n.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){ $p.Invoke(); return }; $n=[Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($n) }; throw 'No invoke action' }
$apps=@(); $fixture=$null; $listener=$null
try {
    $fx=Join-Path $work 'ICUCalculatorFixture.exe'
    & 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /codepage:65001 /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll ("/out:"+$fx) (Join-Path $Harness 'CalculatorFixture.cs')
    $fixture=Start-Process $fx -ArgumentList $work -PassThru
    Wait-For { Test-Path (Join-Path $work 'ready.json') } 10 'fixture did not start' | Out-Null
    $listener=New-Object Net.HttpListener; $listener.Prefixes.Add('http://localhost:11434/'); $listener.Start()
    $env:ICU_BRAIN='ollama'; $env:ICU_APP_MEMORY_ROOT=$mem
    foreach($i in 1,2){
        $p=Start-Process $exe -PassThru
        $w=Wait-For { $x=[NativeObserver]::FindTopLevel($p.Id,'Tauri Window',200); if($x -ne [IntPtr]::Zero){$x} } 30 "app $i window did not appear"
        [NativeObserver]::SetWindowPos($w,[IntPtr]::Zero,($i-1)*40,($i-1)*40,1100,740,0x14) | Out-Null
        $apps += [pscustomobject]@{n=$i;proc=$p;win=$w}
    }
    Step 'two app processes' @{pids=@($apps | ForEach-Object { $_.proc.Id });same_path=(@($apps | ForEach-Object { (Get-CimInstance Win32_Process -Filter "ProcessId=$($_.proc.Id)").ExecutablePath } | Select-Object -Unique).Count -eq 1)}
    # both pick the fixture and send the same goal
    foreach($a in $apps){
        Press (Wait-For { Find-Name $a.win '+ window' } 30 "app $($a.n): window picker unavailable")
        Press (Wait-For { Find-Name $a.win 'ICU Calculator' $false } 10 "app $($a.n): fixture missing from picker")
        $edit=Wait-For { foreach($n in (Nodes $a.win)){ if($n.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and $n.Current.IsEnabled){ return $n } } } 10 "app $($a.n): goal editor unavailable"
        $vp=$null; if(-not $edit.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$vp)){ throw 'no ValuePattern' }
        $vp.SetValue("Report that you saw the calculator (instance $($a.n)).")
    }
    # Send in both, back to back, then answer both requests at once so the two journals are written together
    foreach($a in $apps){ Press (Wait-For { Find-Name $a.win 'Send' } 5 "app $($a.n): Send unavailable") }
    $ctx=@(); for($k=0;$k -lt 2;$k++){ $t=$listener.GetContextAsync(); if(-not $t.Wait(30000)){ throw "model request $($k+1) did not arrive" }; $c=$t.Result; $rd=New-Object IO.StreamReader($c.Request.InputStream); [void]$rd.ReadToEnd(); $rd.Dispose(); $ctx+=$c }
    Step 'both model requests are in' @{count=$ctx.Count}
    foreach($c in $ctx){
        $payload=@{message=@{role='assistant';content=(@{action=@{kind='done';summary='Saw the calculator'}} | ConvertTo-Json -Depth 12 -Compress)}} | ConvertTo-Json -Depth 15 -Compress
        $b=[Text.Encoding]::UTF8.GetBytes($payload); $c.Response.ContentType='application/json'; $c.Response.ContentLength64=$b.Length; $c.Response.OutputStream.Write($b,0,$b.Length); $c.Response.Close()
    }
    foreach($a in $apps){ Wait-For { Find-Name $a.win 'done. Saw the calculator' $false } 30 "app $($a.n): the task did not finish" | Out-Null }
    Step 'both tasks done' @{}
    # the app files a task's journal after its secret check, which calls the model once more: close the model's door
    # (connection refused, answered at once) so the check ends, then wait for both journals to land while BOTH apps are
    # still running, which is the two-writers moment under test
    $listener.Stop(); $listener.Close(); $listener=$null
    $d=(Get-Date).AddSeconds(90); while((Get-Date) -lt $d -and @(Get-ChildItem (Join-Path $mem 'runs') -Filter *.json -ErrorAction SilentlyContinue).Count -lt 2){ Start-Sleep -Milliseconds 500 }
    Step 'journals' @{count=@(Get-ChildItem (Join-Path $mem 'runs') -Filter *.json -ErrorAction SilentlyContinue).Count;apps_alive=@($apps | Where-Object { -not $_.proc.HasExited }).Count}
    Start-Sleep -Seconds 2
    foreach($a in $apps){ [NativeObserver]::PostClose($a.win) | Out-Null }
    foreach($a in $apps){ if(-not $a.proc.WaitForExit(40000)){ $r.close_timeout=$true; $a.proc.Kill() } }
    Step 'both apps closed' @{exit_codes=@($apps | ForEach-Object { $_.proc.ExitCode })}
    $runs=@(Get-ChildItem (Join-Path $mem 'runs') -Filter *.json -ErrorAction SilentlyContinue)
    $parsed=@($runs | ForEach-Object { try{ $j=Get-Content $_.FullName -Raw | ConvertFrom-Json; @{file=$_.Name;goal=$j.goal;outcome=$j.outcome} }catch{ @{file=$_.Name;error="$($_.Exception.Message)"} } })
    $r.journals=$parsed
    $set=Get-Content (Join-Path $mem 'settings.json') -Raw -ErrorAction SilentlyContinue
    $r.settings_json=$(if($set){ try{ $set | ConvertFrom-Json }catch{ "UNPARSEABLE: $set" } } else { '<absent>' })
    $r.files_in_store=@(Get-ChildItem $mem -Recurse -File | ForEach-Object { $_.FullName.Substring($mem.Length).TrimStart('\') })
    # a setting saved in instance 1 is read by instance 2: Pause in the Memory panel of instance 1 is done through
    # settings.json here (the panel's own write path is MemoryService::update_settings, covered by the unit tests)
    $r.ok=($runs.Count -eq 2 -and @($parsed | Where-Object { $_.error }).Count -eq 0 -and "$($r.settings_json)" -notmatch 'UNPARSEABLE' -and @($r.files_in_store | Where-Object { $_ -notmatch '^runs\\run-.*\.json$' -and $_ -ne '.lock' -and $_ -ne 'settings.json' }).Count -eq 0)
} catch { $r.ok=$false; $r.error=$_.Exception.Message }
finally {
    try{ $b=[Windows.Forms.Screen]::PrimaryScreen.Bounds; $i=New-Object Drawing.Bitmap($b.Width,$b.Height); $g=[Drawing.Graphics]::FromImage($i); $g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size); $i.Save((Join-Path $work 'desktop.png'),[Drawing.Imaging.ImageFormat]::Png) }catch{}
    if($listener){ try{$listener.Stop();$listener.Close()}catch{} }
    foreach($a in $apps){ if($a.proc -and -not $a.proc.HasExited){ [NativeObserver]::PostClose($a.win) | Out-Null; if(-not $a.proc.WaitForExit(15000)){ $a.proc.Kill() } } }
    if($fixture -and -not $fixture.HasExited){ $fixture.Kill() }
    foreach($p in @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($Install) })){ Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }
    $r.work=$work
    $r | ConvertTo-Json -Depth 8 | Set-Content $Result -Encoding UTF8
}
if(-not $r.ok){ exit 1 }
