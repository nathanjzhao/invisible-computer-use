param(
    # The install directory holding the signed cua-driver-uia.exe (under Program Files, so it gets UIAccess).
    [Parameter(Mandatory=$true)][string]$Dir,
    [string]$Out=''
)
# Run as the interactive standard user (as-user.ps1). This script is the worker's authorized parent
# (--authorized-parent-pid), so it can send the worker the requests the daemon sends and read the
# replies. It checks the worker's own guards against a normal-integrity fixture window, where
# delivery works, so each refusal is the worker's and not UIPI's:
#   snapshot  returns the tree as data; out-of-bounds caps are refused
#   input     delivers a click inside the window; refuses a click outside it, a malformed value,
#             a window that is not a visible top-level window, an unknown op
#   text      with a focus description: types into the element it matches (focus is moved there
#             first), refuses and types nothing when no live element matches
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
$workerExe=Join-Path $Dir 'cua-driver-uia.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps3-worker-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$report=[ordered]@{case='worker';dir=$Dir;worker_sha256=(Get-FileHash $workerExe -Algorithm SHA256).Hash}
$procs=@()
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
function Read-Text([string]$name) { $p=Join-Path $work $name; if(Test-Path $p){[IO.File]::ReadAllText($p)}else{''} }
function Count-Events([string]$kind) { @((Read-Text 'events.txt') -split "`n" | Where-Object {$_ -like "$kind*"}).Count }
function Worker-Send([string]$line) {
    $p=New-Object IO.Pipes.NamedPipeClientStream('.','cua-driver-uia',[IO.Pipes.PipeDirection]::InOut)
    $p.Connect(5000)
    $w=New-Object IO.StreamWriter($p);$w.AutoFlush=$true;$r=New-Object IO.StreamReader($p)
    $w.WriteLine($line)
    $t=$r.ReadLineAsync()
    $text=$(if($t.Wait(30000)){$t.Result}else{'timeout'})
    $p.Dispose()
    return $text
}
function Ok($reply) { $reply -match '"ok":true' }
try {
    $csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    $fx=Join-Path $work 'UiaGestureFixture.exe'
    & $csc /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll ("/out:$fx") (Join-Path $PSScriptRoot 'UiaGestureFixture.cs') | Out-Null
    $procs+=(Start-Process $fx -ArgumentList ('"'+$work+'"') -PassThru)
    $ready=Wait-For {if(Test-Path (Join-Path $work 'target.ready')){(Get-Content (Join-Path $work 'target.ready')).Split(' ')}} 15 'fixture did not appear'
    $hwnd=[int64]$ready[0]
    $layout=(Read-Text 'layout.json') | ConvertFrom-Json
    $worker=Start-Process $workerExe -ArgumentList '--authorized-parent-pid',$PID -PassThru -WindowStyle Hidden
    $procs+=$worker
    Wait-For {[IO.Directory]::GetFiles('\\.\pipe\') -contains '\\.\pipe\cua-driver-uia'} 10 'worker pipe did not appear' | Out-Null
    $r=[ordered]@{}

    # snapshot: the tree comes back as data
    $snap=Worker-Send ('{"method":"snapshot","hwnd":'+$hwnd+',"max_elements":5000,"max_depth":25}')
    $parsed=$null; try { $parsed=$snap | ConvertFrom-Json } catch {}
    $r.snapshot_ok=(Ok $snap) -and $parsed -and (@($parsed.result.nodes).Count -gt 400) -and (@($parsed.result.nodes | Where-Object {$_.name -eq 'ICU-TEXT'}).Count -eq 1)
    $r.snapshot_nodes=@($parsed.result.nodes).Count
    $r.snapshot_caps_refused=(-not (Ok (Worker-Send ('{"method":"snapshot","hwnd":'+$hwnd+',"max_elements":0,"max_depth":25}')))) -and
        (-not (Ok (Worker-Send ('{"method":"snapshot","hwnd":'+$hwnd+',"max_elements":99999999,"max_depth":25}')))) -and
        (-not (Ok (Worker-Send ('{"method":"snapshot","hwnd":'+$hwnd+',"max_elements":10,"max_depth":1000}'))))
    $r.snapshot_non_window_refused=-not (Ok (Worker-Send '{"method":"snapshot","hwnd":1,"max_elements":10,"max_depth":5}'))

    # input
    $bx=$layout.button[0]; $by=$layout.button[1]
    $before=Count-Events 'click'
    $in=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":'+$bx+',"y":'+$by+',"count":1,"button":"left","modifiers":[],"activate":true}}')
    Start-Sleep -Milliseconds 500
    $r.click_inside_delivered=(Ok $in) -and ((Count-Events 'click') -gt $before)
    $before=Count-Events 'click'
    $outside=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":5,"y":5,"count":1,"button":"left","modifiers":[],"activate":true}}')
    Start-Sleep -Milliseconds 400
    $r.click_outside_window_refused=(-not (Ok $outside)) -and ($outside -match 'outside') -and ((Count-Events 'click') -eq $before)
    $bad=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":'+$bx+',"y":'+$by+',"count":9,"button":"left","modifiers":[],"activate":true}}')
    $r.bad_count_refused=(-not (Ok $bad)) -and ((Count-Events 'click') -eq $before)
    $bad=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":'+$bx+',"y":'+$by+',"count":1,"button":"back","modifiers":[],"activate":true}}')
    $r.bad_button_refused=(-not (Ok $bad)) -and ((Count-Events 'click') -eq $before)
    # drag: a press, moves and a release inside the window land on the pad; one that would release outside
    # the window (or is shaped wrong) is refused before anything is pressed
    $dx=$layout.pad[0]; $dy=$layout.pad[1]
    function Drag-Op([int]$fx,[int]$fy,[int]$tx,[int]$ty,[string]$extra='"duration_ms":300,"steps":10,"button":"left"') { '{"method":"input","input":{"op":"drag","hwnd":'+$hwnd+',"from_x":'+$fx+',"from_y":'+$fy+',"to_x":'+$tx+',"to_y":'+$ty+','+$extra+'}}' }
    $beforeDrag=Count-Events 'drag'
    $d=Worker-Send (Drag-Op ($dx-40) $dy ($dx+40) $dy)
    Start-Sleep -Milliseconds 700
    $r.drag_inside_delivered=(Ok $d) -and ((Count-Events 'drag') -gt $beforeDrag)
    $beforeDrag=Count-Events 'drag'; $beforeDown=Count-Events 'mousedown'
    $d=Worker-Send (Drag-Op ($dx-40) $dy 5 5)
    Start-Sleep -Milliseconds 700
    $r.drag_ending_outside_window_refused=(-not (Ok $d)) -and ($d -match 'outside') -and ((Count-Events 'drag') -eq $beforeDrag)
    Start-Sleep -Milliseconds 300
    $shapes=[ordered]@{ drag_steps_zero_refused='"duration_ms":300,"steps":0,"button":"left"'; drag_steps_over_cap_refused='"duration_ms":300,"steps":501,"button":"left"';
                       drag_duration_over_cap_refused='"duration_ms":99999,"steps":10,"button":"left"'; drag_unknown_button_refused='"duration_ms":300,"steps":10,"button":"back"' }
    foreach($name in $shapes.Keys){
        $beforeShape=Count-Events 'drag'
        $reply=Worker-Send (Drag-Op ($dx-40) $dy ($dx+40) $dy $shapes[$name])
        Start-Sleep -Milliseconds 700
        # a reply that never came (a drag that runs for a minute) is not a refusal: the worker has to say no
        $r[$name]=($reply -match '"ok":false') -and ((Count-Events 'drag') -eq $beforeShape)
    }
    $r.unknown_op_refused=-not (Ok (Worker-Send ('{"method":"input","input":{"op":"format_disk","hwnd":'+$hwnd+'}}')))
    $r.not_a_window_refused=-not (Ok (Worker-Send '{"method":"input","input":{"op":"wheel","hwnd":1,"x":1,"y":1,"ticks":1,"horizontal":false}}'))
    $r.no_input_refused=-not (Ok (Worker-Send '{"method":"input"}'))

    # text with a focus description: focus first moves off the text box (a click on the list), then
    # the worker must find the described element, focus it and type there.
    $lx=$layout.list[0]; $ly=$layout.list[1]
    $null=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":'+$lx+',"y":'+$ly+',"count":1,"button":"left","modifiers":[],"activate":true}}')
    Start-Sleep -Milliseconds 400
    $tx=$layout.text[0]; $ty=$layout.text[1]
    $spec='{"native_hwnd":0,"automation_id":"","name":"ICU-TEXT","control_type":"Edit","class_name":"","x":'+$tx+',"y":'+$ty+',"click_fallback":true}'
    $t=Worker-Send ('{"method":"text","hwnd":'+$hwnd+',"text":"typed-by-worker","focus":'+$spec+'}')
    Start-Sleep -Milliseconds 500
    $r.text_with_focus_lands_in_the_described_element=(Ok $t) -and ((Read-Text 'text.txt') -match 'typed-by-worker')
    # an element that does not exist: refused, nothing typed anywhere
    $null=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":'+$lx+',"y":'+$ly+',"count":1,"button":"left","modifiers":[],"activate":true}}')
    Start-Sleep -Milliseconds 400
    $beforeText=Read-Text 'text.txt'
    $nospec='{"native_hwnd":0,"automation_id":"","name":"NO-SUCH-ELEMENT","control_type":"Edit","class_name":"","x":'+$tx+',"y":'+$ty+',"click_fallback":false}'
    $t=Worker-Send ('{"method":"text","hwnd":'+$hwnd+',"text":"must-not-appear","focus":'+$nospec+'}')
    Start-Sleep -Milliseconds 400
    $r.text_for_a_missing_element_refused=(-not (Ok $t)) -and ((Read-Text 'text.txt') -eq $beforeText) -and (-not ((Read-Text 'text.txt') -match 'must-not-appear'))
    # a description with the right name but another control type is not that element
    $wrongtype='{"native_hwnd":0,"automation_id":"","name":"ICU-TEXT","control_type":"Button","class_name":"","x":'+$tx+',"y":'+$ty+',"click_fallback":false}'
    $t=Worker-Send ('{"method":"text","hwnd":'+$hwnd+',"text":"also-must-not-appear","focus":'+$wrongtype+'}')
    Start-Sleep -Milliseconds 400
    $r.text_for_the_wrong_control_type_refused=(-not (Ok $t)) -and (-not ((Read-Text 'text.txt') -match 'also-must-not-appear'))
    # a description with nothing to identify the element by is not enough to type on
    $anon='{"native_hwnd":0,"automation_id":"","name":"","control_type":"Edit","class_name":"","x":'+$tx+',"y":'+$ty+',"click_fallback":false}'
    $t=Worker-Send ('{"method":"text","hwnd":'+$hwnd+',"text":"anon-must-not-appear","focus":'+$anon+'}')
    Start-Sleep -Milliseconds 400
    $r.text_for_an_anonymous_description_refused=(-not (Ok $t)) -and (-not ((Read-Text 'text.txt') -match 'anon-must-not-appear'))
    $report.checks=$r
    $report.pass=(@($r.Values | Where-Object {$_ -is [bool] -and -not $_}).Count -eq 0)
} catch {
    $report.error=$_.Exception.Message
    $report.pass=$false
} finally {
    foreach($p in $procs){ try { if($p -and -not $p.HasExited){$p.Kill()} } catch {} }
    $json=$report | ConvertTo-Json -Depth 6
    if($Out){$json | Set-Content -Encoding UTF8 $Out}
    $json
}
if(-not $report.pass){exit 1}
