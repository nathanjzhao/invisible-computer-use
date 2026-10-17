param([string]$Root='C:\ICUAudit\paired',[Parameter(Mandatory=$true)][string]$Work)
$ErrorActionPreference='Stop'
if($env:ICU_DISPOSABLE_HOST -ne '1' -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){throw 'disposable interactive host required'}
if(Test-Path $Work){throw 'fresh work directory required'}
New-Item -Type Directory $Work|Out-Null
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $Root 'NativeObserver.cs')
$exe=Join-Path $Root 'candidate\app.exe'
$r=[ordered]@{passed=$false;app_sha256=(Get-FileHash $exe).Hash.ToLowerInvariant();harness_sha256=(Get-FileHash $PSCommandPath).Hash.ToLowerInvariant()}
function Wait-For($pred,[int]$seconds,[string]$why){$end=(Get-Date).AddSeconds($seconds);while((Get-Date) -lt $end){$v=& $pred;if($v){return $v};Start-Sleep -Milliseconds 200};throw $why}
function Nodes { [Windows.Automation.AutomationElement]::FromHandle($script:window).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) }
function Find-Name([string]$name,[bool]$exact=$true){foreach($n in (Nodes)){try{if(-not $n.Current.IsOffscreen -and (($exact -and $n.Current.Name -eq $name) -or (-not $exact -and $n.Current.Name.Contains($name)))){return $n}}catch{}};return $null}
function Find-Id([string]$id){foreach($n in (Nodes)){try{if($n.Current.AutomationId -eq $id){return $n}}catch{}};return $null}
function Press($n){if(-not $n){throw 'button absent'};$p=$null;if($n.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){$p.Invoke();return};throw 'no invoke action'}
function Set-Value($n,[string]$value){$p=$null;if(-not $n.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$p)){throw 'no value pattern'};$p.SetValue($value);Wait-For {$p.Current.Value -eq $value} 5 'full value did not reach editor'|Out-Null}
function Shot([string]$file){$b=[Windows.Forms.SystemInformation]::VirtualScreen;$i=New-Object Drawing.Bitmap($b.Width,$b.Height);$g=[Drawing.Graphics]::FromImage($i);try{$g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size);$i.Save($file,[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$i.Dispose()}}

$app=$null;$script:window=[IntPtr]::Zero
try {
 $env:ICU_APP_MEMORY_ROOT=Join-Path $Work 'memory'
 $app=Start-Process $exe -PassThru
 $script:window=Wait-For {$h=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($h -ne [IntPtr]::Zero){$h}} 40 'app window missing'
 [NativeObserver]::SetWindowPos($script:window,[IntPtr]::Zero,0,0,1010,730,0x14)|Out-Null
 [NativeObserver]::FocusGuardForSetup($script:window)
 Press (Wait-For {Find-Id 'memory-chip'} 15 'memory panel unavailable')
 $details=Wait-For {Find-Name 'Learn from a recording'} 10 'recording controls unavailable'
 $details.SetFocus();[Windows.Forms.SendKeys]::SendWait('{ENTER}')
 $path=Wait-For {Find-Id 'trajectory-path'} 10 'recording path absent'
 Set-Value $path (Join-Path $Root 'review-recording')
 Press (Find-Id 'trajectory-review')
 Wait-For {Find-Name 'Line 9: Observed ui.focus' $false} 20 'actual preview events missing'|Out-Null
 $r.review_text=@(Nodes|ForEach-Object{try{$_.Current.Name}catch{}}|Where-Object{$_ -like 'Line *' -or $_ -like '*events will be imported*'})
 if(-not (($r.review_text -join "`n") -match 'Result')){throw 'actual control labels not reviewed'}
 $actor=Find-Id 'trajectory-actor';$actor.SetFocus();[Windows.Forms.SendKeys]::SendWait('{END}{ENTER}')
 $consent=Find-Id 'trajectory-consent';$toggle=$null
 if(-not $consent.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$toggle)){throw 'consent has no toggle pattern'}
 $toggle.Toggle();Wait-For {(Find-Id 'trajectory-import').Current.IsEnabled} 5 'consent did not enable import'|Out-Null
 Set-Value (Find-Id 'trajectory-project') 'changed-project'
 Wait-For {$toggle.Current.ToggleState -eq [Windows.Automation.ToggleState]::Off -and -not (Find-Id 'trajectory-import').Current.IsEnabled} 5 'edited scope kept stale consent'|Out-Null
 $r.consent_reset_verified=$true
 Shot (Join-Path $Work 'review.png')
 $r.passed=$true
} catch {$r.error=$_.Exception.Message;$r.error_location=$_.InvocationInfo.PositionMessage;try{$r.nodes=@(Nodes|ForEach-Object{@{name=$_.Current.Name;id=$_.Current.AutomationId;type=$_.Current.ControlType.ProgrammaticName}});Shot (Join-Path $Work 'failure.png')}catch{}}
finally {
 if($app -and -not $app.HasExited){[NativeObserver]::PostClose($script:window)|Out-Null;if(-not $app.WaitForExit(30000)){$app.Kill();$app.WaitForExit();$r.forced_cleanup=$true}}
 $r.process_reaped=(!$app -or $app.HasExited)
 $r.passed=$r.passed -and $r.process_reaped -and -not $r.forced_cleanup
 $r|ConvertTo-Json -Depth 12|Set-Content (Join-Path $Work 'result.json') -Encoding UTF8
 ([pscustomobject]$r)|Select-Object passed,error,consent_reset_verified,process_reaped|ConvertTo-Json
}
if(-not $r.passed){exit 1}
