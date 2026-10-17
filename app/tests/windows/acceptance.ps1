param([Parameter(Mandatory=$true)][string]$ManifestPath,[switch]$NegativeControl,[switch]$StopPending,
    [ValidateSet('compute','toolerror','winkey','chord','scroll')][string]$Mode='compute',[string]$FixtureOrigin='',[switch]$MutateFirstAction,[string]$DriverOverride='',[string]$FixtureMonitor='')
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Environment]::OSVersion.Platform -ne 'Win32NT') { throw 'Windows-only acceptance' }
if($NegativeControl -and $StopPending){throw 'Run each control independently'}
if(($NegativeControl -or $StopPending) -and $Mode -ne 'compute'){throw 'Controls apply to the compute workflow'}
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
if ((New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Acceptance must run without administrator privileges' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'NativeObserver.cs')
$manifest=Get-Content $ManifestPath -Encoding UTF8 | ConvertFrom-Json
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$report=@{mode=$Mode;revision=$manifest.revision;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;user=$env:USERNAME;negative_control=[bool]$NegativeControl;stop_pending=[bool]$StopPending;scope='installed app, native WinForms receiver, deterministic local model'}
$app=$null;$appWindow=$null;$fixture=$null;$guard=$null;$listener=$null;$ownedDriver=$null
$multiply=[string][char]0xD7
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
function Nodes {
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($appWindow)
    return $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
}
function Find-Name([string]$text,[bool]$exact=$true) {
    foreach($node in (Nodes)) {
        try { $name=$node.Current.Name; if(-not $node.Current.IsOffscreen -and (($exact -and $name -eq $text) -or (-not $exact -and $name.Contains($text)))) {return $node} } catch {}
    }
    return $null
}
function Press($node) {
    for($i=0;$i -lt 8 -and $node;$i++) {
        $pattern=$null
        if($node.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) {$pattern.Invoke();return}
        $node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)
    }
    throw 'No invoke action for the selected control'
}
function Model-Request {
    $task=$listener.GetContextAsync()
    if(-not $task.Wait(20000)){throw 'Model request did not arrive'}
    $context=$task.Result
    if($context.Request.HttpMethod -ne 'POST' -or $context.Request.Url.AbsolutePath -ne '/api/chat'){throw 'Unexpected model protocol route'}
    $reader=New-Object IO.StreamReader($context.Request.InputStream)
    $body=$reader.ReadToEnd();$reader.Dispose()
    return @{context=$context;body=($body | ConvertFrom-Json);raw=$body}
}
function Expect-NoModelRequest([int]$seconds) {
    # A stopped batch must not ask the model for another decision.
    $task=$listener.GetContextAsync()
    if($task.Wait($seconds*1000)){
        $task.Result.Response.StatusCode=500;$task.Result.Response.Close()
        throw 'A failed or refused action was followed by another model request'
    }
}
function Fixture-Events {
    $path=Join-Path $work 'events.jsonl'
    if(-not (Test-Path $path)){return @()}
    return @(Get-Content $path -Encoding UTF8 | ForEach-Object {$_ | ConvertFrom-Json})
}
function Reply($context,$decision) {
    $payload=@{message=@{role='assistant';content=($decision | ConvertTo-Json -Depth 12 -Compress)}} | ConvertTo-Json -Depth 15 -Compress
    $bytes=[Text.Encoding]::UTF8.GetBytes($payload)
    $context.Response.ContentType='application/json';$context.Response.ContentLength64=$bytes.Length
    $context.Response.OutputStream.Write($bytes,0,$bytes.Length);$context.Response.Close()
}
try {
    # installMode "both": a silent install ignores /D=, so install per user to the product's default folder.
    $install=Join-Path $env:LOCALAPPDATA 'Programs\Copilot'
    $installer=Start-Process $manifest.installer -ArgumentList '/S','/CurrentUser' -PassThru
    if(-not $installer.WaitForExit(120000)){$installer.Kill();throw 'Installer exceeded two minutes'}
    if($installer.ExitCode -ne 0){throw "Installer failed: $($installer.ExitCode)"}
    $exe=Join-Path $install 'app.exe';$driver=Join-Path $install 'cua-driver.exe'
    if((Get-FileHash $exe -Algorithm SHA256).Hash -ne $manifest.app_sha256){throw 'Installed app hash mismatch'}
    if((Get-FileHash $driver -Algorithm SHA256).Hash -ne $manifest.driver_sha256){throw 'Installed driver hash mismatch'}
    if($DriverOverride){
        # Same installed app, only its sidecar replaced: isolates a driver change.
        Copy-Item $DriverOverride $driver -Force
        $report.driver_override=@{source=$DriverOverride;packaged_sha256=$manifest.driver_sha256;sha256=(Get-FileHash $driver -Algorithm SHA256).Hash}
    }
    $report.install=$install
    $fixtureExe=Join-Path $work 'ICUCalculatorFixture.exe'
    & 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /codepage:65001 /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll ("/out:"+$fixtureExe) (Join-Path $PSScriptRoot 'CalculatorFixture.cs')
    if($LASTEXITCODE){throw 'Fixture compilation failed'}
    $probeExe=Join-Path $work 'ICUProbeFixture.exe'
    & 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /codepage:65001 /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll ("/out:"+$probeExe) (Join-Path $PSScriptRoot 'ProbeFixture.cs')
    if($LASTEXITCODE){throw 'Probe fixture compilation failed'}
    # Each mode has one independent receiver; its callbacks are the oracle.
    $receiver=@{compute=@($fixtureExe,@($work),'ready.json','ICU Calculator');toolerror=@($fixtureExe,@($work),'ready.json','ICU Calculator')
        winkey=@($probeExe,@('--text',$work),'text-ready.json','ICU Text Alpha');chord=@($probeExe,@('--text',$work),'text-ready.json','ICU Text Alpha')
        scroll=@($probeExe,@('--scroll',$work),'scroll-ready.json','ICU Scroll')}[$Mode]
    $fixture=Start-Process $receiver[0] -ArgumentList $receiver[1] -PassThru
    Wait-For {Test-Path (Join-Path $work $receiver[2])} 10 'Fixture did not start' | Out-Null
    $ready=Get-Content (Join-Path $work $receiver[2]) -Encoding UTF8 | ConvertFrom-Json
    if($ready.pid -ne $fixture.Id){throw 'Fixture process identity mismatch'}
    if($FixtureMonitor){
        # By device name, so negative origins never pass through argument parsing.
        $monitor=@(& (Join-Path $PSScriptRoot 'layout.ps1') list | ConvertFrom-Json | ForEach-Object {$_} | Where-Object {$_.device -like "*\$FixtureMonitor"})
        if($monitor.Count -ne 1){throw "No monitor named $FixtureMonitor"}
        $report.fixture_monitor=$monitor[0]
        $FixtureOrigin="$($monitor[0].left+80),$($monitor[0].top+80)"
    }
    if($FixtureOrigin){
        # Physical placement, e.g. onto a scaled or negative-origin monitor.
        $receiverWindow=@($ready.calculator,$ready.alpha,$ready.window) | Where-Object {$_} | Select-Object -First 1
        $xy=$FixtureOrigin -split ','
        $report.fixture_window=& (Join-Path $PSScriptRoot 'layout.ps1') move $receiverWindow $xy[0] $xy[1] | ConvertFrom-Json
        if($LASTEXITCODE){throw 'Could not place the receiver window'}
    }
    $guard=Start-Process $fixtureExe -ArgumentList '--guard',$work -PassThru
    Wait-For {Test-Path (Join-Path $work 'guard-ready.json')} 10 'Independent focus guard did not start' | Out-Null
    $guardReady=Get-Content (Join-Path $work 'guard-ready.json') -Encoding UTF8 | ConvertFrom-Json
    if($guardReady.pid -ne $guard.Id -or $guard.Id -eq $fixture.Id){throw 'Focus guard must belong to a separate process'}
    $report.guard=$guardReady;$report.fixture=$ready
    $listener=New-Object Net.HttpListener;$listener.Prefixes.Add('http://localhost:11434/');$listener.Start()
    $env:ICU_BRAIN='ollama'
    $app=Start-Process $exe -PassThru
    # By class and size, never MainWindowHandle (see NativeObserver.FindTopLevel).
    $appWindow=Wait-For {$w=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($w -ne [IntPtr]::Zero){$w}} 15 'App window did not appear'
    $report.app_window=@{handle=$appWindow.ToInt64();class=[NativeObserver]::ClassOf($appWindow);main_window_handle_class=$(try{$app.Refresh();[NativeObserver]::ClassOf($app.MainWindowHandle)}catch{''})}
    $screen=[Windows.Forms.Screen]::PrimaryScreen.Bounds
    if($screen.Width -lt 1000 -or $screen.Height -lt 700){throw 'Test desktop is too small for the app'}
    if(-not [NativeObserver]::SetWindowPos($appWindow,[IntPtr]::Zero,0,0,[Math]::Min(1200,$screen.Width-20),[Math]::Min(800,$screen.Height-20),0x14)){throw 'Could not size app to the test desktop'}
    Press (Wait-For {Find-Name '+ window'} 20 'Window picker unavailable')
    Press (Wait-For {Find-Name $receiver[3] $false} 10 'Receiver missing from picker')
    $edit=Wait-For {foreach($node in (Nodes)){if($node.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit -and $node.Current.IsEnabled){return $node}}} 10 'Goal editor unavailable'
    $value=$null
    if(-not $edit.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$value)){throw 'Goal editor does not expose ValuePattern'}
    $goal=@{compute='Compute six times seven using the calculator buttons.';toolerror='Set the six button to nine, then press seven.'
        winkey='Open the Run dialog.';chord='Replace all text in Field Alpha with x.';scroll='Scroll up in the rows list.'}[$Mode]
    $value.SetValue($goal)
    Press (Wait-For {Find-Name 'Send'} 5 'Send unavailable')
    $first=Model-Request;$first.raw | Set-Content (Join-Path $work 'model-request-1.json') -Encoding UTF8
    $drivers=@(Get-CimInstance Win32_Process | Where-Object {$_.ParentProcessId -eq $app.Id -and $_.Name -eq 'cua-driver.exe' -and $_.CommandLine -match '\bserve\b'})
    if($drivers.Count -ne 1 -or $drivers[0].ExecutablePath -ne $driver){throw 'App did not launch its installed driver'}
    $ownedDriver=Get-Process -Id $drivers[0].ProcessId
    $null=$ownedDriver.Handle # retain this kernel process object across exit
    $report.driver=@{pid=$ownedDriver.Id;parent=$app.Id;path=$drivers[0].ExecutablePath;sha256=(Get-FileHash $driver -Algorithm SHA256).Hash}
    if($StopPending){Press (Wait-For {Find-Name 'Stop'} 5 'Stop unavailable during model request')}
    [NativeObserver]::ShowWindow([IntPtr]$guardReady.window,5) | Out-Null
    [NativeObserver]::FocusGuardForSetup([IntPtr]$guardReady.window)
    Wait-For {([NativeObserver]::Read()).foreground -eq $guardReady.window} 3 'Focus guard did not activate' | Out-Null
    [NativeObserver]::EstablishRecentInput();Start-Sleep -Milliseconds 100
    $baseline=[NativeObserver]::Read();$report.baseline=$baseline
    [NativeObserver]::Start()
    $before=@(Fixture-Events).Count
    $actions=@();foreach($label in @('6',$multiply,'7','=')){$actions+=@{kind='click';target=$label}}
    if($Mode -eq 'toolerror'){$actions=@(@{kind='set_value';target='6';value='9'},@{kind='click';target='7'})}
    # Deliberate red: a first action that succeeds must let the batch continue.
    if($Mode -eq 'toolerror' -and $MutateFirstAction){$actions[0]=@{kind='click';target='6'}}
    if($Mode -eq 'winkey'){$actions=@(@{kind='key';key='Win+r'},@{kind='type';text='must not arrive'})}
    if($Mode -eq 'chord'){$actions=@(@{kind='click';target='Field Alpha'},@{kind='key';key='Ctrl+a'},@{kind='type';text='x'})}
    if($Mode -eq 'scroll'){
        # Follow the scroll sign the app itself states in the model prompt.
        $system=@($first.body.messages | Where-Object {$_.role -eq 'system'}) | ForEach-Object {$_.content}
        $hint=[regex]::Match(($system -join ' ')+' '+$first.raw,'dy\\?"?:-3\}? negative=(up|down)')
        if(-not $hint.Success){throw 'Model prompt does not state the scroll sign'}
        $report.prompt_negative_scroll=$hint.Groups[1].Value
        $actions=@(@{kind='scroll';dy=-3})
    }
    Reply $first.context @{actions=$actions;thought='Use the independently implemented receiver.'}
    if($Mode -eq 'toolerror'){
        Wait-For {Find-Name 'error:' $false} 15 'Failure did not reach the GUI' | Out-Null
        $report.gui_error=(Find-Name 'error:' $false).Current.Name
        $report.gui_log=@(Nodes | ForEach-Object {$_.Current.Name})
        Expect-NoModelRequest 5
    } elseif($Mode -in @('chord','scroll','winkey')){
        # Either the next observation reaches the model, or a native refusal
        # stops the batch. They are different outcomes and are recorded as such.
        # Since refusal-feedback (2026-09-27) a refusal ends the batch and goes
        # back to the model as that step's result ("-> REFUSED" under ACTIONS
        # SO FAR); that next request is a refusal, not a delivery.
        $next=$listener.GetContextAsync()
        Wait-For {$next.IsCompleted -or (Find-Name 'error:' $false)} 20 'Neither a model request nor a failure arrived' | Out-Null
        if($next.IsCompleted){
            $context=$next.Result;$reader=New-Object IO.StreamReader($context.Request.InputStream)
            $raw=$reader.ReadToEnd();$reader.Dispose()
            $raw | Set-Content (Join-Path $work 'model-request-2.json') -Encoding UTF8
            Start-Sleep -Milliseconds 500
            Reply $context @{action=@{kind='done';summary='Observed receiver'}}
            Wait-For {Find-Name 'done. Observed receiver' $false} 10 'Completion did not reach the GUI' | Out-Null
            $refusedLine=[regex]::Match($raw,'[^\\"]*-> REFUSED[^\\"]*')
            if($refusedLine.Success){$report.outcome='refused_to_model';$report.refusal_fed_back=$refusedLine.Value}else{$report.outcome='delivered'}
        } else {
            $report.outcome='refused';$report.gui_error=(Find-Name 'error:' $false).Current.Name
            $report.gui_log=@(Nodes | ForEach-Object {$_.Current.Name})
            if($next.Wait(5000)){throw 'A refused action was followed by another model request'}
        }
    } elseif($StopPending){
        Wait-For {Find-Name 'stopped:' $false} 10 'Stopped outcome did not reach the GUI' | Out-Null
        Start-Sleep -Milliseconds 1000
    } else {
        $second=Model-Request;$second.raw | Set-Content (Join-Path $work 'model-request-2.json') -Encoding UTF8
        $prompt=@($second.body.messages | Where-Object {$_.role -eq 'user'})[0].content
        $tree=($prompt -split 'WINDOW ELEMENT TREE:',2)[1]
        if(-not $tree -or -not $tree.Contains('"42"')){throw 'Fresh model observation omitted 42'}
        if($NegativeControl){[NativeObserver]::PerturbPointerForNegativeControl()}
        Reply $second.context @{action=@{kind='done';summary='Observed 42'}}
        Wait-For {Find-Name 'done. Observed 42' $false} 10 'Completion did not reach the GUI' | Out-Null
    }
    $send=Wait-For {Find-Name 'Send'} 5 'Send was not restored'
    if(-not $send.Current.IsEnabled){throw 'Send remains disabled'}
    $events=@(Fixture-Events);$report.events=$events
    $after=@($events | Select-Object -Skip $before)
    if($Mode -eq 'toolerror'){
        $state=Get-Content (Join-Path $work 'state.json') -Encoding UTF8 | ConvertFrom-Json
        if($state.display -ne '0' -or $state.buttons.Count -ne 0 -or $events.Count){throw 'Input followed the failed native action'}
    } elseif($Mode -eq 'winkey'){
        if(@($after | Where-Object {$_.kind -in @('keydown','text')}).Count){throw 'Keyboard input reached the receiver'}
        if($report.outcome -eq 'delivered'){throw 'Win+r was not refused'}
        if((($report.gui_log -join ' ')+' '+$report.refusal_fed_back) -notmatch 'Windows/Meta key is non-negotiably denied'){throw 'Refusal did not name the Windows key policy'}
    } elseif($Mode -eq 'chord'){
        $alpha=@($after | Where-Object {$_.kind -eq 'text' -and $_.window -eq 'Alpha'})
        if(@($after | Where-Object {$_.window -eq 'Beta' -and $_.kind -ne 'focus'}).Count){throw 'Input reached the other window'}
        if($report.outcome -in @('refused','refused_to_model')){
            # An honest refusal must leave the field untouched.
            if($alpha.Count){throw "Refused chord still changed the field to $($alpha[-1].value)"}
        } elseif(-not $alpha.Count -or $alpha[-1].value -ne 'x'){throw 'Chord did not select all before typing'}
    } elseif($Mode -eq 'scroll'){
        if($report.outcome -in @('refused','refused_to_model')){throw "Scroll was refused: $($report.gui_error)$($report.refusal_fed_back)"}
        $tops=@($after | Where-Object {$_.kind -eq 'top'})
        if(-not $tops.Count){throw 'Scroll did not move the list'}
        $moved=if($tops[-1].top -lt 100){'up'}else{'down'};$report.scrolled=$moved
        if($moved -ne $report.prompt_negative_scroll){throw "Negative scroll moved $moved; the model prompt says $($report.prompt_negative_scroll)"}
    }
    # Text callbacks carry the foreground window seen at delivery time.
    if($Mode -ne 'compute'){$events=@($after | Where-Object {$_.kind -eq 'text'})}
    $state=if($Mode -eq 'compute'){Get-Content (Join-Path $work 'state.json') -Encoding UTF8 | ConvertFrom-Json}
    if($Mode -ne 'compute'){} elseif($StopPending){
        if($state.display -ne '0' -or $state.buttons.Count -ne 0){throw 'Input arrived after Stop'}
    } elseif($state.display -ne '42' -or ($state.buttons -join '|') -ne ('6|'+$multiply+'|7|=')){throw 'Receiver effect/order mismatch'}
    if($Mode -eq 'compute'){
        if($StopPending -and $events.Count){throw 'Native callback arrived after Stop'}
        if(-not $StopPending -and $events.Count -ne 4){throw 'Native callback count mismatch'}
    }
    if(@($events | Where-Object {$_.foreground -ne $guardReady.window}).Count){throw 'Native callback disturbed foreground'}
    $samples=[NativeObserver]::Stop();$report.samples=$samples
    if([NativeObserver]::Error){throw ('Ownership sampling failed: '+[NativeObserver]::Error)}
    if(-not $samples.Count){throw 'No ownership samples'}
    foreach($sample in $samples){foreach($field in @('foreground','focus','x','y','lastInput')){if($sample.$field -ne $baseline.$field){throw "Ownership changed: $field"}}}
    $report.receiver=$state;$report.ok=$true
} catch {
    $report.ok=$false;$report.error=$_.Exception.Message
    try {$report.nodes=@(Nodes | ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName}})} catch {}
} finally {
    $report.samples=[NativeObserver]::Stop()
    try {
        $bounds=[Windows.Forms.Screen]::PrimaryScreen.Bounds
        $image=New-Object Drawing.Bitmap($bounds.Width,$bounds.Height)
        $graphics=[Drawing.Graphics]::FromImage($image)
        $graphics.CopyFromScreen($bounds.Location,[Drawing.Point]::Empty,$bounds.Size)
        $image.Save((Join-Path $work 'desktop.png'),[Drawing.Imaging.ImageFormat]::Png)
        $graphics.Dispose();$image.Dispose()
    } catch {}
    if($listener){$listener.Stop();$listener.Close()}
    if($app -and -not $app.HasExited){if($appWindow){[NativeObserver]::PostClose($appWindow) | Out-Null};if(-not $app.WaitForExit(20000)){$report.ok=$false;$report.cleanup_error='App did not close within 20 seconds';$app.Kill()}}
    if($ownedDriver){
        $report.driver_exited=$ownedDriver.WaitForExit(20000)
        if(-not $report.driver_exited){$report.ok=$false;$report.cleanup_error='Owned driver outlived app shutdown';$ownedDriver.Kill()}
    }
    if($fixture -and -not $fixture.HasExited){$fixture.Kill();$fixture.WaitForExit()}
    if($guard -and -not $guard.HasExited){$guard.Kill();$guard.WaitForExit()}
    # A control passes only when it fails for the reason it exists to cause.
    # Exit 1 alone proves nothing: in the 2026-09-25 review a negative control
    # "failed" because UIA Invoke threw "Unrecognized error" before the probe.
    $expectedFailure=if($NegativeControl){'^Ownership changed: (x|y)$'}elseif($MutateFirstAction){'^Failure did not reach the GUI$'}else{$null}
    if($expectedFailure){
        $report.expected_failure=$expectedFailure
        $report.control=if($report.ok){'did_not_fail'}elseif(-not $report.cleanup_error -and "$($report.error)" -match $expectedFailure){'failed_as_required'}else{'failed_for_another_reason'}
        $report.ok=($report.control -eq 'failed_as_required')
    }
    $report.work=$work
    $report | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $work 'result.json') -Encoding UTF8
    @{work=$work;ok=$report.ok;control=$report.control;error=$report.error} | ConvertTo-Json | Set-Content (Join-Path $env:LOCALAPPDATA 'ICUAudit\latest.json') -Encoding UTF8
}
if(-not $report.ok){exit 1}
