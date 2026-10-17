param(
    # The install directory holding the signed cua-driver.exe and cua-driver-uia.exe
    # (under Program Files, or UIAccess is not granted).
    [Parameter(Mandatory=$true)][string]$Dir,
    # normal:     a target at the daemon's own integrity; the plain foreground swap works, the key lands
    #             and the worker must NOT have been started.
    # elevated:   a target at SYSTEM integrity in this session (started by elevated-run.ps1, which
    #             passes -ExternalDir). The daemon's own SendInput is dropped by UIPI. Expect
    #             forwarded: the key lands through the UIAccess worker. Expect refused: the call
    #             fails with the UIPI diagnostic and no worker runs (what a daemon without the
    #             forwarding does).
    # outside:    as elevated, but the install is not under Program Files (-Dir points there): the
    #             worker starts without UIAccess, so nothing is delivered and the call says why.
    # impostor:   as elevated, but an unrelated process owns the worker's pipe name; the daemon
    #             must refuse and send that process nothing.
    # security:   the worker's own checks (no parent pid, wrong client, parent exit, no CLI route).
    [Parameter(Mandatory=$true)][ValidateSet('normal','elevated','outside','impostor','security')][string]$Case,
    [ValidateSet('forwarded','refused')][string]$Expect='forwarded',
    [string]$ExternalDir='',
    [string]$Out=''
)
# Run as the interactive standard user (as-user.ps1), not SYSTEM: SSM's session 0 has no desktop.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
if ((New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run without administrator privileges' }
$driver=Join-Path $Dir 'cua-driver.exe'
$workerExe=Join-Path $Dir 'cua-driver-uia.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\uia-forward-'+$Case+'-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$pipe='\\.\pipe\icu-uia-forward-'+[guid]::NewGuid().ToString('N')
$report=[ordered]@{case=$Case;expect=$Expect;dir=$Dir;user=$env:USERNAME;driver_sha256=(Get-FileHash $driver -Algorithm SHA256).Hash;
    worker_sha256=(Get-FileHash $workerExe -Algorithm SHA256).Hash;
    driver_signature="$((Get-AuthenticodeSignature $driver).Status)";worker_signature="$((Get-AuthenticodeSignature $workerExe).Status)"}
$procs=@()
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuUf {
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint a, bool i, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, out int v, int l, out int r);
    public static int UIAccess(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int v, r; bool ok = GetTokenInformation(t, 26, out v, 4, out r); CloseHandle(t); CloseHandle(p); return ok ? v : -1;
    }
}
'@
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
function Workers { @(Get-Process -Name 'cua-driver-uia' -ErrorAction SilentlyContinue) }
function Run-Driver([string]$arguments) {
    $info=New-Object Diagnostics.ProcessStartInfo $driver
    $info.Arguments=$arguments
    $info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.CreateNoWindow=$true
    $process=[Diagnostics.Process]::Start($info)
    $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEnd()
    $process.WaitForExit()
    return @{exit=$process.ExitCode;stdout=$stdout.Result.Trim();stderr=$stderr.Trim()}
}
function Call([string]$tool,$arguments) {
    $json=$arguments | ConvertTo-Json -Depth 8 -Compress
    return Run-Driver ('--socket '+$pipe+' call '+$tool+' "'+($json -replace '"','\"')+'"')
}
# One request line to a pipe, from a separate process (so its pid is not this script's).
function Pipe-Client([string]$name,[string]$line) {
    $lines=@(
        ('$p=New-Object IO.Pipes.NamedPipeClientStream(".","'+$name+'",[IO.Pipes.PipeDirection]::InOut)'),
        'try { $p.Connect(3000) } catch { "connect_failed"; exit 0 }',
        '$w=New-Object IO.StreamWriter($p); $w.AutoFlush=$true',
        ('try { $w.WriteLine('''+$line+''') } catch { "write_failed"; exit 0 }'),
        '$r=New-Object IO.StreamReader($p)',
        '$t=$r.ReadLineAsync()',
        'if($t.Wait(3000)){ if($t.Result){$t.Result}else{"closed_without_reply"} } else { "timeout" }')
    $file=Join-Path $work ('client-'+[guid]::NewGuid().ToString('N')+'.ps1')
    [IO.File]::WriteAllLines($file,$lines)
    $o=& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $file
    return (($o | Out-String).Trim())
}
try {
    if($Case -eq 'security'){
        # (a) no parent pid: the worker must refuse to run
        $bare=Start-Process $workerExe -PassThru -WindowStyle Hidden
        $bare.WaitForExit(5000) | Out-Null
        $report.no_parent_pid=@{exited=$bare.HasExited;exit_code=$(if($bare.HasExited){$bare.ExitCode}else{$null})}
        if(-not $bare.HasExited){$bare.Kill()}
        # (b) the pipe accepts its authorized parent (this script) and rejects every other process
        $worker=Start-Process $workerExe -ArgumentList '--authorized-parent-pid',$PID -PassThru -WindowStyle Hidden
        Wait-For {[IO.Directory]::GetFiles('\\.\pipe\') -contains '\\.\pipe\cua-driver-uia'} 8 'worker pipe did not appear' | Out-Null
        $report.worker=@{pid=$worker.Id;uiaccess=[IcuUf]::UIAccess($worker.Id)}
        $report.other_process_key=Pipe-Client 'cua-driver-uia' '{"method":"key","hwnd":1,"key":"a"}'
        $report.other_process_list=Pipe-Client 'cua-driver-uia' '{"method":"list"}'
        # the authorized parent itself is answered: an invalid window is refused with a reason
        $own=New-Object IO.Pipes.NamedPipeClientStream('.','cua-driver-uia',[IO.Pipes.PipeDirection]::InOut)
        $own.Connect(3000)
        $w=New-Object IO.StreamWriter($own);$w.AutoFlush=$true;$r=New-Object IO.StreamReader($own)
        $w.WriteLine('{"method":"key","hwnd":1,"key":"a"}')
        $t=$r.ReadLineAsync();$report.parent_key_bad_hwnd=$(if($t.Wait(3000)){$t.Result}else{'timeout'})
        $own.Dispose()
        # the public CLI route: `cua-driver --socket <worker pipe> call ...` must not work
        $report.cli_to_worker=Run-Driver '--socket \\.\pipe\cua-driver-uia call list_windows "{}"'
        # (c) the worker leaves with its authorized parent
        $parent=Start-Process powershell.exe -ArgumentList '-NoProfile','-Command','Start-Sleep 600' -PassThru -WindowStyle Hidden
        try { Stop-Process -Id $worker.Id -Force -ErrorAction Stop } catch {}
        Start-Sleep -Seconds 1
        $worker2=Start-Process $workerExe -ArgumentList '--authorized-parent-pid',$parent.Id -PassThru -WindowStyle Hidden
        Start-Sleep -Seconds 2
        $report.parent_exit=@{worker_alive_before=-not $worker2.HasExited}
        $parent.Kill();Start-Sleep -Seconds 3
        $report.parent_exit.worker_alive_after=[bool](Get-Process -Id $worker2.Id -ErrorAction SilentlyContinue)
        $rejected=@('closed_without_reply','write_failed','timeout')
        $report.pass=($report.no_parent_pid.exited -eq $true) -and ($report.no_parent_pid.exit_code -ne 0) -and
            ($report.other_process_key -in $rejected) -and ($report.other_process_list -in $rejected) -and
            ($report.parent_key_bad_hwnd -match '"ok":false') -and
            ($report.cli_to_worker.exit -ne 0) -and
            ($report.parent_exit.worker_alive_before -eq $true) -and ($report.parent_exit.worker_alive_after -eq $false)
    } else {
        $report.workers_before=(Workers).Count
        if($Case -eq 'normal'){
            $csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
            $fx=Join-Path $work 'UiaForwardFixture.exe'
            & $csc /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll ("/out:$fx") (Join-Path $PSScriptRoot 'UiaForwardFixture.cs') | Out-Null
            if(-not (Test-Path $fx)){throw 'fixture did not compile'}
            $target=Start-Process $fx -ArgumentList 'target',('"'+$work+'"') -PassThru; $procs+=$target
            $textDir=$work
        } else {
            if(-not $ExternalDir){throw '-ExternalDir is required for this case'}
            $textDir=$ExternalDir
        }
        $ready=Wait-For {if(Test-Path (Join-Path $textDir 'target.ready')){(Get-Content (Join-Path $textDir 'target.ready')).Split(' ')}} 10 'target window is not up'
        $targetHwnd=[int64]$ready[0]; $targetPid=[int]$ready[1]
        $report.target=@{hwnd=$targetHwnd;pid=$targetPid}

        if($Case -eq 'impostor'){
            $iscript=Join-Path $work 'impostor.ps1'
            $received=Join-Path $work 'impostor.received'
            [IO.File]::WriteAllLines($iscript,@(
                '$s=New-Object IO.Pipes.NamedPipeServerStream("cua-driver-uia",[IO.Pipes.PipeDirection]::InOut,4)',
                '$s.WaitForConnection()',
                '$r=New-Object IO.StreamReader($s)',
                '$t=$r.ReadLineAsync()',
                ('if($t.Wait(8000)){ [IO.File]::WriteAllText("'+$received+'",$t.Result) } else { [IO.File]::WriteAllText("'+$received+'","NOTHING") }')))
            $procs+=(Start-Process powershell.exe -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',$iscript -PassThru -WindowStyle Hidden)
            Wait-For {[IO.Directory]::GetFiles('\\.\pipe\') -contains '\\.\pipe\cua-driver-uia'} 8 'impostor pipe did not appear' | Out-Null
        }

        $procs+=(Start-Process $driver -ArgumentList 'serve','--no-overlay','--socket',$pipe -WindowStyle Hidden -PassThru)
        Wait-For {(Call 'list_windows' @{}).exit -eq 0} 20 'driver did not serve' | Out-Null
        $report.workers_after_serve=(Workers).Count
        $call=Call 'press_key' @{pid=$targetPid;window_id=$targetHwnd;key='a';delivery_mode='foreground'}
        $report.press_key=$call
        $call2=Call 'type_text' @{pid=$targetPid;window_id=$targetHwnd;text='xyz';delivery_mode='foreground'}
        $report.type_text=$call2
        Start-Sleep -Milliseconds 800
        $textPath=Join-Path $textDir 'text.txt'
        $report.text=$(if(Test-Path $textPath){[IO.File]::ReadAllText($textPath)}else{''})
        $w=Workers
        $report.workers_after_call=$w.Count
        if($w.Count){$report.worker=@{pid=$w[0].Id;uiaccess=[IcuUf]::UIAccess($w[0].Id);path=$w[0].Path}}
        if($Case -eq 'impostor'){
            Start-Sleep -Seconds 1
            $report.impostor_received=$(if(Test-Path (Join-Path $work 'impostor.received')){[IO.File]::ReadAllText((Join-Path $work 'impostor.received'))}else{'(nothing arrived)'})
        }
        $landed=($report.text -match 'a') -and ($report.text -match 'xyz')
        $landedAny=($report.text -match 'a') -or ($report.text -match 'xyz')
        $said=($call.stdout+' '+$call.stderr+' '+$call2.stdout+' '+$call2.stderr)
        switch($Case){
            'normal' { $report.pass=$landed -and ($report.workers_after_call -eq 0) -and ($call.exit -eq 0) -and ($call2.exit -eq 0) }
            'outside' { $report.pass=(-not $landedAny) -and ($said -match 'UIPI') -and ((-not $report.worker) -or ($report.worker.uiaccess -eq 0)) }
            'impostor' { $report.pass=(-not $landedAny) -and ($report.impostor_received -notmatch 'method') -and ($said -match 'UIPI') -and ($said -match 'is served by') }
            default {
                if($Expect -eq 'forwarded'){ $report.pass=$landed -and ($call.exit -eq 0) -and ($call2.exit -eq 0) -and ($report.worker.uiaccess -eq 1) }
                else { $report.pass=(-not $landedAny) -and ($said -match 'UIPI') -and ($report.workers_after_call -eq 0) }
            }
        }
    }
} catch {
    $report.error=$_.Exception.Message
    $report.pass=$false
} finally {
    foreach($p in $procs){ try { if($p -and -not $p.HasExited){$p.Kill()} } catch {} }
    foreach($w in (Workers)){ try { Stop-Process -Id $w.Id -Force -ErrorAction Stop } catch {} }
    $json=$report | ConvertTo-Json -Depth 6
    if($Out){$json | Set-Content -Encoding UTF8 $Out}
    $json
}
if(-not $report.pass){exit 1}
