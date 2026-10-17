param([Parameter(Mandatory=$true)][string]$Driver,[switch]$Mutate,[int[]]$Phase=@())
# Multi-monitor, mixed-DPI capture/input agreement through the driver's CLI
# route (the Windows app's route). Each click point comes from the driver's own
# screenshot; the fixture button's Click callback is the oracle.
# -Mutate moves every pixel aimed at a cell one cell sideways (right, or left
# from the right column), so a cell case must fire that neighbour instead:
# a wrong control, predicted exactly. The control passes only if every such
# case fires its neighbour and every other case still passes. A refusal, a
# miss onto nothing or any other failure is not that miss.
# Every case names the one outcome it accepts: the button that must fire, or
# the refusal code that must come back with nothing fired.
# -Phase runs only the listed phases (all by default); a partial run is for
# iteration and is never the acceptance result.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
if ((New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Probe must run without administrator privileges' }
Add-Type -Path (Join-Path $PSScriptRoot 'NativeObserver.cs')
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\displays-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$pipe='\\.\pipe\icu-displays-'+[guid]::NewGuid().ToString('N')
$report=@{tool_errors=@();probe='displays';mutate=[bool]$Mutate;phases=$(if($Phase.Count){$Phase}else{'all'});driver=$Driver;driver_sha256=(Get-FileHash $Driver -Algorithm SHA256).Hash;cases=@()}
$server=$null;$guard=$null;$fixtures=@()
$csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$refs=@('/reference:System.Windows.Forms.dll','/reference:System.Drawing.dll','/reference:System.Web.Extensions.dll')
function Want([int]$n) { return (-not $Phase.Count) -or ($n -in $Phase) }
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
# The layout tool occasionally fails once (a screen copy or a window read
# racing a repaint); anything it writes to stderr is kept in tool_errors, and
# three failures in a row stop the run. Output is unrolled for PS5 JSON arrays.
function Tool {
    for($try=1;$try -le 3;$try++){
        $ErrorActionPreference='Continue'
        $output=& $layout @args 2>&1;$code=$LASTEXITCODE
        $ErrorActionPreference='Stop'
        if(-not $code){ return ($output | Where-Object {$_ -is [string]}) | ConvertFrom-Json | ForEach-Object {$_} }
        $script:report.tool_errors+=@("try $try $args : $($output -join ' ')")
        Start-Sleep -Milliseconds 500
    }
    throw "layout tool failed: $args : $($output -join ' ')"
}
# One `cua-driver call`, started now and read by EndCall, so a case can have
# several requests in flight at once.
function StartCall([string]$tool,$arguments) {
    $json=$arguments | ConvertTo-Json -Depth 8 -Compress
    $info=New-Object Diagnostics.ProcessStartInfo $Driver
    $info.Arguments='--socket '+$pipe+' call '+$tool+' "'+($json -replace '"','\"')+'"'
    $info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.CreateNoWindow=$true
    $process=[Diagnostics.Process]::Start($info)
    return @{process=$process;stdout=$process.StandardOutput.ReadToEndAsync();stderr=$process.StandardError.ReadToEndAsync()}
}
function EndCall($started) {
    $started.process.WaitForExit();$out=$started.stdout.Result;$stderr=$started.stderr.Result
    $parsed=$null;try{$parsed=$out | ConvertFrom-Json}catch{}
    $code=$null;if($parsed){$code=$parsed.code;if(-not $code -and $parsed.refusal){$code=$parsed.refusal.code}}
    # Keep diagnostics bounded; screenshots are written to files.
    return @{exit=$started.process.ExitCode;stdout=$(if($out.Length -gt 1500){$out.Substring(0,1500)}else{$out.Trim()});stderr=$stderr.Trim();value=$parsed;code=$code;effect=$(if($parsed){$parsed.effect})}
}
function Call([string]$tool,$arguments) { return EndCall (StartCall $tool $arguments) }
function Events([string]$label) {
    $path=Join-Path $work "dpi-events-$label.jsonl"
    if(-not (Test-Path $path)){return @()}
    return @(Get-Content $path -Encoding UTF8 | ForEach-Object {$_ | ConvertFrom-Json})
}
function Launch([string]$mode,[string]$label,$monitor,[int]$offset=60,[int]$top=60) {
    $process=Start-Process (Join-Path $work 'ICUDpiFixture.exe') -ArgumentList $mode,$label,'100','100',$work -PassThru
    $script:fixtures+=$process
    Wait-For {Test-Path (Join-Path $work "dpi-ready-$label.json")} 10 "Fixture $label did not start" | Out-Null
    $ready=Get-Content (Join-Path $work "dpi-ready-$label.json") -Encoding UTF8 | ConvertFrom-Json
    $placed=Tool move $ready.window ($monitor.left+$offset) ($monitor.top+$top)
    Start-Sleep -Milliseconds 600
    return @{process=$process;ready=$ready;label=$label;mode=$mode;placed=(Tool window $ready.window)}
}
# A capture by the agent's session, or by $As (the app's tile is another session:
# an unbound pixel is read only against its own session's capture, c9-unify).
function Capture($fixture,[string]$name,[int]$maxDimension=640,[string]$As='') {
    $png=Join-Path $work "$name.png"
    $captureSession=$(if($As){$As}else{$session})
    $state=Call 'get_window_state' @{pid=[int]$fixture.ready.pid;window_id=[long]$fixture.ready.window;include_screenshot=$true;include_accessibility_tree=$true;max_dimension=$maxDimension;screenshot_out_file=$png;session=$captureSession}
    if($state.exit -ne 0 -or -not (Test-Path $png)){throw "capture $name failed: $($state.stderr) $($state.stdout)"}
    $colors=Tool colors $png
    $tokens=@{};foreach($element in @($state.value.elements)){if($element.label -match ' ([A-I])$'){$tokens[$Matches[1]]=$element.element_token}}
    return @{png=$png;colors=$colors;tokens=$tokens;screenshot_id=$state.value.screenshot_id;screenshot_width=$state.value.screenshot_width;screenshot_height=$state.value.screenshot_height;window=(Tool window $fixture.ready.window)}
}
# The cell -Mutate moves a pixel onto, and the move in screenshot pixels.
function Neighbor([string]$cell) { $k=[int][char]$cell-65; if($k % 3 -eq 2){$k--}else{$k++}; return [string][char](65+$k) }
function Shift($capture,[string]$cell) {
    if(-not $Mutate){return 0}
    $width=($capture.colors.buttons | Where-Object {$_.button -eq 'B'}).x-($capture.colors.buttons | Where-Object {$_.button -eq 'A'}).x
    if(([int][char]$cell-65) % 3 -eq 2){return -$width}
    return $width
}
function Click($fixture,$capture,[string]$button,[string]$via,[string]$case,[switch]$Bind,[string]$Id='',[string]$As='') {
    $target=@($capture.colors.buttons | Where-Object {$_.button -eq $button})[0]
    $arguments=@{pid=[int]$fixture.ready.pid;window_id=[long]$fixture.ready.window;session=$(if($As){$As}else{$session});_skip_window_change_detection=$true}
    if($via -eq 'pixel'){
        if($target.pixels -lt 50){throw "button $button not visible in $($capture.png)"}
        if($target.opaque -lt $target.pixels){$script:failures+="$case $($fixture.label): screenshot pixels of $button are transparent"}
        $arguments.x=[Math]::Round($target.x+(Shift $capture $button),1);$arguments.y=[Math]::Round($target.y,1)
        # Bind the pixel to the capture it came from, when the driver offers it,
        # or to another capture's id ($Id).
        if($Bind -and $capture.screenshot_id){$arguments.screenshot_id=$capture.screenshot_id}
        if($Id){$arguments.screenshot_id=$Id}
    } else { $arguments.element_token=$capture.tokens[$button] }
    $before=@(Events $fixture.label).Count
    $reply=Call 'click' $arguments
    Start-Sleep -Milliseconds 800
    $landed=@(Events $fixture.label | Select-Object -Skip $before)
    # A typed refusal is a non-zero exit or a refusal code in the reply, the
    # same shapes the app's input() treats as failure.
    $code=$null;if($reply.value){$code=$reply.value.code;if(-not $code -and $reply.value.refusal){$code=$reply.value.refusal.code}}
    $row=@{case=$case;fixture=$fixture.label;mode=$fixture.mode;via=$via;expected=$button;point=@($arguments.x,$arguments.y);
        screenshot_id=$arguments.screenshot_id;exit=$reply.exit;reply=$reply.stdout;stderr=$reply.stderr;landed=@($landed | ForEach-Object {$_.button});events=$landed;
        refused=($reply.exit -ne 0 -or [bool]$code);refusal_code=$code;effect=$(if($reply.value){$reply.value.effect});
        foreground=([NativeObserver]::Read()).foreground;window=(Tool window $fixture.ready.window)}
    $script:report.cases+=$row
    return $row
}
# $want is a button letter that must fire alone with no refusal, or
# 'refuse:<code>' where exactly that refusal code must come back, with
# `effect: refused`, and nothing may fire. Any other refusal code is a
# failure: it is not the guard under test. Under -Mutate a pixel case aimed at
# a cell wants the neighbour its pixel was moved onto. 'X|refuse:<code>'
# accepts either outcome (used only where the outcome depends on how Windows
# hosts the window, and never a silent miss).
function Require($row,[string]$want) {
    $landed=@($row.landed)
    if($Mutate -and $row.via -eq 'pixel' -and $want -match '^[A-I]'){$row.mutated_from=$want.Substring(0,1);$want=(Neighbor $want.Substring(0,1))+$want.Substring(1)}
    $row.want=$want
    $outcome="landed [$($landed -join ',')], exit $($row.exit), code '$($row.refusal_code)' effect '$($row.effect)'"
    foreach($alternative in $want.Split('|')){
        if($alternative -like 'refuse:*'){
            if($landed.Count -eq 0 -and $row.refused -and $row.refusal_code -eq $alternative.Substring(7) -and $row.effect -eq 'refused'){return}
        } elseif($landed.Count -eq 1 -and $landed[0] -eq $alternative -and -not $row.refused){return}
    }
    $script:failures+="$($row.case) $($row.fixture) $($row.via): expected $want, $outcome"
}
function PanelEvents([string]$label) {
    $path=Join-Path $work "panel-events-$label.jsonl"
    if(-not (Test-Path $path)){return @()}
    return @(Get-Content $path -Encoding UTF8 | ForEach-Object {$_ | ConvertFrom-Json})
}
$hitShapes=@('groupbox','transparent','halfhit','region','big','nested','gbchild','wfgroup')
function LaunchPanel([string]$mode,[string]$shape,[string]$label,$monitor,[int]$left,[int]$top) {
    $process=if($shape -eq 'wpf'){ Start-Process (Join-Path $work 'ICUWpfFixture.exe') -ArgumentList $mode,$label,'100','100',$work -PassThru }
        elseif($shape -in $hitShapes){ Start-Process (Join-Path $work 'ICUHitFixture.exe') -ArgumentList $mode,$shape,$label,'100','100',$work -PassThru }
        else { Start-Process (Join-Path $work 'ICUPanelFixture.exe') -ArgumentList $mode,$shape,$label,'100','100',$work -PassThru }
    $script:fixtures+=$process
    Wait-For {Test-Path (Join-Path $work "panel-ready-$label.json")} 10 "Panel fixture $label did not start" | Out-Null
    $ready=Get-Content (Join-Path $work "panel-ready-$label.json") -Encoding UTF8 | ConvertFrom-Json
    Tool move $ready.window ($monitor.left+$left) ($monitor.top+$top) | Out-Null
    return @{process=$process;ready=$ready;label=$label;mode=$mode;shape=$shape;monitor=$monitor.device}
}
# Posted-message gestures on surfaces with no UIA Invoke. Every event must be
# on the wanted cell with the wanted button; a double gesture needs two
# presses. A typed point_unmapped refusal is not accepted: these points are
# centroids of on-screen cells.
function Gesture($fixture,$capture,$gesture) {
    $cell=$gesture.cell
    $target=@($capture.colors.buttons | Where-Object {$_.button -eq $cell})[0]
    if($target.pixels -lt 50){throw "cell $cell not visible in $($capture.png)"}
    $arguments=@{pid=[int]$fixture.ready.pid;window_id=[long]$fixture.ready.window;session=$session;_skip_window_change_detection=$true;
        x=[Math]::Round($target.x+(Shift $capture $cell),1);y=[Math]::Round($target.y,1)}
    if($Mutate){$cell=Neighbor $cell}
    foreach($key in $gesture.args.Keys){$arguments[$key]=$gesture.args[$key]}
    $before=@(PanelEvents $fixture.label).Count
    $reply=Call $gesture.tool $arguments
    Start-Sleep -Milliseconds 800
    $events=@(PanelEvents $fixture.label | Select-Object -Skip $before)
    $code=$null;if($reply.value){$code=$reply.value.code;if(-not $code -and $reply.value.refusal){$code=$reply.value.refusal.code}}
    $row=@{case='posted';fixture=$fixture.label;mode=$fixture.mode;shape=$fixture.shape;monitor=$fixture.monitor;tool=$gesture.tool;extra=$gesture.args;
        expected=$cell;mutated_from=$(if($Mutate){$gesture.cell});want_button=$gesture.button;presses=$gesture.presses;point=@($arguments.x,$arguments.y);exit=$reply.exit;refusal_code=$code;reply=$reply.stdout;
        landed=@($events | ForEach-Object {"$($_.kind):$($_.button):$($_.cell)"})}
    $script:report.cases+=$row
    $good=@($events | Where-Object {$_.cell -eq $cell -and $_.button -eq $gesture.button})
    # A message posted to one window is not global input.
    $row.route=if($reply.value){$reply.value.route}
    if($row.route -eq 'global_input'){$script:failures+="posted $($fixture.label) $($gesture.tool): reply labels a posted message route 'global_input'"}
    if($reply.exit -ne 0 -or $code -or $events.Count -ne $good.Count -or $good.Count -lt $gesture.presses){
        $script:failures+="posted $($fixture.label) $($gesture.tool) $(($gesture.args.Keys | ForEach-Object {"$_=$($gesture.args[$_])"}) -join ' '): expected $($gesture.presses)x $($gesture.button) on $cell, got [$($row.landed -join ',')], exit $($reply.exit), code '$code'"
    }
}
# One pixel action on a panel-family fixture, as a row Require can judge.
# $point overrides the cell's centroid; $Id binds to another capture's id.
function PanelClick($fixture,$capture,[string]$cell,[string]$case,[switch]$Bind,[string]$Id='',[string]$tool='click',$extra=@{},$point=$null) {
    $arguments=@{pid=[int]$fixture.ready.pid;window_id=[long]$fixture.ready.window;session=$session;_skip_window_change_detection=$true}
    if($point){ $arguments.x=$point[0];$arguments.y=$point[1] } else {
        $target=@($capture.colors.buttons | Where-Object {$_.button -eq $cell})[0]
        if($target.pixels -lt 50){throw "cell $cell not visible in $($capture.png)"}
        $arguments.x=[Math]::Round($target.x+(Shift $capture $cell),1);$arguments.y=[Math]::Round($target.y,1)
    }
    if($Bind -and $capture.screenshot_id){$arguments.screenshot_id=$capture.screenshot_id}
    if($Id){$arguments.screenshot_id=$Id}
    # A drag's pixel is its start point.
    if($tool -eq 'drag'){$arguments.from_x=$arguments.x;$arguments.from_y=$arguments.y;$arguments.Remove('x');$arguments.Remove('y')}
    foreach($key in $extra.Keys){$arguments[$key]=$extra[$key]}
    $before=@(PanelEvents $fixture.label).Count
    $reply=Call $tool $arguments
    Start-Sleep -Milliseconds 800
    $events=@(PanelEvents $fixture.label | Select-Object -Skip $before)
    $code=$null;if($reply.value){$code=$reply.value.code;if(-not $code -and $reply.value.refusal){$code=$reply.value.refusal.code}}
    $row=@{case=$case;fixture=$fixture.label;mode=$fixture.mode;shape=$fixture.shape;via=$(if($point){'point'}else{'pixel'});tool=$tool;expected=$cell;
        point=$(if($tool -eq 'drag'){@($arguments.from_x,$arguments.from_y)}else{@($arguments.x,$arguments.y)});screenshot_id=$arguments.screenshot_id;extra=$extra;exit=$reply.exit;reply=$reply.stdout;stderr=$reply.stderr;
        landed=@($events | Where-Object {$_.kind -in @('down','double')} | ForEach-Object {$_.cell} | Select-Object -Unique);events=$events;
        refused=($reply.exit -ne 0 -or [bool]$code);refusal_code=$code;effect=$(if($reply.value){$reply.value.effect})}
    $script:report.cases+=$row
    return $row
}
# A foreground wheel at a list's pixel. The row's `landed` names the list
# whose top item moved; `left`/`right` are the lists' top items after it.
function ListScroll($fixture,$capture,[string]$cell,[string]$case,[switch]$Bind) {
    $target=@($capture.colors.buttons | Where-Object {$_.button -eq $cell})[0]
    if($target.pixels -lt 50){throw "list $cell not visible in $($capture.png)"}
    $arguments=@{pid=[int]$fixture.ready.pid;window_id=[long]$fixture.ready.window;session=$session;_skip_window_change_detection=$true;
        direction='down';amount=5;delivery_mode='foreground';x=[Math]::Round($target.x,1);y=[Math]::Round($target.y,1)}
    if($Bind -and $capture.screenshot_id){$arguments.screenshot_id=$capture.screenshot_id}
    $last={ $all=@(PanelEvents $fixture.label); if($all.Count){$all[-1]}else{@{left=0;right=0}} }
    $before=& $last
    $reply=Call 'scroll' $arguments
    Start-Sleep -Milliseconds 1200
    $after=& $last
    $code=$null;if($reply.value){$code=$reply.value.code;if(-not $code -and $reply.value.refusal){$code=$reply.value.refusal.code}}
    $landed=@();if($after.left -ne $before.left){$landed+='left'};if($after.right -ne $before.right){$landed+='right'}
    $row=@{case=$case;fixture=$fixture.label;mode=$fixture.mode;via='scroll';expected=$cell;point=@($arguments.x,$arguments.y);screenshot_id=$arguments.screenshot_id;
        exit=$reply.exit;reply=$reply.stdout;stderr=$reply.stderr;landed=$landed;left=$after.left;right=$after.right;
        refused=($reply.exit -ne 0 -or [bool]$code);refusal_code=$code;effect=$(if($reply.value){$reply.value.effect})}
    $script:report.cases+=$row
    return $row
}
function Sampling-Start {
    [NativeObserver]::FocusGuardForSetup([IntPtr]$guardReady.window)
    Wait-For {([NativeObserver]::Read()).foreground -eq $guardReady.window} 3 'Focus guard did not activate' | Out-Null
    [NativeObserver]::EstablishRecentInput();Start-Sleep -Milliseconds 100
    $script:baseline=[NativeObserver]::Read();$script:report.baselines+=$script:baseline
    [NativeObserver]::Start()
}
function Sampling-Stop {
    $samples=[NativeObserver]::Stop();$script:report.sample_count+=$samples.Count
    if([NativeObserver]::Error){$script:failures+='Ownership sampling failed: '+[NativeObserver]::Error}
    foreach($sample in $samples){foreach($field in @('foreground','focus','x','y','lastInput')){if($sample.$field -ne $script:baseline.$field){$script:failures+="Ownership changed: $field";break}}}
}
try {
    $layout=Join-Path $work 'ICUDisplayLayout.exe'
    & $csc /nologo /target:exe /main:DisplayLayout @refs ("/out:"+$layout) (Join-Path $PSScriptRoot 'DisplayLayout.cs') (Join-Path $PSScriptRoot 'DpiFixture.cs')
    if($LASTEXITCODE){throw 'layout tool compilation failed'}
    & $csc /nologo /target:winexe @refs ("/out:"+(Join-Path $work 'ICUDpiFixture.exe')) (Join-Path $PSScriptRoot 'DpiFixture.cs')
    if($LASTEXITCODE){throw 'DPI fixture compilation failed'}
    & $csc /nologo /target:winexe @refs ("/out:"+(Join-Path $work 'ICUPanelFixture.exe')) (Join-Path $PSScriptRoot 'PanelFixture.cs')
    if($LASTEXITCODE){throw 'Panel fixture compilation failed'}
    $wpf='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\WPF'
    & $csc /nologo /target:winexe @refs /reference:System.Xaml.dll ("/reference:$wpf\PresentationFramework.dll") ("/reference:$wpf\PresentationCore.dll") ("/reference:$wpf\WindowsBase.dll") ("/reference:$wpf\WindowsFormsIntegration.dll") ("/out:"+(Join-Path $work 'ICUWpfFixture.exe')) (Join-Path $PSScriptRoot 'WpfFixture.cs')
    if($LASTEXITCODE){throw 'WPF fixture compilation failed'}
    & $csc /nologo /target:winexe @refs ("/out:"+(Join-Path $work 'ICUHitFixture.exe')) (Join-Path $PSScriptRoot 'HitFixture.cs')
    if($LASTEXITCODE){throw 'hit fixture compilation failed'}
    & $csc /nologo /target:winexe ("/out:"+(Join-Path $work 'ICUPenDevice.exe')) (Join-Path $PSScriptRoot 'PenDevice.cs')
    if($LASTEXITCODE){throw 'pen device helper compilation failed'}
    & $csc /nologo /codepage:65001 /target:winexe @refs ("/out:"+(Join-Path $work 'ICUCalculatorFixture.exe')) (Join-Path $PSScriptRoot 'CalculatorFixture.cs')
    if($LASTEXITCODE){throw 'guard compilation failed'}
    $monitors=@(Tool list);$report.monitors=$monitors
    if($monitors.Count -lt 2){throw 'Multiple monitors required'}
    $env:ICU_NO_WINDOW_OBSERVE='1'
    $server=Start-Process $Driver -ArgumentList 'serve','--no-overlay','--socket',$pipe -WindowStyle Hidden -PassThru
    Wait-For {(Call 'list_windows' @{}).exit -eq 0} 15 'Driver did not serve' | Out-Null
    $session='icu-displays'
    # One tile session per phase: an idle named session ends after 5 minutes.
    $tileSession='icu-displays-tile'
    $screen=Call 'get_screen_size' @{};$report.screen_size=$screen.value
    $desktopPng=Join-Path $work 'driver-desktop.png'
    $desktop=Call 'get_desktop_state' @{screenshot_out_file=$desktopPng};$report.desktop_state=$desktop.value
    $report.virtual_desktop=Tool shot (Join-Path $work 'virtual-desktop-before.png')
    $guard=Start-Process (Join-Path $work 'ICUCalculatorFixture.exe') -ArgumentList '--guard',$work -PassThru
    Wait-For {Test-Path (Join-Path $work 'guard-ready.json')} 10 'Focus guard did not start' | Out-Null
    $guardReady=Get-Content (Join-Path $work 'guard-ready.json') -Encoding UTF8 | ConvertFrom-Json
    $failures=@();$report.baselines=@();$report.sample_count=0;$report.captures=@{}
    $primary=@($monitors | Where-Object {$_.primary})[0]
    $far=@($monitors | Where-Object {-not $_.primary} | Sort-Object {[Math]::Min($_.left,$_.top)})[0]
    $scaled=@($monitors | Where-Object {$_.scale_percent -gt 100} | Sort-Object scale_percent -Descending)
    if(-not $scaled.Count){throw 'A scaled monitor is required'}
    $highest=$scaled[0]

    if(Want 1){
        # Phase 1. Fixtures on every monitor in both DPI modes: capture, then click
        # the screenshot centroid of E and of C, and G by element token.
        $placed=@()
        for($i=0;$i -lt $monitors.Count;$i++){
            $placed+=Launch 'pmv2' "pmv2-m$i" $monitors[$i] 60
            $placed+=Launch 'unaware' "unaware-m$i" $monitors[$i] 460
        }
        # Lifecycle fixtures start before sampling: a new window activates itself.
        $life=@{pmv2=(Launch 'pmv2' 'pmv2-life' $primary 60 400);unaware=(Launch 'unaware' 'unaware-life' $primary 460 400)}
        # Never captured by this driver, then minimized: only the minimized guard
        # can refuse a pixel click here, because there is no capture to be stale.
        $fresh=@{pmv2=(Launch 'pmv2' 'pmv2-uncaptured' $primary 560 60);unaware=(Launch 'unaware' 'unaware-uncaptured' $primary 660 60)}
        foreach($f in $fresh.Values){[NativeObserver]::ShowWindow([IntPtr]$f.ready.window,7) | Out-Null}
        Start-Sleep -Milliseconds 800
        $report.virtual_desktop_fixtures=Tool shot (Join-Path $work 'virtual-desktop-fixtures.png')
        Sampling-Start
        foreach($fixture in $placed){
            $capture=Capture $fixture $fixture.label
            $report.captures[$fixture.label]=@{colors=$capture.colors;window=$capture.window;tokens=@($capture.tokens.Keys);screenshot_id=$capture.screenshot_id}
            foreach($button in @('E','C')){ Require (Click $fixture $capture $button 'pixel' 'placed') $button }
            Require (Click $fixture $capture 'G' 'token' 'placed') 'G'
        }
        # Lifecycle on the most distant non-primary monitor: move between capture
        # and click, resize, minimize and close. A PMv2 fixture keeps its size
        # across monitors (it ignores WM_DPICHANGED), so its old pixel stays valid.
        foreach($mode in @('pmv2','unaware')){
            $f=$life[$mode]
            $capture=Capture $f "$mode-life-primary"
            Tool move $f.ready.window ($far.left+80) ($far.top+80) | Out-Null;Start-Sleep -Milliseconds 800
            Require (Click $f $capture 'I' 'pixel' 'moved-after-capture') $(if($mode -eq 'pmv2'){'I'}else{'refuse:stale_capture'})
            $capture=Capture $f "$mode-life-far"
            # The session's latest capture is this one, at the window's current
            # size, so its pixel lands (c9-unify; the unaware window, captured at
            # two sizes, used to refuse ambiguous_capture).
            Require (Click $f $capture 'I' 'pixel' 'recaptured-after-move') 'I'
            Require (Click $f $capture 'I' 'pixel' 'recaptured-after-move-bound' -Bind) 'I'
            $w=$capture.window
            Tool move $f.ready.window $w.left $w.top ($w.right-$w.left+240) ($w.bottom-$w.top+160) | Out-Null;Start-Sleep -Milliseconds 800
            Require (Click $f $capture 'I' 'pixel' 'resized-after-capture') 'refuse:stale_capture'
            [NativeObserver]::ShowWindow([IntPtr]$f.ready.window,7) | Out-Null;Start-Sleep -Milliseconds 800
            Require (Click $f $capture 'I' 'pixel' 'minimized') 'refuse:window_minimized'
            Require (Click $f $capture 'I' 'token' 'minimized-token') 'refuse:window_minimized'
            $f.process.Kill();$f.process.WaitForExit();Start-Sleep -Milliseconds 500
            Require (Click $f $capture 'I' 'pixel' 'closed') 'refuse:window_target_not_found'
        }
        # The pixel is the centre of E in a 100% capture of this fixture; nothing
        # was captured, so no stale-capture record exists for the window.
        foreach($mode in @('pmv2','unaware')){
            $f=$fresh[$mode]
            $synthetic=@{png='(none)';colors=@{buttons=@(@{button='E';pixels=6000;opaque=6000;x=169.0;y=127.0},@{button='A';x=64.0},@{button='B';x=169.0})};tokens=@{}}
            Require (Click $f $synthetic 'E' 'pixel' 'minimized-never-captured') 'refuse:window_minimized'
        }
        Sampling-Stop
        foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    }

    if(Want 2){
        # Phase 2. Posted-message routes: fixtures with no UIA Invoke, as one
        # painted HWND (canvas) or one child HWND per cell (panels), in both DPI
        # modes on every monitor.
        $panels=@()
        foreach($monitor in $monitors){
            $slot=0
            foreach($mode in @('pmv2','unaware')){ foreach($shape in @('canvas','panels')){
                $label="$shape-$mode-$($monitor.device -replace '\\\\\.\\','')"
                $panels+=LaunchPanel $mode $shape $label $monitor (40+($slot%2)*540) (40+[Math]::Floor($slot/2)*400)
                $slot++
            }}
        }
        Start-Sleep -Milliseconds 800
        $report.virtual_desktop_panels=Tool shot (Join-Path $work 'virtual-desktop-panels.png')
        Sampling-Start
        $gestures=@(
            @{tool='click';cell='E';args=@{};button='Left';presses=1},
            @{tool='right_click';cell='C';args=@{};button='Right';presses=1},
            @{tool='double_click';cell='G';args=@{};button='Left';presses=2},
            @{tool='click';cell='I';args=@{button='right'};button='Right';presses=1},
            @{tool='click';cell='A';args=@{count=2};button='Left';presses=2})
        foreach($fixture in $panels){
            $capture=Capture $fixture $fixture.label
            $report.captures[$fixture.label]=@{colors=$capture.colors;window=$capture.window;screenshot_id=$capture.screenshot_id}
            foreach($gesture in $gestures){ Gesture $fixture $capture $gesture }
            # The caption is outside every client area. With only the hit test to
            # go on, a posted click there reached the form's client area with
            # exit 0 (review C19).
            Require (PanelClick $fixture $capture '' 'caption' -point @([Math]::Round($capture.colors.width/4,1),4)) 'refuse:point_unmapped'
        }
        Sampling-Stop
        foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    }

    if(Want 3){
        $tileSession='icu-displays-tile-3'
        # Phase 3. Screenshot-id binding: a later capture of the same window, e.g.
        # the app's live tile, must not make an older capture's pixels current.
        $bind=@{moved=(Launch 'unaware' 'bind-moved' $primary 60 60);ratio=(Launch 'pmv2' 'bind-ratio' $primary 460 60);
            drag=(LaunchPanel 'unaware' 'panels' 'bind-drag' $primary 60 440)}
        Start-Sleep -Milliseconds 800
        Sampling-Start
        # The agent captures, the window moves to the most scaled monitor, the
        # tile captures it at the new size, then the agent clicks its own pixel
        # of I with its own screenshot_id.
        $f=$bind.moved
        $agent=Capture $f 'bind-moved-agent'
        Tool move $f.ready.window ($highest.left+80) ($highest.top+80) | Out-Null;Start-Sleep -Milliseconds 800
        $tile=Capture $f 'bind-moved-tile' 640 $tileSession
        $report.bind=@{agent_id=$agent.screenshot_id;tile_id=$tile.screenshot_id;agent_window=$agent.window;tile_window=$tile.window}
        Require (Click $f $agent 'I' 'pixel' 'bound-after-tile-capture' -Bind) 'refuse:stale_capture'
        Require (Click $f $tile 'I' 'pixel' 'bound-to-tile-capture' -Bind) 'I'
        # Without an id the agent's pixel is read against the agent's own
        # capture, and the window has changed size since: stale, whatever the
        # tile captured after (c9-unify).
        Require (Click $f $agent 'I' 'pixel' 'unbound-after-tile-capture') 'refuse:stale_capture'
        $movedTile=$tile
        # The agent captures at full size, a tile then captures the same window
        # downscaled; the agent's bound pixel must use its own capture's scale.
        $f=$bind.ratio
        $agent=Capture $f 'bind-ratio-agent'
        $tile=Capture $f 'bind-ratio-tile' 200 $tileSession
        $report.bind.ratio=@{agent=@($agent.colors.width,$agent.colors.height);tile=@($tile.colors.width,$tile.colors.height)}
        Require (Click $f $agent 'E' 'pixel' 'bound-after-downscaled-capture' -Bind) 'E'
        # The window did not change at all, yet without an id the agent's pixel
        # was scaled by the tile's capture and landed on I (review C9). It is
        # read against the agent's own capture now, and the tile's pixel against
        # the tile's; a session that captured nothing refuses (c9-unify).
        Require (Click $f $agent 'E' 'pixel' 'unbound-after-downscaled-capture') 'E'
        Require (Click $f $tile 'E' 'pixel' 'tile-session-unbound' -As $tileSession) 'E'
        Require (Click $f $agent 'E' 'pixel' 'stranger-session-unbound' -As 'icu-displays-stranger') 'refuse:ambiguous_capture'
        # An id from another window is refused like every other bad id, with
        # effect: refused (review N4).
        Require (Click $f $agent 'E' 'pixel' 'id-of-another-window' -Id $movedTile.screenshot_id) 'refuse:invalid_arguments'
        # Drag endpoints read from two captures: each is bound to its own. The
        # start comes from the pre-move capture, so it is stale (review N5).
        $f=$bind.drag
        $agent=Capture $f 'bind-drag-agent'
        Tool move $f.ready.window ($highest.left+620) ($highest.top+80) | Out-Null;Start-Sleep -Milliseconds 800
        $tile=Capture $f 'bind-drag-tile' 640 $tileSession
        $dragTo=@($tile.colors.buttons | Where-Object {$_.button -eq 'G'})[0]
        $to=@{to_x=[Math]::Round($dragTo.x,1);to_y=[Math]::Round($dragTo.y,1)}
        # The review's shape: the call's screenshot_id is the fresh capture the
        # end was read from; only from_screenshot_id says the start is older.
        Require (PanelClick $f $agent 'I' 'drag-stale-start' -Id $tile.screenshot_id -tool 'drag' -extra ($to+@{from_screenshot_id=$agent.screenshot_id})) 'refuse:stale_capture'
        Require (PanelClick $f $tile 'I' 'drag-bound-both' -tool 'drag' -extra ($to+@{from_screenshot_id=$tile.screenshot_id;to_screenshot_id=$tile.screenshot_id})) 'I'
        Require (PanelClick $f $tile 'I' 'drag-bound-one-id' -Bind -tool 'drag' -extra $to) 'I'
        Sampling-Stop
        foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    }

    if(Want 4){
        $tileSession='icu-displays-tile-4'
        # Phase 4. A foreground wheel at a list's pixel is bound like a click
        # (review N1: a point from a 200 px capture scrolled the other list). No
        # ownership sampling: a foreground wheel moves the pointer by design.
        foreach($mode in @('pmv2','unaware')){
            $f=LaunchPanel $mode 'lists' "lists-$mode" $highest $(if($mode -eq 'pmv2'){40}else{600}) 440
            Start-Sleep -Milliseconds 800
            $small=Capture $f "lists-$mode-200" 200
            Require (ListScroll $f $small 'C' 'scroll-bound-200' -Bind) 'right'
            $large=Capture $f "lists-$mode-640" 640 $tileSession
            Require (ListScroll $f $large 'C' 'scroll-bound-640' -Bind) 'right'
            # the agent's session captured only at 200, so its pixel is read at 200
            Require (ListScroll $f $small 'C' 'scroll-unbound-after-two-scales') 'right'
        }
        foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    }

    if(Want 5){
        $tileSession='icu-displays-tile-5'
        # Phase 5. The window is destroyed and its HWND handed to a new window
        # painted in reverse; the old capture's pixels must not be applied to it
        # (review N2: the old id landed on G with exit 0).
        $reuse=LaunchPanel 'pmv2' 'reuse' 'reuse' $primary 60 60
        Start-Sleep -Milliseconds 800
        $before=Capture $reuse 'reuse-before'
        New-Item -ItemType File (Join-Path $work 'panel-reuse-go-reuse') | Out-Null
        Wait-For {Test-Path (Join-Path $work 'panel-reuse-done-reuse.json')} 150 'reuse fixture did not finish' | Out-Null
        Start-Sleep -Milliseconds 800
        $report.reuse=Get-Content (Join-Path $work 'panel-reuse-done-reuse.json') -Encoding UTF8 | ConvertFrom-Json
        if($report.reuse.found -ne $reuse.ready.window){throw "reuse fixture did not get HWND $($reuse.ready.window) back"}
        Sampling-Start
        Require (PanelClick $reuse $before 'C' 'reused-hwnd-bound' -Bind) 'refuse:stale_capture'
        Require (PanelClick $reuse $before 'C' 'reused-hwnd-unbound') 'refuse:stale_capture'
        # A tile then captures the new window. The agent's pixel, read from the
        # old one, clicked G with exit 0 (review R3-5). The agent's own frame
        # carries the old window's identity, so it stays stale (c9-unify).
        $tile=Capture $reuse 'reuse-tile-of-new-window' 640 $tileSession
        Require (PanelClick $reuse $before 'C' 'reused-hwnd-bound-after-tile' -Bind) 'refuse:stale_capture'
        Require (PanelClick $reuse $before 'C' 'reused-hwnd-unbound-after-tile') 'refuse:stale_capture'
        Require (PanelClick $reuse $tile 'C' 'reused-hwnd-bound-to-tile' -Bind) 'C'
        Sampling-Stop
    }

    if(Want 6){
        # Phase 6. Mixed-DPI hosting: a form of one awareness hosting a child
        # container of the other, one child HWND per cell. The hit test runs in
        # the root's space, so for the container's cells it can pick a window
        # other than the one drawn at the pixel. Each posted gesture must reach
        # the cell drawn there or be refused point_unmapped: never a click
        # posted to another window with exit 0.
        $hosted=@()
        foreach($monitor in $monitors){
            $slot=0
            foreach($mode in @('pmv2','unaware')){
                $label="hosted-$mode-$($monitor.device -replace '\\\\\.\\','')"
                $hosted+=LaunchPanel $mode 'hosted' $label $monitor (20+$slot*560) 40
                $slot++
            }
        }
        Start-Sleep -Milliseconds 800
        $report.virtual_desktop_hosted=Tool shot (Join-Path $work 'virtual-desktop-hosted.png')
        Sampling-Start
        foreach($fixture in $hosted){
            $capture=Capture $fixture $fixture.label
            $report.captures[$fixture.label]=@{colors=$capture.colors;window=$capture.window;screenshot_id=$capture.screenshot_id}
            foreach($cell in @('A','B','C','D','E','F','G','H','I')){
                Require (PanelClick $fixture $capture $cell 'hosted-click' -Bind) "$cell|refuse:point_unmapped"
                Require (PanelClick $fixture $capture $cell 'hosted-right' -Bind -tool 'right_click') "$cell|refuse:point_unmapped"
            }
        }
        Sampling-Stop
        foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    }

    if(Want 7){
        # Phase 7. WPF: the grid is drawn in one HWND with no child windows
        # and no UIA Invoke. Review N3: pixel clicks here returned exit 0 and
        # nothing reached WPF. Each must reach the drawn cell or be refused.
        $wpf=@()
        foreach($monitor in @($primary,$highest)){
            $slot=0
            foreach($mode in @('default','pmv2')){
                $wpf+=LaunchPanel $mode 'wpf' "wpf-$mode-$($monitor.device -replace '\\\\\.\\','')" $monitor (40+$slot*600) 40
                $slot++
            }
        }
        Start-Sleep -Milliseconds 1500
        $report.virtual_desktop_wpf=Tool shot (Join-Path $work 'virtual-desktop-wpf.png')
        Sampling-Start
        foreach($fixture in $wpf){
            $capture=Capture $fixture $fixture.label
            $report.captures[$fixture.label]=@{colors=$capture.colors;window=$capture.window;screenshot_id=$capture.screenshot_id;ready=$fixture.ready}
            foreach($cell in @('A','E','I')){ Require (PanelClick $fixture $capture $cell 'wpf-click' -Bind) "$cell|refuse:background_unavailable" }
            Require (PanelClick $fixture $capture 'C' 'wpf-right' -Bind -tool 'right_click') 'C|refuse:background_unavailable'
            Require (PanelClick $fixture $capture 'G' 'wpf-double' -Bind -tool 'double_click') 'G|refuse:background_unavailable'
        }
        Sampling-Stop
        foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    }

    if(Want 8){
        # Phase 8. The window real input reaches at a pixel (review R3-3), on
        # raw Win32 shapes where the previous hit test's rules were wrong: a
        # group box raised above its cells (it answers HTTRANSPARENT), an
        # opaque WS_EX_TRANSPARENT child J over E, a child J over E that is
        # HTTRANSPARENT on its left half only, and E with an elliptic window
        # region. Every posted gesture must reach that window; no refusal is
        # accepted. J's centre is E's centre, and E's pixels ring it.
        # The final review's shapes (D1): an opaque J inside a plain static,
        # inside a group box, and inside a WinForms FlatStyle.System GroupBox,
        # each over E's middle. Real input reaches J; beside J, inside the
        # container, it passes through the container to E. A walk that skipped
        # a transparent container with its children sent all of them to E.
        foreach($monitor in @($primary,$highest)){ foreach($mode in @('pmv2','unaware')){ foreach($shapes in @(@('groupbox','transparent','halfhit','region'),@('nested','gbchild','wfgroup'))){
            $hit=@();$slot=0
            $halfWidth=[Math]::Floor(($monitor.right-$monitor.left)/2);$halfHeight=[Math]::Floor(($monitor.bottom-$monitor.top)/2)
            foreach($shape in $shapes){
                $hit+=LaunchPanel $mode $shape "$shape-$mode-$($monitor.device -replace '\\\\\.\\','')" $monitor (20+($slot%2)*$halfWidth) (20+[Math]::Floor($slot/2)*$halfHeight)
                $slot++
            }
            Start-Sleep -Milliseconds 800
            Sampling-Start
            foreach($fixture in $hit){
                $capture=Capture $fixture $fixture.label
                $report.captures[$fixture.label]=@{colors=$capture.colors;window=$capture.window;screenshot_id=$capture.screenshot_id}
                $e=@($capture.colors.buttons | Where-Object {$_.button -eq 'E'})[0]
                $ey=[Math]::Round($e.y,1);$ew=$e.maxx-$e.minx
                if($fixture.shape -eq 'groupbox'){
                    Require (PanelClick $fixture $capture 'E' 'groupbox-click' -Bind) 'E'
                    Require (PanelClick $fixture $capture 'C' 'groupbox-right' -Bind -tool 'right_click') 'C'
                    Require (PanelClick $fixture $capture 'G' 'groupbox-double' -Bind -tool 'double_click') 'G'
                } elseif($fixture.shape -eq 'transparent'){
                    Require (PanelClick $fixture $capture '' 'transparent-click-J' -Bind -point @([Math]::Round($e.x,1),$ey)) 'J'
                    Require (PanelClick $fixture $capture '' 'transparent-right-J' -Bind -tool 'right_click' -point @([Math]::Round($e.x,1),$ey)) 'J'
                    Require (PanelClick $fixture $capture '' 'transparent-click-E-beside-J' -Bind -point @(($e.minx+4),$ey)) 'E'
                } elseif($fixture.shape -in @('nested','gbchild','wfgroup')){
                    $eh=$e.maxy-$e.miny
                    Require (PanelClick $fixture $capture '' "$($fixture.shape)-click-J" -Bind -point @([Math]::Round($e.x,1),$ey)) 'J'
                    Require (PanelClick $fixture $capture '' "$($fixture.shape)-right-J" -Bind -tool 'right_click' -point @([Math]::Round($e.x,1),$ey)) 'J'
                    # 45 of E's 60 rows down: inside the container, below J.
                    Require (PanelClick $fixture $capture '' "$($fixture.shape)-container-below-J" -Bind -point @([Math]::Round($e.x,1),[Math]::Round($e.miny+0.75*$eh,1))) 'E'
                    Require (PanelClick $fixture $capture '' "$($fixture.shape)-click-E-beside" -Bind -point @(($e.minx+4),$ey)) 'E'
                } elseif($fixture.shape -eq 'halfhit'){
                    Require (PanelClick $fixture $capture '' 'halfhit-left-half' -Bind -point @([Math]::Round($e.minx+0.4*$ew,1),$ey)) 'E'
                    Require (PanelClick $fixture $capture '' 'halfhit-right-half' -Bind -point @([Math]::Round($e.minx+0.6*$ew,1),$ey)) 'J'
                } else {
                    # The corner of E's rect is outside its region: the form is drawn there.
                    Require (PanelClick $fixture $capture '' 'region-corner' -Bind -point @(($e.minx+2),($e.miny+2))) 'form'
                    # Inside the region near its right edge; outside the rect's logical
                    # width if the region is read without the window's own scale.
                    Require (PanelClick $fixture $capture '' 'region-edge' -Bind -point @(($e.maxx-4),$ey)) 'E'
                    Require (PanelClick $fixture $capture 'E' 'region-centre' -Bind) 'E'
                }
            }
            Sampling-Stop
            foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
        }}}
    }

    if(Want 9){
        # Phase 9. WPF that neither route reaches (review R3-2, R3-4): WPF in a
        # WinForms form through an ElementHost, which is posted to, and WPF
        # windows while another process holds a synthetic pen device, which
        # makes SM_DIGITIZER report a ready pen. One WPF window starts before
        # the device exists, one after. Each gesture must be refused
        # background_unavailable; the previous driver returned exit 0 with
        # nothing fired. The other way round, WinForms panels in a WPF window
        # through a WindowsFormsHost (final review D7), are real Win32 children
        # that take posted clicks: every gesture must land on its panel, where
        # the previous driver refused them all with the WPF cause.
        $wpf=@();$hosted=@()
        foreach($monitor in @($primary,$highest)){
            $wpf+=LaunchPanel 'elementhost' 'wpf' "elementhost-$($monitor.device -replace '\\\\\.\\','')" $monitor 40 40
            $hosted+=LaunchPanel 'wfhost' 'wpf' "wfhost-$($monitor.device -replace '\\\\\.\\','')" $monitor 40 400
        }
        $wpf+=LaunchPanel 'default' 'wpf' 'wpf-before-pen' $primary 520 40
        $pen=Start-Process (Join-Path $work 'ICUPenDevice.exe') -ArgumentList $work,'held','240' -PassThru;$fixtures+=$pen
        Wait-For {Test-Path (Join-Path $work 'pen-held.json')} 10 'pen device helper did not start' | Out-Null
        $report.pen_device=Get-Content (Join-Path $work 'pen-held.json') -Raw | ConvertFrom-Json
        Start-Sleep -Seconds 2
        $wpf+=LaunchPanel 'default' 'wpf' 'wpf-after-pen' $primary 520 400
        Start-Sleep -Milliseconds 1500
        $report.virtual_desktop_wpf_hosted=Tool shot (Join-Path $work 'virtual-desktop-wpf-hosted.png')
        Sampling-Start
        foreach($fixture in $wpf){
            $capture=Capture $fixture $fixture.label
            $report.captures[$fixture.label]=@{colors=$capture.colors;window=$capture.window;screenshot_id=$capture.screenshot_id;ready=$fixture.ready}
            foreach($cell in @('A','E','I')){ Require (PanelClick $fixture $capture $cell 'wpf-unreached-click' -Bind) "$cell|refuse:background_unavailable" }
            Require (PanelClick $fixture $capture 'C' 'wpf-unreached-right' -Bind -tool 'right_click') 'C|refuse:background_unavailable'
            Require (PanelClick $fixture $capture 'G' 'wpf-unreached-double' -Bind -tool 'double_click') 'G|refuse:background_unavailable'
        }
        foreach($fixture in $hosted){
            $capture=Capture $fixture $fixture.label
            $report.captures[$fixture.label]=@{colors=$capture.colors;window=$capture.window;screenshot_id=$capture.screenshot_id;ready=$fixture.ready}
            foreach($cell in @('A','E','I')){ Require (PanelClick $fixture $capture $cell 'wfhost-click' -Bind) $cell }
            Require (PanelClick $fixture $capture 'C' 'wfhost-right' -Bind -tool 'right_click') 'C'
            Require (PanelClick $fixture $capture 'G' 'wfhost-double' -Bind -tool 'double_click') 'G'
        }
        Sampling-Stop
        foreach($process in $fixtures){if(-not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    }

    if(Want 10){
        # Phase 10. A capture in flight (review R3-1). The agent reads E from a
        # 900 px capture of a 3000x1800 window. A tile's first capture of the
        # window starts, and while it is taken and downscaled, 20 unbound
        # clicks at E's pixel are started 15 ms apart. Each must land on E or
        # be refused ambiguous_capture: the previous driver read a click that
        # arrived between the tile's capture and its downscale at 1:1 and
        # clicked A with exit 0 (9 of 50 single-click trials). Each trial is a
        # new window, since the refusal is sticky. No ownership sampling: the
        # window is larger than the monitor.
        $lower=$scaled[-1];$report.race=@()
        for($trial=1;$trial -le 4;$trial++){
            $f=LaunchPanel 'pmv2' 'big' "race-$trial" $lower 0 0
            Start-Sleep -Milliseconds 800
            $agent=Capture $f "race-$trial-agent" 900
            $e=@($agent.colors.buttons | Where-Object {$_.button -eq 'E'})[0]
            if($e.pixels -lt 50){throw "E not visible in $($agent.png)"}
            $arguments=@{pid=[int]$f.ready.pid;window_id=[long]$f.ready.window;session=$session;_skip_window_change_detection=$true;x=[Math]::Round($e.x,1);y=[Math]::Round($e.y,1)}
            $before=@(PanelEvents $f.label).Count
            $tile=StartCall 'get_window_state' @{pid=[int]$f.ready.pid;window_id=[long]$f.ready.window;include_screenshot=$true;include_accessibility_tree=$false;max_dimension=1799;screenshot_out_file=(Join-Path $work "race-$trial-tile.png");session='icu-displays-tile'}
            $clicks=@()
            for($k=0;$k -lt 20;$k++){ $clicks+=StartCall 'click' $arguments; Start-Sleep -Milliseconds 15 }
            $replies=@($clicks | ForEach-Object {EndCall $_})
            $tileReply=EndCall $tile
            Start-Sleep -Milliseconds 1000
            $downs=@(PanelEvents $f.label | Select-Object -Skip $before | Where-Object {$_.kind -eq 'down'} | ForEach-Object {$_.cell})
            $delivered=@($replies | Where-Object {$_.exit -eq 0 -and -not $_.code}).Count
            $ambiguous=@($replies | Where-Object {$_.code -eq 'ambiguous_capture' -and $_.effect -eq 'refused'}).Count
            # With 20 clients at once, the CLI can find every pipe instance busy
            # and exit before sending anything; such a click never reached the
            # driver and is counted apart.
            $unsent=@($replies | Where-Object {$_.exit -ne 0 -and $_.stderr -match 'daemon is not running'}).Count
            $other=@($replies | Where-Object {-not ($_.exit -eq 0 -and -not $_.code) -and -not ($_.code -eq 'ambiguous_capture' -and $_.effect -eq 'refused') -and -not ($_.exit -ne 0 -and $_.stderr -match 'daemon is not running')} | ForEach-Object {"exit $($_.exit) code '$($_.code)' $($_.stderr)"})
            $row=@{case='capture-race';fixture=$f.label;point=@($arguments.x,$arguments.y);agent_id=$agent.screenshot_id;tile_exit=$tileReply.exit;tile_id=$(if($tileReply.value){$tileReply.value.screenshot_id});
                clicks=$replies.Count;delivered=$delivered;ambiguous_capture=$ambiguous;not_sent=$unsent;other=$other;landed=$downs}
            $report.cases+=$row;$report.race+=$row
            $wrong=@($downs | Where-Object {$_ -ne 'E'})
            # The tile is another session, so nothing it captures, in flight or
            # finished, affects the agent's pixel: every sent click lands on E.
            if($wrong.Count -or $downs.Count -ne $delivered -or $ambiguous -or $other.Count -or $tileReply.exit -ne 0){
                $failures+="capture-race $($f.label): $($replies.Count) clicks, $delivered delivered, $ambiguous ambiguous_capture, $unsent not sent, landed [$($downs -join ',')], other [$($other -join '; ')], tile exit $($tileReply.exit)"
            }
            $f.process.Kill();$f.process.WaitForExit()
        }
    }

    $report.failures=@($failures | Select-Object -Unique)
    if($Mutate){
        # Every moved cell case wanted the neighbour its pixel was moved onto,
        # so no failure means each one fired exactly that wrong control. A
        # refusal, a miss onto nothing, a sampling failure or a crash is a
        # failure, and fails the control.
        $report.mutated_cases=@($report.cases | Where-Object {$_.mutated_from}).Count
        $report.control=if(-not $report.mutated_cases){'did_not_mutate'}elseif(-not $report.failures.Count){'failed_as_required'}else{'failed_for_another_reason'}
        $report.ok=($report.control -eq 'failed_as_required')
    } else { $report.ok=($report.failures.Count -eq 0) }
} catch {
    $report.ok=$false;$report.error=$_.Exception.Message+' @ '+$_.InvocationInfo.PositionMessage
} finally {
    [NativeObserver]::Stop() | Out-Null
    try { $report.virtual_desktop_after=Tool shot (Join-Path $work 'virtual-desktop-after.png') } catch {}
    foreach($process in @($fixtures+$guard+$server)){if($process -and -not $process.HasExited){$process.Kill();$process.WaitForExit()}}
    $report.work=$work
    $report | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $work 'result.json') -Encoding UTF8
    @{work=$work;ok=$report.ok;control=$report.control;error=$report.error;failures=$report.failures} | ConvertTo-Json | Set-Content (Join-Path $env:LOCALAPPDATA 'ICUAudit\latest.json') -Encoding UTF8
}
if(-not $report.ok){exit 1}
