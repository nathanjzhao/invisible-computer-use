param([string]$Root='C:\ICUAudit\paired',[Parameter(Mandatory=$true)][string]$Work)
$ErrorActionPreference='Stop'
if($env:ICU_DISPOSABLE_HOST -ne '1' -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){throw 'disposable interactive host required'}
if(Test-Path $Work){throw 'fresh work directory required'}
New-Item -Type Directory $Work|Out-Null
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $Root 'NativeObserver.cs')
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class DemoPointer {
 [DllImport("user32.dll")]static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")]static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 public static void Click(int x,int y){if(!SetCursorPos(x,y))throw new Exception("pointer placement failed");mouse_event(2,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(20);mouse_event(4,0,0,0,UIntPtr.Zero);}
}
'@
$exe='C:\Program Files\ICU Gap3\app.exe'
$r=[ordered]@{passed=$false;app_sha256=(Get-FileHash $exe).Hash.ToLowerInvariant();harness_sha256=(Get-FileHash $PSCommandPath).Hash.ToLowerInvariant()}
function Wait-For($pred,[int]$seconds,[string]$why){$end=(Get-Date).AddSeconds($seconds);while((Get-Date) -lt $end){$v=& $pred;if($v){return $v};Start-Sleep -Milliseconds 200};throw $why}
function Nodes { [Windows.Automation.AutomationElement]::FromHandle($script:window).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) }
function Find-Name([string]$name,[bool]$exact=$true){foreach($n in (Nodes)){try{if(-not $n.Current.IsOffscreen -and (($exact -and $n.Current.Name -eq $name) -or (-not $exact -and $n.Current.Name.Contains($name)))){return $n}}catch{}};return $null}
function Find-Id([string]$id){foreach($n in (Nodes)){try{if($n.Current.AutomationId -eq $id){return $n}}catch{}};return $null}
function Press($n){if(-not $n){throw 'button absent'};$p=$null;if($n.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){$p.Invoke();return};throw 'no invoke action'}
function Set-Value($n,[string]$value){$p=$null;if(-not $n.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$p)){throw 'no value pattern'};$p.SetValue($value);Wait-For {$p.Current.Value -eq $value} 5 'full value did not reach editor'|Out-Null}
function Shot([string]$file){$b=[Windows.Forms.SystemInformation]::VirtualScreen;$i=New-Object Drawing.Bitmap($b.Width,$b.Height);$g=[Drawing.Graphics]::FromImage($i);try{$g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size);$i.Save($file,[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$i.Dispose()}}
function Select-Last([string]$id){$n=Find-Id $id;if(-not $n){throw "select absent: $id"};$n.SetFocus();[Windows.Forms.SendKeys]::SendWait('{END}{ENTER}')}
function Write-Utf8([string]$path,[string]$text){[IO.File]::WriteAllText($path,$text,(New-Object Text.UTF8Encoding($false)))}

$app=$null;$fx=$null;$rec=$null;$owned=@();$script:window=[IntPtr]::Zero
try {
 # Only the fixture's real input is demonstrated. Its state file is a separate oracle.
 $demo=Join-Path $Work 'demonstration';New-Item -ItemType Directory $demo|Out-Null
 $fx=Start-Process (Join-Path $Root 'PairFixture.exe') -ArgumentList $demo -PassThru
 Wait-For {Test-Path "$demo\state.json"} 15 'demonstration fixture missing'|Out-Null
 $state=Get-Content "$demo\state.json" -Raw -Encoding UTF8|ConvertFrom-Json
 [NativeObserver]::FocusGuardForSetup([IntPtr]$state.window)
 if([NativeObserver]::Read().foreground -ne [long]$state.window){throw 'demonstration fixture did not take the foreground'}
 $observer=Join-Path $Root 'observer.exe';$r.observer_sha256=(Get-FileHash $observer).Hash.ToLowerInvariant()
 $rec=Start-Process $observer -ArgumentList "record --out $demo --session demo --secs 13 --screen false --input false --net false --devices false --audio false --ui true --ui-values false" -PassThru -NoNewWindow -RedirectStandardOutput "$demo\observer.out" -RedirectStandardError "$demo\observer.err"
 $rec.Handle|Out-Null
 Start-Sleep -Seconds 3
 if([NativeObserver]::Read().foreground -ne [long]$state.window){throw 'another app covered the demonstration before recording input'}
 foreach($label in @('6',[string][char]0x00D7,'7','=')){
  $top=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$state.window)
  $button=$top.FindFirst([Windows.Automation.TreeScope]::Descendants,(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,$label)))
  if(-not $button){throw "demonstration control missing: $label"}
  $b=$button.Current.BoundingRectangle;[DemoPointer]::Click([int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2));Start-Sleep -Milliseconds 650
 }
 if(-not $rec.WaitForExit(25000) -or $rec.ExitCode -ne 0){throw 'demonstration observer did not finish cleanly'}
 $recording=Join-Path $demo 'recording-demo';$events=@(Get-Content "$recording\events.jsonl" -Encoding UTF8|ForEach-Object{$_|ConvertFrom-Json})
 $clicks=@($events|Where-Object{$_.kind -eq 'ui.click' -and $_.payload.pid -eq $fx.Id})
 $r.demo_oracle=Get-Content "$demo\state.json" -Raw -Encoding UTF8|ConvertFrom-Json
 $r.demo_labels=@($clicks|ForEach-Object{$_.payload.label})
 $r.recording_sha256=(Get-FileHash "$recording\events.jsonl").Hash.ToLowerInvariant()
 $r.raw_input_events=@($events|Where-Object{$_.kind -like 'input.*'}).Count
 $r.recorded_values=@($events|Where-Object{$_.kind -like 'ui.*' -and $_.payload.PSObject.Properties.Name -contains 'value'}).Count
 if(($r.demo_labels -join ',') -ne ('6,'+[char]0x00D7+',7,=') -or $r.demo_oracle.display -ne '42' -or $r.raw_input_events -or $r.recorded_values){throw 'native demonstration acceptance failed'}
 $fx.Kill();$fx.WaitForExit();$fx=$null

 $env:ICU_APP_MEMORY_ROOT=Join-Path $Work 'memory'
 New-Item -ItemType Directory -Force $env:ICU_APP_MEMORY_ROOT|Out-Null
 Write-Utf8 (Join-Path $env:ICU_APP_MEMORY_ROOT 'settings.json') '{"chatMemories":true,"skysight":false,"paused":false,"retentionDays":7,"idleMinutes":1,"provider":""}'
 $env:ICU_OPENAI_MODEL='gpt-6-astra'
 $app=Start-Process $exe -PassThru
 $script:window=Wait-For {$h=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($h -ne [IntPtr]::Zero){$h}} 40 'app window missing'
 [NativeObserver]::SetWindowPos($script:window,[IntPtr]::Zero,0,0,1010,730,0x14)|Out-Null
 [NativeObserver]::FocusGuardForSetup($script:window)
 Press (Wait-For {Find-Id 'memory-chip'} 15 'memory panel unavailable')
 $details=Wait-For {Find-Name 'Learn from a recording'} 10 'recording controls unavailable'
 $details.SetFocus();[Windows.Forms.SendKeys]::SendWait('{ENTER}')
 $path=Wait-For {Find-Id 'trajectory-path'} 10 'recording path absent'
 Set-Value $path $recording
 Press (Find-Id 'trajectory-review')
 Wait-For {Find-Name 'Observed ui.click' $false} 20 'actual preview events missing'|Out-Null
 $r.review_text=@(Nodes|ForEach-Object{try{$_.Current.Name}catch{}}|Where-Object{$_ -like 'Line *' -or $_ -like '*events will be imported*'})

 Set-Value (Find-Id 'trajectory-project') 'windows-reuse'
 Set-Value (Find-Id 'trajectory-goal') 'This demonstration shows multiplication controls in this app.'
 Select-Last 'trajectory-actor';Select-Last 'trajectory-provider'
 $consent=Find-Id 'trajectory-consent';$toggle=$null
 if(-not $consent.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$toggle)){throw 'consent has no toggle pattern'}
 $toggle.Toggle();Wait-For {(Find-Id 'trajectory-import').Current.IsEnabled} 5 'consent did not enable import'|Out-Null
 Press (Find-Id 'trajectory-import')
 Wait-For {Find-Name 'The full recording will be summarized' $false} 90 'import did not finish'|Out-Null
 $durable=Join-Path $env:ICU_APP_MEMORY_ROOT 'durable.json'
 Wait-For {if(Test-Path $durable){try{return @((Get-Content $durable -Raw -Encoding UTF8|ConvertFrom-Json).entries).Count -gt 0}catch{}}} 180 'memory did not summarize'|Out-Null
 $r.entries=(Get-Content $durable -Raw -Encoding UTF8|ConvertFrom-Json).entries
 if(@($r.entries|Where-Object{-not $_.untrusted}).Count){throw 'observed demonstration gained trusted authority'}
 $r.summaries=@(Get-ChildItem (Join-Path $env:ICU_APP_MEMORY_ROOT 'summaries') -Filter '*.json'|ForEach-Object{Get-Content $_.FullName -Raw -Encoding UTF8|ConvertFrom-Json})
 $origins=@($r.summaries|ForEach-Object{$_.recordings})
 if(-not @($origins|Where-Object{$_.recording_sha256 -eq $r.recording_sha256 -and $_.actor -eq 'synthetic_fixture'}).Count){throw 'memory lost native recording provenance'}
 Press (Find-Id 'memory-chip')
 $data=Join-Path $Work 'reuse-fixture';New-Item -ItemType Directory $data|Out-Null
 $fx=Start-Process (Join-Path $Root 'PairFixture.exe') -ArgumentList $data -PassThru
 Wait-For {Test-Path "$data\state.json"} 15 'reuse fixture missing'|Out-Null
 [NativeObserver]::FocusGuardForSetup($script:window)
 Wait-For {[NativeObserver]::Read().foreground -eq $script:window.ToInt64()} 5 'controller did not regain focus for provider selection'|Out-Null
 $combo=Find-Id 'brain-select';$combo.SetFocus();[Windows.Forms.SendKeys]::SendWait('{HOME}{DOWN}{DOWN}{ENTER}')
 Wait-For {
  $n=Find-Id 'brain-select';$pattern=$null
  if($n.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){return $pattern.Current.Value -eq 'OpenAI'}
  if($n.TryGetCurrentPattern([Windows.Automation.SelectionPattern]::Pattern,[ref]$pattern)){return @($pattern.GetCurrentSelection()|Where-Object{$_.Current.Name -eq 'OpenAI'}).Count -eq 1}
 } 8 'OpenAI provider selection did not take'|Out-Null
 Press (Find-Id 'add-window')
 Press (Wait-For {Find-Name 'Paired task fixture' $false} 20 'reuse fixture absent from picker')
 Set-Value (Find-Id 'chat-input') 'Compute seven times six using the buttons. Read the result before finishing.'
 Press (Find-Name 'Send')
 Wait-For {Find-Name 'Stop'} 15 'reuse task did not start'|Out-Null
 $r.fixture_confirmations=@()
 Wait-For {
  $allow=Find-Name 'Allow this step'
  if($allow){
   $text=(@(Nodes|ForEach-Object{$_.Current.Name}) -join ' ')
   if($text -notmatch 'PairFixture|Paired task fixture'){throw 'confirmation outside the owned fixture'}
   Press $allow;$r.fixture_confirmations+='explicit approval for this synthetic fixture operation'
  }
  !(Find-Name 'Stop') -and (Find-Name 'Send')
 } 120 'reuse task did not end'|Out-Null
 $r.oracle=Get-Content "$data\state.json" -Raw -Encoding UTF8|ConvertFrom-Json
 $r.chat=@(Nodes|ForEach-Object{try{$_.Current.Name}catch{}})
 $r.memory_used=($r.chat -join ' ') -match '\d+ notes? given to the model'
 $r.model_completed=@($r.chat|Where-Object{$_ -like 'done.*'}).Count -gt 0
 $r.model_confirmed=($r.chat -join ' ').Contains('codex-oauth:gpt-6-astra')
 $r.passed=$r.oracle.display -eq '42' -and $r.oracle.Alpha -eq 'alpha' -and $r.oracle.draft -eq 'seed' -and $r.memory_used -and $r.model_completed -and $r.model_confirmed -and @($r.entries).Count -gt 0
 $owned=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id)"|Where-Object{$_.Name -eq 'cua-driver.exe'})
 Shot (Join-Path $Work 'review.png')

} catch {$r.error=$_.Exception.Message;$r.error_location=$_.InvocationInfo.PositionMessage;try{$r.nodes=@(Nodes|ForEach-Object{@{name=$_.Current.Name;id=$_.Current.AutomationId;type=$_.Current.ControlType.ProgrammaticName}});Shot (Join-Path $Work 'failure.png')}catch{}}
finally {
 if($app -and -not $app.HasExited){[NativeObserver]::PostClose($script:window)|Out-Null;if(-not $app.WaitForExit(30000)){$app.Kill();$app.WaitForExit();$r.forced_cleanup=$true}}
 foreach($p in @($fx,$rec)){if($p -and -not $p.HasExited){$p.Kill();$p.WaitForExit()}}
 $r.owned_daemon_exited=@($owned|Where-Object{Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue}).Count -eq 0
 $r.process_reaped=(!$app -or $app.HasExited) -and (!$fx -or $fx.HasExited) -and (!$rec -or $rec.HasExited)
 $r.passed=$r.passed -and $r.process_reaped -and $r.owned_daemon_exited -and -not $r.forced_cleanup
 $r|ConvertTo-Json -Depth 12|Set-Content (Join-Path $Work 'result.json') -Encoding UTF8
 ([pscustomobject]$r)|Select-Object passed,error,consent_reset_verified,process_reaped|ConvertTo-Json
}
if(-not $r.passed){exit 1}
