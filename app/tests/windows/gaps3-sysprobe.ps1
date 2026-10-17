param(
    [Parameter(Mandatory=$true)][string]$Dir,
    [Parameter(Mandatory=$true)][string]$ExternalDir,
    [string]$Out=''
)
# Run as the interactive user (as-user.ps1), with a gesture fixture already running at higher
# integrity (gaps3-run.ps1 starts it and passes -ExternalDir). This script is the UIAccess worker's
# authorized parent, so it can ask the worker directly what Windows lets a UIAccess process do to
# that window, with no driver logic in between:
#   snapshot    how many nodes of the window's UIA tree the worker can read
#   post_click  a posted pointer click (PostMessage)
#   post_scroll posted scroll messages
#   wheel/click SendInput gestures, whose effect is read from the fixture's own files
# The answer differs between a High-integrity window of the same user and a SYSTEM window.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
$workerExe=Join-Path $Dir 'cua-driver-uia.exe'
$report=[ordered]@{case='sysprobe';dir=$Dir}
$procs=@()
function Read-Text([string]$name) { $p=Join-Path $ExternalDir $name; if(Test-Path $p){[IO.File]::ReadAllText($p)}else{''} }
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
try {
    $ready=(Get-Content (Join-Path $ExternalDir 'target.ready')).Split(' ')
    $hwnd=[int64]$ready[0]
    $layout=(Read-Text 'layout.json') | ConvertFrom-Json
    $worker=Start-Process $workerExe -ArgumentList '--authorized-parent-pid',$PID -PassThru -WindowStyle Hidden
    $procs+=$worker
    $deadline=(Get-Date).AddSeconds(10)
    while(-not ([IO.Directory]::GetFiles('\\.\pipe\') -contains '\\.\pipe\cua-driver-uia') -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 100}
    $snap=Worker-Send ('{"method":"snapshot","hwnd":'+$hwnd+',"max_elements":5000,"max_depth":25}')
    $parsed=$null; try { $parsed=$snap | ConvertFrom-Json } catch {}
    $report.snapshot=@{reply_ok=[bool]($snap -match '"ok":true');nodes=@($parsed.result.nodes).Count;names=@($parsed.result.nodes | Select-Object -First 6 | ForEach-Object {''+$_.control_type+':'+$_.name});error=$(if($snap -notmatch '"ok":true'){$snap.Substring(0,[Math]::Min(300,$snap.Length))}else{$null})}
    $bx=$layout.button[0]; $by=$layout.button[1]
    $before=Count-Events 'mousedown'
    $r=Worker-Send ('{"method":"input","input":{"op":"post_click","hwnd":'+$hwnd+',"x":'+$bx+',"y":'+$by+',"count":1,"button":"left"}}')
    Start-Sleep -Milliseconds 600
    $report.post_click=@{reply=$r.Substring(0,[Math]::Min(300,$r.Length));pointer_press_seen=((Count-Events 'mousedown') -gt $before)}
    $lx=$layout.list[0]; $ly=$layout.list[1]
    $top0=[int]((Read-Text 'scroll.txt').Trim())
    $r=Worker-Send ('{"method":"input","input":{"op":"post_scroll","hwnd":'+$hwnd+',"direction":"down","by":"line","amount":3,"at":['+$lx+','+$ly+']}}')
    Start-Sleep -Milliseconds 600
    $report.post_scroll=@{reply=$r.Substring(0,[Math]::Min(300,$r.Length));moved=([int]((Read-Text 'scroll.txt').Trim()) -gt $top0)}
    $top0=[int]((Read-Text 'scroll.txt').Trim())
    $r=Worker-Send ('{"method":"input","input":{"op":"wheel","hwnd":'+$hwnd+',"x":'+$lx+',"y":'+$ly+',"ticks":-3,"horizontal":false}}')
    Start-Sleep -Milliseconds 600
    $report.wheel=@{reply=$r.Substring(0,[Math]::Min(300,$r.Length));moved=([int]((Read-Text 'scroll.txt').Trim()) -gt $top0)}
    $before=Count-Events 'mousedown'
    $r=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":'+$bx+',"y":'+$by+',"count":1,"button":"left","modifiers":[],"activate":true}}')
    Start-Sleep -Milliseconds 600
    $report.click=@{reply=$r.Substring(0,[Math]::Min(300,$r.Length));pointer_press_seen=((Count-Events 'mousedown') -gt $before)}
    $report.pass=$true
} catch {
    $report.error=$_.Exception.Message
    $report.pass=$false
} finally {
    foreach($p in $procs){ try { if($p -and -not $p.HasExited){$p.Kill()} } catch {} }
    $json=$report | ConvertTo-Json -Depth 6
    if($Out){$json | Set-Content -Encoding UTF8 $Out}
    $json
}
