param(
    [Parameter(Mandatory=$true)][string]$Dir,
    [Parameter(Mandatory=$true)][string]$ExternalDir,
    [string]$Out='',
    # The fixture opted in to the mouse messages (UiaGestureFixture's allow-messages): the same posts must then land.
    [switch]$ExpectOptIn,
    [ValidateSet('high','system')][string]$TargetLevel='system'
)
# Run as the interactive user (as-user.ps1) with a gesture fixture at higher integrity already running
# (gaps3-run.ps1 -UserScript gaps4-uipi.ps1 starts it). Shows precisely why a background (posted) gesture into
# a window above the caller's integrity cannot be delivered, for two senders:
#   1. this process (normal integrity), PostMessage straight to the control under the button's centre;
#   2. the UIAccess worker (cua-driver-uia.exe, token UIAccess 1), asked for a posted click over its pipe.
# Both are refused by Windows (UIPI, ERROR_ACCESS_DENIED = 5) and nothing reaches the control; a SendInput click
# from the worker does reach it. When the window opted in to the mouse messages itself
# (ChangeWindowMessageFilterEx, the fixture's allow-messages argument), the same posts land: UIPI is the reason.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
$workerExe=Join-Path $Dir 'cua-driver-uia.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps4-uipi-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$report=[ordered]@{case='uipi';optin=[bool]$ExpectOptIn;dir=$Dir}
$procs=@()
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuUipi {
    [StructLayout(LayoutKind.Sequential)] public struct PT { public int x, y; }
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(PT p);
    [DllImport("user32.dll", SetLastError=true)] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool ScreenToClient(IntPtr h, ref PT p);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, out int v, int l, out int r);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, byte[] v, int l, out int r);
    public static long ControlAt(int x, int y) { PT p; p.x = x; p.y = y; return (long)WindowFromPoint(p); }
    // PostMessage a left click (down, up) at a screen point to the control there. Returns "ok" or "error <code>" per message.
    public static string PostClick(long h, int sx, int sy) {
        PT p; p.x = sx; p.y = sy; ScreenToClient((IntPtr)h, ref p);
        IntPtr lp = (IntPtr)((p.y << 16) | (p.x & 0xFFFF));
        string a = PostMessageW((IntPtr)h, 0x201, (IntPtr)1, lp) ? "ok" : "error " + Marshal.GetLastWin32Error();
        string b = PostMessageW((IntPtr)h, 0x202, IntPtr.Zero, lp) ? "ok" : "error " + Marshal.GetLastWin32Error();
        return "WM_LBUTTONDOWN " + a + ", WM_LBUTTONUP " + b;
    }
    public static int UIAccess(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int v, r; bool ok = GetTokenInformation(t, 26, out v, 4, out r); CloseHandle(t); CloseHandle(p); return ok ? v : -1;
    }
    public static int Integrity(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int need; GetTokenInformation(t, 25, null, 0, out need);
        byte[] b = new byte[need]; int got;
        bool ok = GetTokenInformation(t, 25, b, need, out got); CloseHandle(t); CloseHandle(p);
        if (!ok) return -1;
        IntPtr sid = (IntPtr)(Environment.Is64BitProcess ? BitConverter.ToInt64(b, 0) : BitConverter.ToInt32(b, 0));
        int count = Marshal.ReadByte(sid, 1);
        return Marshal.ReadInt32(sid, 8 + 4 * (count - 1));
    }
}
'@
function Read-Text([string]$name) { $p=Join-Path $ExternalDir $name; if(-not (Test-Path $p)){return ''}; try { $f=[IO.File]::Open($p,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite); try { (New-Object IO.StreamReader($f)).ReadToEnd() } finally { $f.Dispose() } } catch { '' } }
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
    $hwnd=[int64]$ready[0]; $targetPid=[int]$ready[1]
    $layout=(Read-Text 'layout.json') | ConvertFrom-Json
    $bx=[int]$layout.button[0]; $by=[int]$layout.button[1]
    $report.target=@{hwnd=$hwnd;pid=$targetPid;integrity=('0x{0:x}' -f [IcuUipi]::Integrity($targetPid))}
    $report.self=@{pid=$PID;integrity=('0x{0:x}' -f [IcuUipi]::Integrity($PID));uiaccess=[IcuUipi]::UIAccess($PID)}
    $control=[IcuUipi]::ControlAt($bx,$by)
    $report.control_under_point=$control

    # 1. this process posts the click itself
    # (a small separate process, UipiPoster.exe, as in gaps4-uipi-matrix.ps1; it posts to the control inside the fixture's own
    # window, so a window of another process that covers that screen point, such as this task's console, cannot take the post)
    $poster=Join-Path $work 'UipiPoster.exe'
    & 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /target:winexe "/out:$poster" (Join-Path $PSScriptRoot 'UipiPoster.cs') | Out-Null
    $before=Count-Events 'mousedown'
    Remove-Item (Join-Path $ExternalDir 'poster-own.json') -ErrorAction SilentlyContinue
    Start-Process $poster -ArgumentList ('"'+$ExternalDir+'"'),'own' -Wait
    Start-Sleep -Milliseconds 700
    $pj=$null; try { $pj=Get-Content (Join-Path $ExternalDir 'poster-own.json') -Raw | ConvertFrom-Json } catch {}
    $report.own_post=@{result=('WM_LBUTTONDOWN '+$pj.down+', WM_LBUTTONUP '+$pj.up);sender_integrity=$pj.integrity;reached_the_control=((Count-Events 'mousedown') -gt $before)}

    # 2. the UIAccess worker posts it
    $worker=Start-Process $workerExe -ArgumentList '--authorized-parent-pid',$PID -PassThru -WindowStyle Hidden
    $procs+=$worker
    $deadline=(Get-Date).AddSeconds(10)
    while(-not ([IO.Directory]::GetFiles('\\.\pipe\') -contains '\\.\pipe\cua-driver-uia') -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 100}
    $report.worker=@{pid=$worker.Id;uiaccess=[IcuUipi]::UIAccess($worker.Id);integrity=('0x{0:x}' -f [IcuUipi]::Integrity($worker.Id))}
    $before=Count-Events 'mousedown'
    $r=Worker-Send ('{"method":"input","input":{"op":"post_click","hwnd":'+$hwnd+',"x":'+$bx+',"y":'+$by+',"count":1,"button":"left"}}')
    Start-Sleep -Milliseconds 700
    $report.worker_post=@{reply=$r.Substring(0,[Math]::Min(400,$r.Length));reached_the_control=((Count-Events 'mousedown') -gt $before)}

    # 3. the worker's SendInput click (not a message: UIAccess is what lets it through)
    $before=Count-Events 'mousedown'
    $r=Worker-Send ('{"method":"input","input":{"op":"click","hwnd":'+$hwnd+',"x":'+$bx+',"y":'+$by+',"count":1,"button":"left","modifiers":[],"activate":true}}')
    Start-Sleep -Milliseconds 700
    $report.worker_sendinput=@{reply=$r.Substring(0,[Math]::Min(300,$r.Length));reached_the_control=((Count-Events 'mousedown') -gt $before)}

    # What Windows does, as measured (see the README): a UIAccess process runs at High integrity, so it may post to a
    # High window but not a SYSTEM one; a normal (medium) process may post to neither; a window that opted in
    # (ChangeWindowMessageFilterEx) takes the posts of both.
    $ownDenied=($report.own_post.result -match 'WM_LBUTTONDOWN error 5') -and (-not $report.own_post.reached_the_control)
    $workerDenied=($report.worker_post.reply -match 'Access is denied') -and (-not $report.worker_post.reached_the_control)
    $workerLands=($report.worker_post.reply -match '"ok":true') -and $report.worker_post.reached_the_control
    if($ExpectOptIn){
        $report.expectation='the window opted in: the medium post and the worker post both arrive'
        $report.pass=($report.own_post.reached_the_control) -and $workerLands -and $report.worker_sendinput.reached_the_control -and ($report.worker.uiaccess -eq 1)
    } elseif($TargetLevel -eq 'system') {
        $report.expectation='SYSTEM window, no opt-in: the medium post and the High UIAccess worker post are both denied (error 5); the worker SendInput click arrives'
        $report.pass=$ownDenied -and $workerDenied -and $report.worker_sendinput.reached_the_control -and ($report.worker.uiaccess -eq 1)
    } else {
        $report.expectation='High window, no opt-in: the medium post is denied (error 5); the worker (High) post arrives'
        $report.pass=$ownDenied -and $workerLands -and ($report.worker.uiaccess -eq 1)
    }
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
