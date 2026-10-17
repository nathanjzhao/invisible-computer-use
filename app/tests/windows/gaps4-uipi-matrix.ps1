param(
    [string]$Out='C:\Users\Public\gaps4-uipi-matrix.json'
)
# Run as SYSTEM (SSM). Who may post a mouse message to whom? For a gesture fixture at SYSTEM and at High, with and
# without the window opting in to the mouse messages (ChangeWindowMessageFilterEx), a tiny poster (UipiPoster.exe)
# is started at normal integrity, at High and as SYSTEM in the interactive session and posts a left click to the
# control under the button. The fixture's own event file says whether it arrived; the poster's file says what
# PostMessage answered. (The UIAccess worker is a fourth sender, covered by gaps4-uipi.ps1.)
$ErrorActionPreference='Stop'
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuSession5 {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct STARTUPINFO { public int cb; public string r, d, t; public int x,y,w,h,xc,yc,fa,fl; public short sw, cb2; public IntPtr r2, si, so, se; }
    [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr hP, hT; public int pid, tid; }
    [DllImport("kernel32.dll")] static extern int WTSGetActiveConsoleSessionId();
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool DuplicateTokenEx(IntPtr t, uint a, IntPtr attrs, int imp, int type, out IntPtr n);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool SetTokenInformation(IntPtr t, int c, ref int v, int l);
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool CreateProcessAsUser(IntPtr t, string app, string cmd, IntPtr pa, IntPtr ta, bool inh, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    public static int StartInActiveSession(string exe, string args) {
        IntPtr self, token;
        if (!OpenProcessToken(GetCurrentProcess(), 0xF01FF, out self)) throw new System.ComponentModel.Win32Exception();
        if (!DuplicateTokenEx(self, 0x2000000, IntPtr.Zero, 2, 1, out token)) throw new System.ComponentModel.Win32Exception();
        int session = WTSGetActiveConsoleSessionId();
        if (!SetTokenInformation(token, 12, ref session, 4)) throw new System.ComponentModel.Win32Exception();
        var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si); si.d = "winsta0\\default";
        PROCESS_INFORMATION pi;
        if (!CreateProcessAsUser(token, exe, "\"" + exe + "\" " + args, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref si, out pi)) throw new System.ComponentModel.Win32Exception();
        CloseHandle(pi.hP); CloseHandle(pi.hT); CloseHandle(token); CloseHandle(self);
        return pi.pid;
    }
}
'@
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$root='C:\Users\Public\gaps4-matrix'
if(Test-Path $root){cmd /c "rd /s /q $root"}
New-Item -ItemType Directory $root | Out-Null
icacls $root /grant 'Users:(OI)(CI)M' | Out-Null
$fx="$root\UiaGestureFixture.exe"; $poster="$root\UipiPoster.exe"
& $csc /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll "/out:$fx" (Join-Path $here 'UiaGestureFixture.cs') | Out-Null
& $csc /nologo /target:winexe "/out:$poster" (Join-Path $here 'UipiPoster.cs') | Out-Null
if(-not ((Test-Path $fx) -and (Test-Path $poster))){ throw 'compile failed' }
function Start-As([string]$level,[string]$exe,[string]$arguments,[string]$task) {
    # returns the pid when it is known (system), 0 otherwise
    if($level -eq 'system'){ return [IcuSession5]::StartInActiveSession($exe,$arguments) }
    $principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel $(if($level -eq 'high'){'Highest'}else{'Limited'})
    $action=New-ScheduledTaskAction -Execute $exe -Argument $arguments
    $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 5) -AllowStartIfOnBatteries
    Register-ScheduledTask -TaskName $task -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask $task
    return 0
}
function Events([string]$dir){ if(Test-Path "$dir\events.txt"){ @((Get-Content "$dir\events.txt") | Where-Object {$_ -like 'mousedown*'}).Count } else { 0 } }
$results=@()
foreach($case in @(@('medium',''),@('system',''),@('system','allow-messages'),@('high',''),@('high','allow-messages'))){
    $targetLevel=$case[0]; $opt=$case[1]
    $dir="$root\t-$targetLevel-$(if($opt){'optin'}else{'default'})"
    New-Item -ItemType Directory $dir | Out-Null
    icacls $dir /grant 'Users:(OI)(CI)M' | Out-Null
    $fxPid=Start-As $targetLevel $fx ("`"$dir`" ICU-GESTURE-TARGET $opt") 'ICUMatrixFixture'
    $d=(Get-Date).AddSeconds(25); while(-not (Test-Path "$dir\target.ready") -and (Get-Date) -lt $d){ Start-Sleep -Milliseconds 200 }
    if(-not (Test-Path "$dir\target.ready")){ throw "fixture ($targetLevel $opt) did not appear" }
    $targetPid=[int](Get-Content "$dir\target.ready").Split(' ')[1]
    Start-Sleep -Seconds 1
    foreach($sender in 'medium','high','system'){
        $before=Events $dir
        $null=Start-As $sender $poster ("`"$dir`" $sender") 'ICUMatrixPoster'
        $d=(Get-Date).AddSeconds(20); while(-not (Test-Path "$dir\poster-$sender.json") -and (Get-Date) -lt $d){ Start-Sleep -Milliseconds 200 }
        Start-Sleep -Milliseconds 800
        $answer=$(if(Test-Path "$dir\poster-$sender.json"){ Get-Content "$dir\poster-$sender.json" -Raw | ConvertFrom-Json } else { $null })
        $results+=[ordered]@{target=$targetLevel;target_opted_in=[bool]$opt;sender=$sender;sender_integrity=$answer.integrity;post_down=$answer.down;post_up=$answer.up;arrived=((Events $dir) -gt $before)}
        foreach($t in 'ICUMatrixPoster'){ Unregister-ScheduledTask $t -Confirm:$false -ErrorAction SilentlyContinue }
    }
    Stop-Process -Id $targetPid -Force -ErrorAction SilentlyContinue
    Unregister-ScheduledTask ICUMatrixFixture -Confirm:$false -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}
$results | ForEach-Object { '{0,-7} target  opted-in={1,-5}  sender {2,-6} ({3})  WM_LBUTTONDOWN {4,-9} WM_LBUTTONUP {5,-9}  arrived={6}' -f $_.target,$_.target_opted_in,$_.sender,$_.sender_integrity,$_.post_down,$_.post_up,$_.arrived }
$results | ConvertTo-Json -Depth 4 | Set-Content $Out -Encoding UTF8
