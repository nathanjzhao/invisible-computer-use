param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$Work,[string]$Model='gpt-6-astra',[string]$AppExe='')
# Native user setup of the real app. The model, app, and driver perform the task.
$ErrorActionPreference='Stop'
if($env:ICU_DISPOSABLE_HOST -ne '1' -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){throw 'owned disposable interactive Windows host required'}
if(Test-Path $Work){throw 'fresh evidence directory required'}
New-Item -Type Directory $Work|Out-Null
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $Root 'NativeObserver.cs')
$exe=if($AppExe){$AppExe}else{Join-Path $Root 'candidate\app.exe'}
$fixture=Join-Path $Root 'PairFixture.exe'
$report=[ordered]@{engine='icu-packaged-app';mode='model-driven-product';model=$Model;passed=$false;cases=@();app_sha256=(Get-FileHash $exe).Hash.ToLowerInvariant();fixture_sha256=(Get-FileHash $fixture).Hash.ToLowerInvariant();harness_sha256=(Get-FileHash $PSCommandPath).Hash.ToLowerInvariant()}
function Wait-For($pred,[int]$seconds,[string]$why){$end=(Get-Date).AddSeconds($seconds);while((Get-Date) -lt $end){$v=& $pred;if($v){return $v};Start-Sleep -Milliseconds 200};throw $why}
function Nodes { [Windows.Automation.AutomationElement]::FromHandle($script:window).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) }
function Find-Name([string]$name,[bool]$exact=$true){foreach($n in (Nodes)){try{if(-not $n.Current.IsOffscreen -and (($exact -and $n.Current.Name -eq $name) -or (-not $exact -and $n.Current.Name.Contains($name)))){return $n}}catch{}};return $null}
function Find-Id([string]$id){foreach($n in (Nodes)){try{if($n.Current.AutomationId -eq $id){return $n}}catch{}};return $null}
function Press($n){if(-not $n){throw 'button absent'};$p=$null;if($n.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){$p.Invoke();return};throw 'no invoke action'}
function Set-Value($n,[string]$value){$p=$null;if(-not $n.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$p)){throw 'no value pattern'};$p.SetValue($value);Wait-For {$p.Current.Value -eq $value} 5 'full value did not reach editor'|Out-Null}
function Shot([string]$file){$b=[Windows.Forms.SystemInformation]::VirtualScreen;$i=New-Object Drawing.Bitmap($b.Width,$b.Height);$g=[Drawing.Graphics]::FromImage($i);try{$g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size);$i.Save($file,[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$i.Dispose()}}
$tasks=@(
 @{id='calculator';goal='Compute six times seven by using the calculator buttons. Read the result before finishing.';expected=@{display='42';draft='seed';Alpha='alpha'}},
 @{id='text_replacement';goal='Replace the Draft field text with paired text. Keep the Alpha document unchanged.';expected=@{display='0';draft='paired text';Alpha='alpha'}}
)
foreach($task in $tasks){
 $dir=Join-Path $Work $task.id;New-Item -Type Directory $dir|Out-Null
 $data=Join-Path $dir 'fixture';New-Item -Type Directory $data|Out-Null
 $r=[ordered]@{id=$task.id;goal=$task.goal;expected=$task.expected;passed=$false};$report.cases+=,$r
 $app=$null;$fx=$null;$owned=@();$script:window=[IntPtr]::Zero
 try{
  $fx=Start-Process $fixture -ArgumentList $data -PassThru
  Wait-For {Test-Path (Join-Path $data 'state.json')} 10 'fixture did not start'|Out-Null
  $env:ICU_APP_MEMORY_ROOT=Join-Path $dir 'memory';$env:ICU_OPENAI_MODEL=$Model
  $app=Start-Process $exe -PassThru -RedirectStandardOutput (Join-Path $dir 'app.stdout.log') -RedirectStandardError (Join-Path $dir 'app.stderr.log')
  $script:window=Wait-For {$h=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($h -ne [IntPtr]::Zero){$h}} 40 'app window unavailable'
  [NativeObserver]::SetWindowPos($script:window,[IntPtr]::Zero,0,0,1200,800,0x14)|Out-Null
  [NativeObserver]::FocusGuardForSetup($script:window)
  $r.stage='provider'
  $combo=Wait-For {Find-Id 'brain-select'} 20 'provider select unavailable'
  $combo.SetFocus()
  [Windows.Forms.SendKeys]::SendWait('{HOME}{DOWN}{DOWN}{ENTER}')
  Wait-For {
   $n=Find-Id 'brain-select';$p=$null
   if($n.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$p)){return $p.Current.Value -eq 'OpenAI'}
   if($n.TryGetCurrentPattern([Windows.Automation.SelectionPattern]::Pattern,[ref]$p)){return @($p.GetCurrentSelection()|Where-Object{$_.Current.Name -eq 'OpenAI'}).Count -eq 1}
  } 8 'OpenAI provider selection did not take'|Out-Null
  $r.stage='memory-options'
  foreach($id in @('run-use-memory','run-remember')){$n=Find-Id $id;$pattern=$null;if(-not $n -or -not $n.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){throw 'memory checkbox unavailable'};if($pattern.Current.ToggleState -eq [Windows.Automation.ToggleState]::On){$pattern.Toggle()}}
  $r.stage='picker-open'
  Press (Wait-For {Find-Name '+ window'} 10 'window picker unavailable')
  $r.stage='picker-selection'
  Press (Wait-For {Find-Name 'Paired task fixture' $false} 15 'fixture absent from picker')
  $r.stage='goal'
  $entry=Wait-For {Find-Id 'chat-input'} 10 'goal input absent'
  Set-Value $entry $task.goal
  $r.stage='send'
  Press (Wait-For {Find-Name 'Send'} 5 'Send unavailable')
  $started=Get-Date
  Wait-For {Find-Name 'Stop'} 15 'task did not start'|Out-Null
  $owned=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id)" | Where-Object {$_.Name -eq 'cua-driver.exe' -and $_.CommandLine -match ' serve '} | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine)
  $r.owned_driver=$owned
  if($owned.Count -ne 1){throw 'owned native driver not established'}
  Wait-For {if(-not (Find-Name 'Stop')){Find-Name 'Send'}} 300 'task did not finish'|Out-Null
  $r.wall_seconds=((Get-Date)-$started).TotalSeconds
  $r.fixture_state=Wait-For {try{Get-Content (Join-Path $data 'state.json') -Raw -Encoding UTF8 -ErrorAction Stop|ConvertFrom-Json}catch{$null}} 5 'fixture oracle could not be read'
  $r.outcome=@{display=$r.fixture_state.display;draft=$r.fixture_state.draft;Alpha=$r.fixture_state.Alpha}
  $r.chat=@(Nodes|ForEach-Object{try{$_.Current.Name}catch{}}|Where-Object{$_})
  $r.model_confirmed=($r.chat -join "`n").Contains('codex-oauth:'+$Model)
  $r.model_completed=@($r.chat|Where-Object{$_ -like 'done.*'}).Count -gt 0
  $r.passed=$r.model_confirmed -and $r.model_completed -and $r.outcome.display -eq $task.expected.display -and $r.outcome.draft -eq $task.expected.draft -and $r.outcome.Alpha -eq $task.expected.Alpha
  if($task.id -eq 'calculator'){$r.passed=$r.passed -and ($r.fixture_state.buttons -join ',') -eq (@('6',[string][char]0xD7,'7','=') -join ',')}
  Shot (Join-Path $dir 'after.png')
 }catch{$r.error=$_.Exception.Message;$r.error_location=$_.InvocationInfo.PositionMessage;try{$r.ui_nodes=@(Nodes|ForEach-Object{@{name=$_.Current.Name;id=$_.Current.AutomationId;type=$_.Current.ControlType.ProgrammaticName}});Shot (Join-Path $dir 'failure.png')}catch{}}
 finally{
  if($app -and -not $app.HasExited){[NativeObserver]::PostClose($script:window)|Out-Null;if(-not $app.WaitForExit(30000)){$app.Kill();$app.WaitForExit();$r.forced_app_cleanup=$true}}
  if($fx -and -not $fx.HasExited){$fx.Kill();$fx.WaitForExit()}
  $r.owned_daemon_exited=@($owned|Where-Object{Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue}).Count -eq 0
  $r.owned_processes_reaped=(!$app -or $app.HasExited) -and (!$fx -or $fx.HasExited)
  $r.passed=$r.passed -and $r.owned_daemon_exited -and $r.owned_processes_reaped -and -not $r.forced_app_cleanup
  $r|ConvertTo-Json -Depth 16|Set-Content (Join-Path $dir 'result.json') -Encoding UTF8
 }
}
$report.passed=$report.cases.Count -eq 2 -and @($report.cases|Where-Object{-not $_.passed}).Count -eq 0
$report|ConvertTo-Json -Depth 18|Set-Content (Join-Path $Work 'result.json') -Encoding UTF8
([pscustomobject]$report)|Select-Object engine,model,passed,@{N='cases';E={@($_.cases|ForEach-Object{[pscustomobject]$_|Select-Object id,passed,error,outcome,model_confirmed,owned_daemon_exited,owned_processes_reaped})}}|ConvertTo-Json -Depth 8
if(-not $report.passed){exit 1}
