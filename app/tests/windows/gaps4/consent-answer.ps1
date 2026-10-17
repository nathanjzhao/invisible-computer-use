param(
    # A copy of the signed worker under Program Files (cancel-run.ps1 -Action start-clicker makes it): the clicker must not hold the
    # installed worker open, or the installer cannot replace it and waits on an Abort/Retry/Ignore box.
    [string]$Dir='C:\Program Files\icu-clicker',
    [ValidateSet('escape','return','alt+y')][string]$Key='escape',
    [int]$Minutes=15,
    [string]$Log='C:\Users\Public\consent-answers.txt'
)
# Runs as the interactive user (cancel-run.ps1 -Action start-clicker starts it as a scheduled task) while the UAC policy
# has the consent prompt on the user's own desktop (PromptOnSecureDesktop 0, `cancel-run.ps1 -Action policy-nosecure`).
# The prompt is then an ordinary window of consent.exe, which runs as SYSTEM. When it appears this script answers it with
# a real key press delivered through the signed UIAccess worker (SendInput into a SYSTEM window is the one thing the worker
# can do there) and logs the prompt's window, a screenshot, and what the worker said. Escape is "No" on that dialog.
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
Add-Type @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class IcuConsent {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
    public static string[] Find() {
        var list = new List<string>();
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
            if (t.ToString() != "User Account Control") return true;
            var c = new StringBuilder(256); GetClassNameW(h, c, 256); RECT r; GetWindowRect(h, out r);
            list.Add((long)h + "|" + c + "|" + r.left + "|" + r.top + "|" + r.right + "|" + r.bottom); return true; }, IntPtr.Zero);
        return list.ToArray();
    }
}
'@
function Say([string]$m){ ((Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff')+' '+$m) | Add-Content $Log }
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
$workerExe=Join-Path $Dir 'cua-driver-uia.exe'
$worker=$null
$answered=0
Say "clicker start (user $env:USERNAME, key $Key)"
try {
    $end=(Get-Date).AddMinutes($Minutes)
    while((Get-Date) -lt $end){
        $found=@([IcuConsent]::Find())
        if($found.Count){
            $f=$found[0].Split('|'); $hwnd=[int64]$f[0]
            Start-Sleep -Milliseconds 700
            $n=++$answered
            try {
                $b=New-Object Drawing.Rectangle([int]$f[2],[int]$f[3],([int]$f[4]-[int]$f[2]),([int]$f[5]-[int]$f[3]))
                $bmp=New-Object Drawing.Bitmap($b.Width,$b.Height); $g=[Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size)
                $bmp.Save("C:\Users\Public\consent-$n.png",[Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
            } catch { Say "screenshot failed: $($_.Exception.Message)" }
            Say "prompt $n window $hwnd class $($f[1]) rect $($f[2]),$($f[3]),$($f[4]),$($f[5])"
            if(-not $worker -or $worker.HasExited){
                $worker=Start-Process $workerExe -ArgumentList '--authorized-parent-pid',$PID -PassThru -WindowStyle Hidden
                $d=(Get-Date).AddSeconds(10); while(-not ([IO.Directory]::GetFiles('\\.\pipe\') -contains '\\.\pipe\cua-driver-uia') -and (Get-Date) -lt $d){Start-Sleep -Milliseconds 100}
            }
            $snap=Worker-Send ('{"method":"snapshot","hwnd":'+$hwnd+',"max_elements":2000,"max_depth":25}')
            $nodes=$null; try { $nodes=@(($snap | ConvertFrom-Json).result.nodes).Count } catch {}
            Say "worker snapshot of the prompt: $nodes nodes"
            $parts=$Key.Split('+'); $keyName=$parts[-1]; $mods=@($parts | Select-Object -SkipLast 1 | ForEach-Object { '"'+$_+'"' }) -join ','
            $reply=Worker-Send ('{"method":"key","hwnd":'+$hwnd+',"key":"'+$keyName+'","modifiers":['+$mods+']}')
            Say "worker answered the prompt with $Key : $reply"
            Start-Sleep -Seconds 4
        }
        Start-Sleep -Milliseconds 200
    }
} finally {
    if($worker -and -not $worker.HasExited){ $worker.Kill() }
    Say "clicker end, answered $answered"
}
