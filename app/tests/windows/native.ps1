param([Parameter(Mandatory=$true)][string]$Driver,[Parameter(Mandatory=$true)][ValidateSet('text','scroll','errors','identity','notepad')][string]$Probe,[switch]$Mutate)
# Drive the installed driver exactly as the Windows app does: one serve
# process on a private pipe and one `cua-driver call` subprocess per request.
# -Mutate deliberately breaks the request so the oracle must reject the run;
# the run then exits 0 only if the rejection is the one the mutation causes.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
if ((New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Probe must run without administrator privileges' }
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'NativeObserver.cs')
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\native-'+$Probe+'-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$pipe='\\.\pipe\icu-native-'+[guid]::NewGuid().ToString('N')
$report=@{probe=$Probe;mutate=[bool]$Mutate;driver=$Driver;driver_sha256=(Get-FileHash $Driver -Algorithm SHA256).Hash;calls=@()}
$server=$null;$fixture=$null;$guard=$null
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
function Call([string]$tool,$arguments) {
    $json=$arguments | ConvertTo-Json -Depth 8 -Compress
    $info=New-Object Diagnostics.ProcessStartInfo $Driver
    $info.Arguments='--socket '+$pipe+' call '+$tool+' "'+($json -replace '"','\"')+'"'
    $info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.CreateNoWindow=$true
    $process=[Diagnostics.Process]::Start($info)
    $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEnd()
    $process.WaitForExit();$out=$stdout.Result
    $parsed=$null;try{$parsed=$out | ConvertFrom-Json}catch{}
    $row=@{tool=$tool;args=$arguments;exit=$process.ExitCode;stdout=$out.Trim();stderr=$stderr.Trim();json=($null -ne $parsed)}
    if($tool -ne 'get_window_state'){$script:report.calls+=$row}
    return @{row=$row;value=$parsed}
}
function Events {
    $path=Join-Path $work 'events.jsonl'
    if(-not (Test-Path $path)){return @()}
    return @(Get-Content $path -Encoding UTF8 | ForEach-Object {$_ | ConvertFrom-Json})
}
function Fixture([string[]]$arguments,[string]$ready) {
    $exe=Join-Path $work 'ICUProbeFixture.exe'
    $script:fixture=Start-Process $exe -ArgumentList $arguments -PassThru
    Wait-For {Test-Path (Join-Path $work $ready)} 10 'Fixture did not start' | Out-Null
    return Get-Content (Join-Path $work $ready) -Encoding UTF8 | ConvertFrom-Json
}
function Token($state,[string]$label) {
    foreach($element in @($state.elements)){ if($element.label -eq $label -or $element.title -eq $label -or $element.name -eq $label){return $element.element_token} }
    throw "No element labelled $label"
}
try {
    $csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    foreach($source in @('ProbeFixture','CalculatorFixture')){
        & $csc /nologo /codepage:65001 /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll ("/out:"+(Join-Path $work "ICU$source.exe")) (Join-Path $PSScriptRoot "$source.cs")
        if($LASTEXITCODE){throw "$source compilation failed"}
    }
    $env:ICU_NO_WINDOW_OBSERVE='1'
    $server=Start-Process $Driver -ArgumentList 'serve','--no-overlay','--socket',$pipe -WindowStyle Hidden -PassThru
    Wait-For {(Call 'list_windows' @{}).row.exit -eq 0} 15 'Driver did not serve' | Out-Null
    $report.calls=@()
    $session='icu-native-'+$Probe
    if($Probe -eq 'text'){$ready=Fixture @('--text',$work) 'text-ready.json'}
    elseif($Probe -eq 'scroll'){$ready=Fixture @('--scroll',$work) 'scroll-ready.json'}
    elseif($Probe -eq 'identity'){$ready=Fixture @('--identity',$work) 'identity-ready.json'}
    elseif($Probe -eq 'notepad'){
        # Classic Win32 Notepad (class Notepad on Server 2022), whose process
        # name the driver routes through UIA accelerators.
        $text=Join-Path $work 'lines.txt'
        (1..40 | ForEach-Object {"line $_ of the probe file"}) -join "`r`n" | Set-Content $text -Encoding ASCII
        $script:fixture=Start-Process notepad.exe -ArgumentList $text -PassThru
        $npWindow=Wait-For {$w=[NativeObserver]::FindTopLevel($fixture.Id,'Notepad',100);if($w -ne [IntPtr]::Zero){$w}} 10 'Notepad window did not appear'
        $ready=@{pid=$fixture.Id;window=$npWindow.ToInt64();class=[NativeObserver]::ClassOf($npWindow)}
        # Let Notepad finish activating itself, and keep it off the focus
        # guard (720,520), whose centre the guard setup clicks.
        Start-Sleep -Seconds 2
        if(-not [NativeObserver]::SetWindowPos($npWindow,[IntPtr]::Zero,40,40,600,400,0x14)){throw 'Could not place Notepad'}
    }
    else {$ready=Fixture @('--text',$work) 'text-ready.json'}
    $report.fixture=$ready
    $guard=Start-Process (Join-Path $work 'ICUCalculatorFixture.exe') -ArgumentList '--guard',$work -PassThru
    Wait-For {Test-Path (Join-Path $work 'guard-ready.json')} 10 'Focus guard did not start' | Out-Null
    $guardReady=Get-Content (Join-Path $work 'guard-ready.json') -Encoding UTF8 | ConvertFrom-Json
    [NativeObserver]::FocusGuardForSetup([IntPtr]$guardReady.window)
    Wait-For {([NativeObserver]::Read()).foreground -eq $guardReady.window} 3 'Focus guard did not activate' | Out-Null
    [NativeObserver]::EstablishRecentInput();Start-Sleep -Milliseconds 100
    $baseline=[NativeObserver]::Read();$report.baseline=$baseline
    $pid_=[int]$ready.pid
    function Input([string]$tool,$arguments){
        $arguments.pid=$pid_;$arguments.session=$session;$arguments._skip_window_change_detection=$true
        $result=Call $tool $arguments;Start-Sleep -Milliseconds 700;return $result
    }
    $failures=@()
    if($Probe -eq 'text'){
        $alpha=[long]$ready.alpha;$beta=[long]$ready.beta
        $stateA=(Call 'get_window_state' @{pid=$pid_;window_id=$alpha;include_screenshot=$false;include_accessibility_tree=$true;session=$session}).value
        $stateB=(Call 'get_window_state' @{pid=$pid_;window_id=$beta;include_screenshot=$false;include_accessibility_tree=$true;session=$session}).value
        $locked=[long]$ready.locked
        $stateL=(Call 'get_window_state' @{pid=$pid_;window_id=$locked;include_screenshot=$false;include_accessibility_tree=$true;session=$session}).value
        $report.observed=@{alpha=@($stateA.elements | ForEach-Object {$_.label});beta=@($stateB.elements | ForEach-Object {$_.label});locked=@($stateL.elements | ForEach-Object {$_.label})}
        [NativeObserver]::Start()
        $steps=@(
            @{name='bare type to Alpha';window='Alpha';tool='type_text';args=@{window_id=$alpha;text='|Alpha|'}},
            @{name='click Alpha field';window='Alpha';tool='click';args=@{window_id=$alpha;element_token=(Token $stateA 'Field Alpha')}},
            # -Mutate labels this Alpha request as Beta's: the oracle must then
            # report Alpha's change as a write into the wrong window.
            @{name='type to Alpha after click';window=$(if($Mutate){'Beta'}else{'Alpha'});tool='type_text';args=@{window_id=$alpha;text='|Alpha2|'}},
            # One UI thread has one focus: Beta must regain it before typing.
            @{name='click Beta field';window='Beta';tool='click';args=@{window_id=$beta;element_token=(Token $stateB 'Field Beta')}},
            @{name='positive control: type to Beta';window='Beta';tool='type_text';args=@{window_id=$beta;text='|Beta|'}},
            # Read-only: SetValue is refused, the WM_CHAR fallback is ignored,
            # and both readbacks succeed, so the driver sees the value unchanged.
            @{name='type to read-only Locked';window='Locked';tool='type_text';args=@{window_id=$locked;element_token=(Token $stateL 'Field Locked');text='|Locked|'}})
        $report.steps=@()
        foreach($step in $steps){
            $before=@(Events).Count
            $result=Input $step.tool $step.args
            $changed=@(Events | Select-Object -Skip $before | Where-Object {$_.kind -eq 'text'})
            $record=@{name=$step.name;requested=$step.window;reply=$result.row;changed=@($changed | ForEach-Object {@{window=$_.window;value=$_.value}})
                # A zero exit with no receiver change is reported, not hidden.
                silent_noop=($step.tool -eq 'type_text' -and $result.row.exit -eq 0 -and -not $changed.Count)}
            $report.steps+=$record
            $wrong=@($changed | Where-Object {$_.window -ne $step.window})
            if($wrong.Count){$failures+="$($step.name): text reached $($wrong[0].window)"}
            # delivery_failed claims the input did not take effect. It must not
            # ride on a reply whose text landed.
            $reason=if($result.value -and $result.value.escalation){$result.value.escalation.reason}
            $record.escalation_reason=$reason
            if($step.tool -eq 'type_text' -and $changed.Count -and $reason -eq 'delivery_failed'){$failures+="$($step.name): reply says delivery_failed but the text landed"}
            $effect=if($result.value){$result.value.effect};$record.effect=$effect
            # A readback that shows the field unchanged is evidence the text did
            # not land: the reply must say so and advise a foreground retry.
            if($step.window -eq 'Locked'){
                if($changed.Count){$failures+="$($step.name): the read-only field changed, so the fixture proves nothing"}
                elseif($effect -ne 'suspected_noop' -or $result.value.escalation.target -ne 'foreground'){$failures+="$($step.name): expected effect suspected_noop with a foreground escalation, got effect '$effect', escalation '$($result.value.escalation.target)/$reason'"}
            }
        }
        $final=@{}
        foreach($event in (Events | Where-Object {$_.kind -eq 'text'})){$final[$event.window]=$event.value}
        $report.final=$final
        if(-not ($final.Beta -like '*|Beta|*')){$failures+='positive control: Beta-targeted text did not reach Beta'}
        $report.alpha_received=[bool]($final.Alpha -like '*|Alpha*')
    } elseif($Probe -eq 'scroll'){
        $window=[long]$ready.window
        [NativeObserver]::Start()
        $report.steps=@()
        foreach($direction in @('down','up')){
            $sent=if($Mutate){@{down='up';up='down'}[$direction]}else{$direction}
            $before=@(Events).Count
            $result=Input 'scroll' @{window_id=$window;direction=$sent;by='line';amount=3}
            $after=@(Events | Select-Object -Skip $before)
            $tops=@($after | Where-Object {$_.kind -eq 'top'} | ForEach-Object {$_.top})
            $wheels=@($after | Where-Object {$_.kind -eq 'wheel'} | ForEach-Object {$_.delta})
            $start=@(Events | Select-Object -First $before | Where-Object {$_.kind -eq 'top'})[-1].top
            $route=if($result.value){$result.value.route}
            $report.steps+=@{expected=$direction;sent=$sent;reply=$result.row;route=$route;start=$start;tops=$tops;wheel_deltas=$wheels}
            # Posted WM_VSCROLL is a synthetic event to one window, not global input.
            if($route -ne 'synthetic_events'){$failures+="$direction scroll reported route '$route'"}
            if(-not $tops.Count){$failures+="$direction scroll did not move the list";continue}
            $moved=if($tops[-1] -gt $start){'down'}else{'up'}
            if($moved -ne $direction){$failures+="$direction scroll moved $moved"}
        }
    } elseif($Probe -eq 'identity'){
        # Cross-turn identity: two observations of one window around a relabel.
        # -Mutate also replaces Keep's AutomationId, so its stable id must change.
        $window=[long]$ready.window
        function Observe([string]$name){
            $r=Call 'get_window_state' @{pid=$pid_;window_id=$window;include_screenshot=$false;include_accessibility_tree=$true;session=$session}
            $r.row.stdout | Set-Content (Join-Path $work "$name.json") -Encoding UTF8
            return $r.value
        }
        function StableId($state,[string]$label){ @($state.elements | Where-Object {$_.label -eq $label})[0].stable_id }
        [NativeObserver]::Start()
        $first=Observe 'turn-1'
        Set-Content (Join-Path $work 'relabel.txt') $(if($Mutate){'rename-id'}else{'labels'})
        Wait-For {@(Events | Where-Object {$_.kind -eq 'relabel'}).Count} 5 'Fixture did not relabel' | Out-Null
        Start-Sleep -Milliseconds 300
        $second=Observe 'turn-2'
        $report.stable_ids=@{keep=@((StableId $first 'Keep'),(StableId $second 'Kept'));alpha=@((StableId $first 'Alpha'),(StableId $second 'Play'));still=@((StableId $first 'Still'),(StableId $second 'Still'))}
        $report.diff=$second.diff
        $keep=$report.stable_ids.keep;$still=$report.stable_ids.still
        if($null -eq $keep[0] -or $null -eq $keep[1]){$failures+='Keep/Kept has no stable id'}
        elseif($keep[0] -ne $keep[1]){$failures+="identified control changed stable id across a relabel: $($keep[0]) -> $($keep[1])"}
        if($null -eq $still[0] -or $still[0] -ne $still[1]){$failures+='unchanged control lost its stable id'}
    } elseif($Probe -eq 'notepad'){
        Add-Type @'
using System; using System.Runtime.InteropServices;
public static class NotepadEdit {
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent,IntPtr after,string cls,string title);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr w,uint m,IntPtr wp,IntPtr lp);
}
'@
        $window=[long]$ready.window
        $edit=[NotepadEdit]::FindWindowExW([IntPtr]$window,[IntPtr]::Zero,'Edit',$null)
        if($edit -eq [IntPtr]::Zero){throw 'Notepad edit control not found'}
        function EditState { @{length=[NotepadEdit]::SendMessageW($edit,0x0E,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64();sel=[NotepadEdit]::SendMessageW($edit,0xB0,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64()} }
        [NativeObserver]::Start()
        $report.steps=@()
        foreach($keys in @(@('ctrl','a'),@('shift','end'))){
            $before=EditState
            $result=Input 'hotkey' @{window_id=$window;keys=$keys}
            $after=EditState
            $code=if($result.value){$result.value.code}
            $report.steps+=@{keys=$keys;reply=$result.row;code=$code;before=$before;after=$after}
            # An undelivered chord is a typed refusal the app can parse.
            if(-not $result.row.json -or $code -ne 'background_unavailable'){$failures+="hotkey $($keys -join '+'): expected a JSON background_unavailable refusal, got exit $($result.row.exit): $($result.row.stdout)"}
            if($before.length -ne $after.length -or $before.sel -ne $after.sel){$failures+="hotkey $($keys -join '+') changed Notepad"}
        }
    } else {
        # Record exactly what the app's CLI route receives for native failures.
        $alpha=[long]$ready.alpha
        $stateA=(Call 'get_window_state' @{pid=$pid_;window_id=$alpha;include_screenshot=$false;include_accessibility_tree=$true;session=$session}).value
        [NativeObserver]::Start()
        $null=Input 'click' @{window_id=$alpha;element_token='not-a-token'}
        $null=Input 'set_value' @{window_id=$alpha;element_token=(Token $stateA 'Field Alpha');value=''}
        $null=Input 'press_key' @{window_id=$alpha;key='Ctrl+a'}
        $null=Input 'hotkey' @{window_id=$alpha;keys=@('cmd','r')}
        $null=Input 'click' @{window_id=123456;x=10;y=10}
        # SW_SHOWMINNOACTIVE: minimizing must not move foreground itself.
        [NativeObserver]::ShowWindow([IntPtr]$alpha,7) | Out-Null;Start-Sleep -Milliseconds 500
        $null=Input 'click' @{window_id=$alpha;element_token=(Token $stateA 'Field Alpha')}
        $report.events=@(Events)
    }
    $samples=[NativeObserver]::Stop();$report.sample_count=$samples.Count
    if([NativeObserver]::Error){$failures+='Ownership sampling failed: '+[NativeObserver]::Error}
    foreach($sample in $samples){foreach($field in @('foreground','focus','x','y','lastInput')){if($sample.$field -ne $baseline.$field){$failures+="Ownership changed: $field";break}}}
    $report.failures=@($failures | Select-Object -Unique)
    $expected=@{text='^type to Alpha after click: text reached Alpha$';scroll='^(down|up) scroll moved (up|down)$';identity='^identified control changed stable id across a relabel'}[$Probe]
    if($Mutate -and $expected){
        $report.expected_failure=$expected
        $matched=@($report.failures | Where-Object {$_ -match $expected})
        $report.control=if(-not $report.failures.Count){'did_not_fail'}elseif($matched.Count -eq $report.failures.Count){'failed_as_required'}else{'failed_for_another_reason'}
        $report.ok=($report.control -eq 'failed_as_required')
    } else { $report.ok=($report.failures.Count -eq 0) }
} catch {
    $report.ok=$false;$report.error=$_.Exception.Message
} finally {
    [NativeObserver]::Stop() | Out-Null
    try {
        $bounds=[Windows.Forms.SystemInformation]::VirtualScreen
        $image=New-Object Drawing.Bitmap($bounds.Width,$bounds.Height)
        $graphics=[Drawing.Graphics]::FromImage($image)
        $graphics.CopyFromScreen($bounds.Location,[Drawing.Point]::Empty,$bounds.Size)
        $image.Save((Join-Path $work 'desktop.png'),[Drawing.Imaging.ImageFormat]::Png)
        $graphics.Dispose();$image.Dispose()
    } catch {}
    foreach($process in @($fixture,$guard,$server)){if($process -and -not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    $report.work=$work
    $report | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $work 'result.json') -Encoding UTF8
    @{work=$work;ok=$report.ok;control=$report.control;error=$report.error;failures=$report.failures} | ConvertTo-Json | Set-Content (Join-Path $env:LOCALAPPDATA 'ICUAudit\latest.json') -Encoding UTF8
}
if(-not $report.ok){exit 1}
