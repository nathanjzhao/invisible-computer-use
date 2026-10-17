param(
    [Parameter(Mandatory=$true)][ValidateSet('machine','user')][string]$Mode,
    [Parameter(Mandatory=$true)][ValidateSet('task-then-update','idle-update','wrongkey','tampered','downgrade','versionmismatch')][string]$Scenario,
    [string]$Dir='C:\ICUAudit\upd',
    [string]$Harness='C:\ICUAudit\harness',
    [string]$Result='C:\Users\Public\upd-result.json'
)
# One updater scenario against an INSTALLED 0.1.0 build whose updater endpoint is the local server
# (node upd-server.js, throwaway key). Runs as the logged-on standard user in the interactive session.
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $Harness 'NativeObserver.cs')
$install = if($Mode -eq 'machine'){ 'C:\Program Files\Copilot' } else { Join-Path $env:LOCALAPPDATA 'Programs\Copilot' }
$exe = Join-Path $install 'app.exe'
$work = Join-Path $env:LOCALAPPDATA ('ICUAudit\upd-'+$Scenario+'-'+$Mode+'-'+(Get-Date -Format 'HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$r=[ordered]@{scenario=$Scenario;mode=$Mode;user=$env:USERNAME;install=$install;steps=@()}
function Step([string]$what,$data=$null){ $r.steps += [ordered]@{t=(Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff');what=$what;data=$data}; Write-Output ("{0} {1} {2}" -f (Get-Date -Format 'HH:mm:ss'),$what,($data|ConvertTo-Json -Compress -Depth 4)) }
function Req { @(Get-Content (Join-Path $Dir 'requests.log') -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ }) }
function Version { try { (Get-Item $exe).VersionInfo.ProductVersion } catch { '<missing>' } }
function Shot([string]$name){ try{ $b=[Windows.Forms.Screen]::PrimaryScreen.Bounds; $i=New-Object Drawing.Bitmap($b.Width,$b.Height); $g=[Drawing.Graphics]::FromImage($i); $g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size); $i.Save((Join-Path $work "$name.png"),[Drawing.Imaging.ImageFormat]::Png); $g.Dispose();$i.Dispose() }catch{} }
function Wait-For($predicate,[int]$seconds,[string]$message){ $d=(Get-Date).AddSeconds($seconds); while((Get-Date) -lt $d){ $v=& $predicate; if($v){return $v}; Start-Sleep -Milliseconds 200 }; throw $message }
$script:appWindow=[IntPtr]::Zero
function Nodes { $root=[Windows.Automation.AutomationElement]::FromHandle($script:appWindow); return $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) }
function Find-Name([string]$text,[bool]$exact=$true){ foreach($n in (Nodes)){ try{ $name=$n.Current.Name; if(-not $n.Current.IsOffscreen -and (($exact -and $name -eq $text) -or (-not $exact -and $name.Contains($text)))){return $n} }catch{} }; return $null }
function Press($node){ for($i=0;$i -lt 8 -and $node;$i++){ $p=$null; if($node.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){ $p.Invoke(); return }; $node=[Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node) }; throw 'No invoke action' }
function Texts { @(Nodes | ForEach-Object { try{$_.Current.Name}catch{} } | Where-Object { $_ -and $_.Length -lt 200 }) }
function App-Procs { @(Get-CimInstance Win32_Process -Filter "Name='app.exe'" | Where-Object { $_.ExecutablePath -eq $exe }) }

$server=$null; $app=$null; $fixture=$null; $listener=$null
try {
    Step 'start' @{version_on_disk=(Version);scenario=$Scenario}
    # which manifest this scenario is served
    $map=@{'task-then-update'='good';'idle-update'='good';'wrongkey'='wrongkey';'tampered'='tampered';'downgrade'='downgrade';'versionmismatch'='versionmismatch'}
    Copy-Item (Join-Path $Dir ("manifest-"+$map[$Scenario]+".json")) (Join-Path $Dir 'manifest.json') -Force
    Remove-Item (Join-Path $Dir 'requests.log') -ErrorAction SilentlyContinue
    if($Scenario -eq 'task-then-update'){ Set-Content (Join-Path $Dir 'hold') 'held until the task is running' }
    $server=Start-Process 'C:\Program Files\nodejs\node.exe' -ArgumentList "`"$Dir\upd-server.js`"","`"$Dir`"",'18080' -PassThru -WindowStyle Hidden
    Start-Sleep -Milliseconds 800
    $manifestNow=Get-Content (Join-Path $Dir 'manifest.json') -Raw | ConvertFrom-Json
    Step 'server' @{pid=$server.Id;manifest_version=$manifestNow.version}
    $startVersion=Version
    if($Scenario -eq 'task-then-update'){
        $csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
        $fx=Join-Path $work 'ICUCalculatorFixture.exe'
        & $csc /nologo /codepage:65001 /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll ("/out:"+$fx) (Join-Path $Harness 'CalculatorFixture.cs')
        if($LASTEXITCODE){ throw 'fixture compile failed' }
        $fixture=Start-Process $fx -ArgumentList $work -PassThru
        Wait-For { Test-Path (Join-Path $work 'ready.json') } 10 'fixture did not start' | Out-Null
        $listener=New-Object Net.HttpListener; $listener.Prefixes.Add('http://localhost:11434/'); $listener.Start()
        $env:ICU_BRAIN='ollama'
    }
    $app=Start-Process $exe -PassThru
    $script:appWindow=Wait-For { $w=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200); if($w -ne [IntPtr]::Zero){$w} } 30 'app window did not appear'
    $oldPid=$app.Id
    $screen=[Windows.Forms.Screen]::PrimaryScreen.Bounds
    [NativeObserver]::SetWindowPos($script:appWindow,[IntPtr]::Zero,0,0,[Math]::Min(1200,$screen.Width-20),[Math]::Min(800,$screen.Height-20),0x14) | Out-Null
    Step 'app started' @{pid=$oldPid;path=$exe;version=$startVersion}

    if($Scenario -eq 'task-then-update'){
        # a task is under way: the manifest request is still held, the model call is held too
        Press (Wait-For { Find-Name '+ window' } 30 'window picker unavailable')
        Press (Wait-For { Find-Name 'ICU Calculator' $false } 10 'fixture missing from picker')
        $edit=Wait-For { foreach($n in (Nodes)){ if($n.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and $n.Current.IsEnabled){ return $n } } } 10 'goal editor unavailable'
        $vp=$null; if(-not $edit.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$vp)){ throw 'goal editor has no ValuePattern' }
        $vp.SetValue('Compute six times seven using the calculator buttons.')
        Press (Wait-For { Find-Name 'Send' } 5 'Send unavailable')
        $t=$listener.GetContextAsync(); if(-not $t.Wait(30000)){ throw 'model request did not arrive' }
        $ctx1=$t.Result; $rd=New-Object IO.StreamReader($ctx1.Request.InputStream); [void]$rd.ReadToEnd(); $rd.Dispose()
        $running = [bool](Find-Name 'Stop')
        Step 'task running (model call held)' @{stop_button_visible=$running;app_pid=$app.Id}
        if(-not $running){ throw 'the task is not running (no Stop button)' }
        # now let the app's one update check through: the offer arrives mid-task
        Remove-Item (Join-Path $Dir 'hold')
        $toast=Wait-For { Find-Name 'is ready' $false } 30 'no update toast during the task'
        Step 'update offered during the task' @{toast=$toast.Current.Name;stop_button_still_visible=[bool](Find-Name 'Stop');app_pid=$app.Id;alive=-not $app.HasExited}
        Shot 'offered-during-task'
        # the install click during the task
        $before=@(App-Procs | ForEach-Object { $_.ProcessId })
        Press (Wait-For { Find-Name 'Install & restart' } 5 'Install & restart button missing')
        Start-Sleep -Seconds 6
        $msg=@(Texts | Where-Object { $_ -match 'task is running|Downloading|Restarting|Update failed' })
        $after=@(App-Procs | ForEach-Object { $_.ProcessId })
        Step 'install clicked during the task' @{message=$msg;pids_before=$before;pids_after=$after;version=(Version);stop_button_still_visible=[bool](Find-Name 'Stop');requests=(Req)}
        Shot 'install-click-during-task'
        if(-not ($msg -match 'task is running')){ throw 'the click was not refused with the task-running message' }
        if($app.HasExited -or (Version) -ne $startVersion){ throw 'the click during the task changed something' }
        if(@(Get-Content (Join-Path $Dir 'requests.log') | Where-Object { $_ -match '/files/' }).Count){ throw 'the archive was downloaded during the task' }
        # the task goes on and finishes
        Start-Sleep -Milliseconds 300
        $payload=@{message=@{role='assistant';content=(@{actions=@(@{kind='click';target='6'},@{kind='click';target=([string][char]0xD7)},@{kind='click';target='7'},@{kind='click';target='='});thought='go'} | ConvertTo-Json -Depth 12 -Compress)}} | ConvertTo-Json -Depth 15 -Compress
        $bytes=[Text.Encoding]::UTF8.GetBytes($payload); $ctx1.Response.ContentType='application/json'; $ctx1.Response.ContentLength64=$bytes.Length; $ctx1.Response.OutputStream.Write($bytes,0,$bytes.Length); $ctx1.Response.Close()
        $t2=$listener.GetContextAsync(); if(-not $t2.Wait(30000)){ throw 'second model request did not arrive' }
        $ctx2=$t2.Result; $rd=New-Object IO.StreamReader($ctx2.Request.InputStream); [void]$rd.ReadToEnd(); $rd.Dispose()
        $payload=@{message=@{role='assistant';content=(@{action=@{kind='done';summary='Observed 42'}} | ConvertTo-Json -Depth 12 -Compress)}} | ConvertTo-Json -Depth 15 -Compress
        $bytes=[Text.Encoding]::UTF8.GetBytes($payload); $ctx2.Response.ContentType='application/json'; $ctx2.Response.ContentLength64=$bytes.Length; $ctx2.Response.OutputStream.Write($bytes,0,$bytes.Length); $ctx2.Response.Close()
        Wait-For { Find-Name 'done. Observed 42' $false } 20 'the task did not finish' | Out-Null
        $state=Get-Content (Join-Path $work 'state.json') -Encoding UTF8 | ConvertFrom-Json
        Step 'task finished' @{receiver_display=$state.display;app_pid=$app.Id;same_pid=($app.Id -eq $oldPid);alive=-not $app.HasExited;version=(Version)}
        if($state.display -ne '42'){ throw 'the task did not compute 42' }
    } else {
        $toast=$null
        try { $toast=Wait-For { Find-Name 'is ready' $false } 25 'no toast' } catch {}
        Step 'toast' @{shown=[bool]$toast;text=$(if($toast){$toast.Current.Name}else{$null});requests=(Req)}
        Shot 'toast'
        if($Scenario -eq 'downgrade'){
            if($toast){ throw 'a downgrade was offered' }
            Step 'downgrade not offered' @{manifest_version=$manifestNow.version;installed=$startVersion}
        }
    }

    if($Scenario -in @('wrongkey','tampered','versionmismatch')){
        if(-not $toast){ throw 'no toast to click' }
        Press (Find-Name 'Install & restart')
        $failed=Wait-For { Find-Name 'Update failed' $false } 60 'no failure shown'
        Start-Sleep -Seconds 2
        Step 'install refused' @{message=$failed.Current.Name;texts=@(Texts | Where-Object { $_ -match 'update|Update' });app_alive=-not $app.HasExited;pid=$app.Id;version=(Version);requests=(Req)}
        Shot 'refused'
        if($app.HasExited -or (Version) -ne $startVersion){ throw 'the refused update changed something' }
    }

    if($Scenario -in @('task-then-update','idle-update')){
        if($Scenario -eq 'idle-update'){ if(-not $toast){ throw 'no update toast' } }
        else { $toast=Find-Name 'Install & restart'; if(-not $toast){ throw 'toast gone after the task' } }
        $oldHash=(Get-FileHash $exe -Algorithm SHA256).Hash
        $pidsBefore=@(App-Procs | ForEach-Object { $_.ProcessId })
        Press (Find-Name 'Install & restart')
        Step 'install clicked (idle)' @{pids_before=$pidsBefore;old_hash=$oldHash}
        # the plugin runs the installer and exits the app at once; the installer relaunches the new one
        $gone=$false; try { Wait-For { $app.Refresh(); $app.HasExited } 60 'old app still running' | Out-Null; $gone=$true } catch {}
        Step 'old app' @{exited=$gone}
        $newVersion=$null
        $deadline=(Get-Date).AddSeconds(240)
        while((Get-Date) -lt $deadline){ $newVersion=Version; if($newVersion -ne $startVersion -and $newVersion -ne '<missing>'){ break }; Start-Sleep -Seconds 2 }
        Step 'version on disk' @{before=$startVersion;after=$newVersion;new_hash=(Get-FileHash $exe -Algorithm SHA256 -ErrorAction SilentlyContinue).Hash}
        $newApp=$null; $d2=(Get-Date).AddSeconds(90)
        while((Get-Date) -lt $d2){ $p=@(App-Procs); if($p.Count){ $newApp=$p[0]; break }; Start-Sleep -Seconds 2 }
        Step 'relaunched app' @{running=[bool]$newApp;pid=$(if($newApp){$newApp.ProcessId});path=$(if($newApp){$newApp.ExecutablePath});is_new_pid=($newApp -and $newApp.ProcessId -ne $oldPid)}
        Shot 'after-update'
        $r.final_version=$newVersion; $r.relaunched=[bool]$newApp
        $sig=Get-AuthenticodeSignature $exe; $r.final_signature=@{status="$($sig.Status)";subject=$sig.SignerCertificate.Subject;thumbprint=$sig.SignerCertificate.Thumbprint}
        # drivers beside the app, and whether the new app launched its own driver
        $r.installed_files=@(Get-ChildItem $install -File | ForEach-Object { $s=Get-AuthenticodeSignature $_.FullName; @{name=$_.Name;status="$($s.Status)";sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash} })
        $expected=Join-Path $Dir 'expected-0.2.0.json'
        if(Test-Path $expected){
            $e=Get-Content $expected -Raw | ConvertFrom-Json
            $r.sidecars_are_0_2_0=@{driver=((Get-FileHash (Join-Path $install 'cua-driver.exe')).Hash -eq $e.driver);worker=((Get-FileHash (Join-Path $install 'cua-driver-uia.exe')).Hash -eq $e.worker)}
        }
        # anything of the old install still running (a sidecar that outlived the app would hold its file open)
        Start-Sleep -Seconds 3
        $r.processes_from_install=@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($install) } | ForEach-Object { @{name=$_.Name;pid=$_.ProcessId} })
        if($newVersion -eq $startVersion){ throw 'the version on disk did not change' }
    }
    $r.ok=$true
} catch { $r.ok=$false; $r.error=$_.Exception.Message; Step 'FAILED' @{error=$_.Exception.Message}; Shot 'failed' }
finally {
    $r.version_at_end=Version
    $r.server_log=(Req)
    if($listener){ try{$listener.Stop();$listener.Close()}catch{} }
    if($fixture -and -not $fixture.HasExited){ $fixture.Kill() }
    # leave nothing running: the app (old or relaunched), its driver and worker, the server
    foreach($p in @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($install) })){ try{ Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }catch{} }
    if($server -and -not $server.HasExited){ $server.Kill() }
    $r.work=$work
    $r | ConvertTo-Json -Depth 10 | Set-Content $Result -Encoding UTF8
    Copy-Item $Result (Join-Path $work 'result.json') -Force
}
if(-not $r.ok){ exit 1 }
