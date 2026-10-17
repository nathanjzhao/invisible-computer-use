param([string]$Root='C:\ICUAudit\paired',[string]$Work='C:\Users\Public\icu-evidence\audio-ui',[string]$AppExe='C:\Program Files\ICU Gap4 Audio\app.exe')
$ErrorActionPreference='Stop'
if($env:ICU_DISPOSABLE_HOST -ne '1' -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){throw 'owned interactive test desktop required'}
if(Test-Path $Work){throw 'fresh evidence directory required'}
New-Item -Type Directory $Work|Out-Null
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path "$Root\NativeObserver.cs"
function Wait-For($predicate,[int]$seconds,[string]$why){$end=(Get-Date).AddSeconds($seconds);while((Get-Date) -lt $end){$v=& $predicate;if($v){return $v};Start-Sleep -Milliseconds 100};throw $why}
function Nodes{[Windows.Automation.AutomationElement]::FromHandle($script:window).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)}
function Find-Id($id){foreach($n in (Nodes)){if($n.Current.AutomationId -eq $id){return $n}}}
function Press($n){if(!$n){throw 'button absent'};$p=$null;if(!$n.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){throw ('invoke pattern absent: '+$n.Current.AutomationId+' '+$n.Current.ControlType.ProgrammaticName+' '+(($n.GetSupportedPatterns()|ForEach-Object{$_.ProgrammaticName}) -join ','))};$p.Invoke()}
function Toggle($id,$on){$n=Find-Id $id;$p=$null;if(!$n -or !$n.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$p)){throw "checkbox absent: $id"};if(($p.Current.ToggleState -eq [Windows.Automation.ToggleState]::On) -ne $on){$p.Toggle()}}
function Shot($path){$b=[Windows.Forms.SystemInformation]::VirtualScreen;$i=New-Object Drawing.Bitmap($b.Width,$b.Height);$g=[Drawing.Graphics]::FromImage($i);try{$g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size);$i.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$i.Dispose()}}
$r=[ordered]@{passed=$false;app_sha256=(Get-FileHash $AppExe).Hash};$app=$null;$player=$null;$script:window=[IntPtr]::Zero
try {
 $tone='C:\Users\Public\icu-evidence\audio-capture\tone.wav'
 if(!(Test-Path $tone)){throw 'synthetic tone prerequisite missing'}
 $player=New-Object System.Media.SoundPlayer($tone);$player.PlayLooping()
 $app=Start-Process $AppExe -PassThru -RedirectStandardOutput "$Work\app.out" -RedirectStandardError "$Work\app.err"
 $script:window=Wait-For {$h=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($h -ne [IntPtr]::Zero){$h}} 40 'app missing'
 [NativeObserver]::FocusGuardForSetup($script:window)
 Wait-For {@(Nodes|Where-Object{$_.Current.Name -eq 'driver connected'}).Count -gt 0} 25 'driver not ready'|Out-Null
 $audio=Wait-For {Find-Id 'audio-button'} 15 'audio controls missing'
 $expand=$null;if($audio.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$expand)){$expand.Expand()}else{Press $audio}
 if((Find-Id 'audio-start').Current.IsEnabled){throw 'recording admitted without sources or consent'}
 Toggle 'audio-system' $true
 if((Find-Id 'audio-start').Current.IsEnabled){throw 'recording admitted without consent'}
 Toggle 'audio-consent' $true;Press (Find-Id 'audio-start')
 Wait-For { @(Nodes|Where-Object{$_.Current.Name -like '*Recording audio*'}).Count -gt 0 } 5 'recording indicator missing'|Out-Null
 Start-Sleep -Milliseconds 5000;Shot "$Work\recording.png"
 Press (Find-Id 'audio-stop')
 $message=Wait-For {foreach($node in (Nodes)){if($node.Current.Name -like 'Saved audio in *'){return $node.Current.Name}}} 12 'audio was not saved'
 $path=$message.Substring('Saved audio in '.Length).TrimEnd('.')
 if($path.StartsWith('\\?\')){$path=$path.Substring(4)}
 $m=Get-Content (Join-Path $path 'manifest.json') -Raw|ConvertFrom-Json
 if(@($m.audio.sources).Count -ne 1 -or $m.audio.sources[0].name -ne 'system' -or $m.audio.sources[0].duration_sec -le 0){throw 'UI captured wrong or empty sources'}
 Copy-Item $path "$Work\recording" -Recurse
 $r.first_manifest=$m;$r.saved_message=$message
 if((Find-Id 'audio-start').Current.IsEnabled){throw 'recording consent was reused'}
 Shot "$Work\saved.png"
 Toggle 'audio-consent' $true;Press (Find-Id 'audio-start')
 $owned=Wait-For {$p=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id)"|Where-Object{$_.Name -eq 'taiku-observer.exe'});if($p.Count){$p}} 5 'audio helper did not start'
 Start-Sleep -Milliseconds 800
 [NativeObserver]::PostClose($script:window)|Out-Null
 if(!$app.WaitForExit(15000)){throw 'app exit did not finish audio cleanup'}
 Wait-For {@($owned|Where-Object{Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue}).Count -eq 0} 5 'audio helper survived app exit'|Out-Null
 $r.cleanup_verified=$true;$r.passed=$true
} catch {$r.error=$_.Exception.Message;$r.location=$_.InvocationInfo.PositionMessage;try{$r.chat=@(Nodes|ForEach-Object{$_.Current.Name});Shot "$Work\failure.png"}catch{}}
finally {if($app -and !$app.HasExited){[NativeObserver]::PostClose($script:window)|Out-Null;if(!$app.WaitForExit(15000)){$app.Kill();$app.WaitForExit()}};if($player){$player.Stop();$player.Dispose()};$r|ConvertTo-Json -Depth 12|Set-Content "$Work\result.json" -Encoding UTF8;$r|Select-Object passed,error,cleanup_verified|ConvertTo-Json}
if(!$r.passed){exit 1}
