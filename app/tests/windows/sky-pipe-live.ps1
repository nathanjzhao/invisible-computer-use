param(
    # icu.exe built from this revision, and the driver it drives (the packaged cua-driver.exe).
    [Parameter(Mandatory=$true)][string]$IcuExe,
    [Parameter(Mandatory=$true)][string]$Driver,
    # The repository root (for Codex's own client files under reference\openai-sky).
    [Parameter(Mandatory=$true)][string]$Repo,
    [string]$Node='C:\Program Files\nodejs\node.exe',
    [string]$Out='C:\Users\Public\skypipe-live.json'
)
# `icu sky-pipe` on Windows, driven by Codex's own client code (client.js and native-pipe.js, unmodified but for
# the tslib import path) against the real Windows driver and a native WinForms calculator fixture. Run as the
# logged-on standard user in the interactive session (as-user.ps1), not SYSTEM: SSM's session 0 has no desktop.
#
# The fixture is the oracle: the client clicks 6, x, 7, = by element id through the pipe, and the fixture's own
# callbacks (events.jsonl, state.json) must show 4 clicks in that order and a display of 42. Then the pipe's
# access list is checked from a second local user (skypipe-foreign.ps1 does that part as SYSTEM).
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
if([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){ throw 'Interactive desktop required' }
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\skypipe-'+(Get-Date -Format 'HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$name='icu-sky-live-'+[guid]::NewGuid().ToString('N').Substring(0,8)
$pipe='\\.\pipe\'+$name
$r=[ordered]@{user=$env:USERNAME;pipe=$pipe;icu=$IcuExe;icu_sha256=(Get-FileHash $IcuExe -Algorithm SHA256).Hash;driver=$Driver;driver_sha256=(Get-FileHash $Driver -Algorithm SHA256).Hash;work=$work}
$icu=$null; $fixture=$null
try {
    $fx=Join-Path $work 'ICUCalculatorFixture.exe'
    & 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /codepage:65001 /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll ("/out:"+$fx) (Join-Path $here 'CalculatorFixture.cs')
    if($LASTEXITCODE){ throw 'fixture compile failed' }
    $fixture=Start-Process $fx -ArgumentList $work -PassThru
    $dl=(Get-Date).AddSeconds(10); while(-not (Test-Path (Join-Path $work 'ready.json')) -and (Get-Date) -lt $dl){ Start-Sleep -Milliseconds 100 }
    if(-not (Test-Path (Join-Path $work 'ready.json'))){ throw 'fixture did not start' }
    $r.fixture=Get-Content (Join-Path $work 'ready.json') -Raw | ConvertFrom-Json
    # icu sky-pipe over the driver, approval off (Codex's client approves for itself)
    $env:ICU_DRIVER=$Driver; $env:ICU_HOME=Join-Path $work 'icu-home'; $env:ICU_REQUIRE_APP_APPROVAL='0'; $env:ICU_POST_ACTION_SLEEP_MS='0'
    $icu=Start-Process $IcuExe -ArgumentList 'sky-pipe','--socket',$name -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $work 'icu.err')
    $dl=(Get-Date).AddSeconds(20); while(-not (Test-Path $pipe) -and (Get-Date) -lt $dl -and -not $icu.HasExited){ Start-Sleep -Milliseconds 100 }
    if(-not (Test-Path $pipe)){ throw "the pipe did not appear; icu said: $(Get-Content (Join-Path $work 'icu.err') -Raw)" }
    $r.pipe_listed=@([IO.Directory]::GetFiles('\\.\pipe\') | Where-Object { $_ -like "*$name" }).Count
    # Codex's client files
    $js=Join-Path $work 'js'; New-Item -ItemType Directory $js | Out-Null
    Set-Content (Join-Path $js 'package.json') '{"type": "module"}'
    foreach($f in 'client.js','native-pipe.js','errors.js'){
        $t=Get-Content (Join-Path $Repo "reference\openai-sky\sky_js_src\targets\mac\$f") -Raw -Encoding UTF8
        Set-Content (Join-Path $js $f) ([regex]::Replace($t,'from"[^"]*tslib\.es6\.js"','from"./tslib-shim.js"')) -Encoding UTF8
    }
    Copy-Item (Join-Path $here 'sky-pipe-live\tslib-shim.js') $js
    Copy-Item (Join-Path $here 'sky-pipe-live\sky_win_drive.mjs') $js
    # the app id the layer gives a Windows process is its exe name
    $app='ICUCalculatorFixture.exe'; $r.app=$app
    $p=Start-Process $Node -ArgumentList 'sky_win_drive.mjs',$pipe,$app -WorkingDirectory $js -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $work 'client.out') -RedirectStandardError (Join-Path $work 'client.err')
    $null=$p.Handle # keeps the exit code readable after the process ends
    if(-not $p.WaitForExit(120000)){ $p.Kill(); $r.client_timeout=$true }
    $r.client_exit=$(if($p.HasExited){$p.ExitCode}else{-1})
    $r.client_err=(Get-Content (Join-Path $work 'client.err') -Raw -ErrorAction SilentlyContinue)
    $lines=@(Get-Content (Join-Path $work 'client.out') -Encoding UTF8 -ErrorAction SilentlyContinue)
    if($lines.Count){ $r.client=($lines[-1] | ConvertFrom-Json) }
    Copy-Item (Join-Path $work 'client.out') (Join-Path $work 'client-output.json')
    $state=Get-Content (Join-Path $work 'state.json') -Encoding UTF8 | ConvertFrom-Json
    $events=@(Get-Content (Join-Path $work 'events.jsonl') -Encoding UTF8 -ErrorAction SilentlyContinue | ForEach-Object { $_ | ConvertFrom-Json })
    $r.receiver=@{display=$state.display;buttons=@($state.buttons);events=$events.Count}
    $mult=[string][char]0xD7
    $r.ok=($r.client_exit -eq 0 -and $state.display -eq '42' -and (@($state.buttons) -join '|') -eq ('6|'+$mult+'|7|=') -and $events.Count -eq 4)
} catch { $r.ok=$false; $r.error=$_.Exception.Message }
finally {
    if($icu -and -not $icu.HasExited){ $icu.Kill() }
    if($fixture -and -not $fixture.HasExited){ $fixture.Kill() }
    $r | ConvertTo-Json -Depth 12 | Set-Content $Out -Encoding UTF8
}
if(-not $r.ok){ exit 1 }
