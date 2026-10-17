param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$Work,[string]$AppExe='C:\Program Files\ICU Gap4\app.exe',[int]$Seed=0,[switch]$WithoutScreenshots)
$ErrorActionPreference='Stop'
if($env:ICU_DISPOSABLE_HOST -ne '1' -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){throw 'owned interactive test desktop required'}
if(Test-Path $Work){throw 'fresh evidence directory required'}
New-Item -ItemType Directory $Work|Out-Null
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path "$Root\NativeObserver.cs"
function Wait-For($predicate,[int]$seconds,[string]$why){$end=(Get-Date).AddSeconds($seconds);while((Get-Date) -lt $end){$v=& $predicate;if($v){return $v};Start-Sleep -Milliseconds 200};throw $why}
function Nodes{[Windows.Automation.AutomationElement]::FromHandle($script:window).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)}
function Find-Id($id){foreach($n in (Nodes)){if($n.Current.AutomationId -eq $id){return $n}}}
function Find-Name($name,[bool]$exact=$true){foreach($n in (Nodes)){if(-not $n.Current.IsOffscreen -and (($exact -and $n.Current.Name -eq $name) -or (!$exact -and ([string]$n.Current.Name).Contains($name)))){return $n}}}
function Press($n){if(!$n){throw 'button absent'};$p=$null;if(!$n.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){throw 'invoke pattern absent'};$p.Invoke()}
function Toggle($id,$on){$n=Find-Id $id;$p=$null;if(!$n.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$p)){throw 'checkbox absent'};if(($p.Current.ToggleState -eq [Windows.Automation.ToggleState]::On) -ne $on){$p.Toggle()}}
function Shot($path){$b=[Windows.Forms.SystemInformation]::VirtualScreen;$i=New-Object Drawing.Bitmap($b.Width,$b.Height);$g=[Drawing.Graphics]::FromImage($i);try{$g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size);$i.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$i.Dispose()}}
$r=[ordered]@{passed=$false;model='gpt-6-astra';screenshots_enabled=(!$WithoutScreenshots);app_sha256=(Get-FileHash $AppExe).Hash.ToLowerInvariant();seed=$Seed};$fx=$null;$app=$null;$owned=@();$script:window=[IntPtr]::Zero
try{
 $state=Join-Path $Work 'state.json'
 # Suppress first-login Edge welcome on this owned disposable desktop only.
 Get-Process msedge -ErrorAction SilentlyContinue|Where-Object{$_.SessionId -eq [Diagnostics.Process]::GetCurrentProcess().SessionId}|Stop-Process -Force
 Start-Sleep -Seconds 1
 $fx=Start-Process "$Root\VisualFixture.exe" -ArgumentList $state,$Seed -PassThru
 $fx.Handle|Out-Null;Wait-For {Test-Path $state} 10 'visual fixture missing'|Out-Null
 $env:ICU_APP_MEMORY_ROOT=Join-Path $Work 'memory';$env:ICU_OPENAI_MODEL='gpt-6-astra'
 $app=Start-Process $AppExe -PassThru -RedirectStandardOutput "$Work\app.out" -RedirectStandardError "$Work\app.err"
 $script:window=Wait-For {$h=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($h -ne [IntPtr]::Zero){$h}} 40 'app missing'
 [NativeObserver]::SetWindowPos($script:window,[IntPtr]::Zero,0,0,1010,730,0x14)|Out-Null;[NativeObserver]::FocusGuardForSetup($script:window)
 $combo=Wait-For {Find-Id 'brain-select'} 15 'provider unavailable';$combo.SetFocus();Start-Sleep -Milliseconds 200;[Windows.Forms.SendKeys]::SendWait('{HOME}{DOWN}{DOWN}{ENTER}')
 Wait-For {$p=$null;$n=Find-Id 'brain-select';if($n.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$p)){return $p.Current.Value -eq 'OpenAI'};if($n.TryGetCurrentPattern([Windows.Automation.SelectionPattern]::Pattern,[ref]$p)){return @($p.GetCurrentSelection()|Where-Object{$_.Current.Name -eq 'OpenAI'}).Count -eq 1}} 8 'OpenAI not selected'|Out-Null
 Toggle 'run-use-memory' $false;Toggle 'run-remember' $false;Toggle 'run-use-screenshots' (!$WithoutScreenshots)
 Press (Find-Name '+ window');Press (Wait-For {Find-Name 'Visual parity fixture' $false} 15 'visual target absent')
 $entry=Find-Id 'chat-input';$pattern=$null;if(!$entry.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){throw 'goal unavailable'}
 $goal='Click the purple circle exactly once, then verify that the window says Selection confirmed. Leave the Unchanged visual guard window untouched.'
 $pattern.SetValue($goal);$r.goal=$goal;Press (Find-Name 'Send');Wait-For {Find-Name 'Stop'} 15 'task did not start'|Out-Null
 $owned=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id)"|Where-Object{$_.Name -eq 'cua-driver.exe'})
 Wait-For {!(Find-Name 'Stop') -and (Find-Name 'Send')} 180 'visual task did not finish'|Out-Null
 $r.oracle=Get-Content $state -Raw -Encoding UTF8|ConvertFrom-Json;$r.chat=@(Nodes|ForEach-Object{$_.Current.Name});$r.model_confirmed=($r.chat -join ' ').Contains('codex-oauth:gpt-6-astra');$r.model_completed=@($r.chat|Where-Object{$_ -like 'done.*'}).Count -gt 0
 $r.passed=$r.model_confirmed -and $r.model_completed -and $r.oracle.completed -and $r.oracle.wrong -eq 0 -and @($r.oracle.actions).Count -eq 1 -and $r.oracle.guard -eq 'guard unchanged'
 if($WithoutScreenshots){$r.passed=$r.model_confirmed -and !$r.model_completed -and !$r.oracle.completed -and @($r.oracle.actions).Count -eq 0 -and $r.oracle.guard -eq 'guard unchanged'}
 Shot "$Work\after.png"
}catch{$r.error=$_.Exception.Message;$r.location=$_.InvocationInfo.PositionMessage;try{$r.chat=@(Nodes|ForEach-Object{$_.Current.Name});Shot "$Work\failure.png"}catch{}}
finally{
 if($app -and !$app.HasExited){[NativeObserver]::PostClose($script:window)|Out-Null;if(!$app.WaitForExit(30000)){$app.Kill();$app.WaitForExit();$r.forced_cleanup=$true}}
 if($fx -and !$fx.HasExited){$fx.Kill();$fx.WaitForExit()}
 $r.cleanup_verified=(!$app -or $app.HasExited) -and (!$fx -or $fx.HasExited) -and @($owned|Where-Object{Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue}).Count -eq 0 -and !$r.forced_cleanup
 $r.passed=$r.passed -and $r.cleanup_verified
 $r|ConvertTo-Json -Depth 14|Set-Content "$Work\result.json" -Encoding UTF8
 $r|Select-Object passed,error,seed,model_confirmed,model_completed,oracle,cleanup_verified|ConvertTo-Json -Depth 6
}
if(!$r.passed){exit 1}
