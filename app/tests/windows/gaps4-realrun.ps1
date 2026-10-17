param(
    [Parameter(Mandatory=$true)][string]$Dir,
    [Parameter(Mandatory=$true)][ValidateSet('notepad','cmd','regedit','services','taskmgr')][string]$App,
    # normal: the control, the application runs at the daemon's own integrity (a limited-run-level task).
    # high:   the application runs elevated (High integrity) as the interactive user, through a highest-privilege
    #         scheduled task (ICUAudit must be a member of Administrators and logged on after that).
    # system: it runs as SYSTEM in the user's session (CreateProcessAsUser with this process's token).
    [Parameter(Mandatory=$true)][ValidateSet('high','system','normal')][string]$Target,
    [string]$Out=''
)
# Run as SYSTEM (SSM). Starts a REAL application at the requested integrity in the interactive session, runs
# gaps4-real.ps1 as the interactive user (normal integrity) against it, then closes it by pid and removes what it
# set up. Prints the user script's JSON result.
$ErrorActionPreference='Stop'
if(-not $Out){ $Out="C:\Users\Public\gaps4-real-$App-$Target.json" }
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuSession4 {
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
        if (!CreateProcessAsUser(token, exe, "\"" + exe + "\" " + args, IntPtr.Zero, IntPtr.Zero, false, 0x10, IntPtr.Zero, null, ref si, out pi)) throw new System.ComponentModel.Win32Exception();
        CloseHandle(pi.hP); CloseHandle(pi.hT); CloseHandle(token); CloseHandle(self);
        return pi.pid;
    }
}
'@
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$commands=@{
    notepad=@('C:\Windows\System32\notepad.exe','');
    cmd=@('C:\Windows\System32\cmd.exe','/k title ICU4CMD');
    regedit=@('C:\Windows\regedit.exe','');
    services=@('C:\Windows\System32\mmc.exe','C:\Windows\System32\services.msc');
    taskmgr=@('C:\Windows\System32\Taskmgr.exe','')
}
$exe=$commands[$App][0]; $arguments=$commands[$App][1]
# regedit reopens at its last key: start every run at the root.
if($App -eq 'regedit'){
    foreach($hive in 'Registry::HKEY_USERS\.DEFAULT','HKCU:'){ Remove-ItemProperty "$hive\Software\Microsoft\Windows\CurrentVersion\Applets\Regedit" -Name LastKey -ErrorAction SilentlyContinue }
    $sid=(Get-LocalUser ICUAudit).SID.Value
    Remove-ItemProperty "Registry::HKEY_USERS\$sid\Software\Microsoft\Windows\CurrentVersion\Applets\Regedit" -Name LastKey -ErrorAction SilentlyContinue
}
$taskName='ICUGaps4High'
$startedPid=0
if($Target -eq 'system'){
    $startedPid=[IcuSession4]::StartInActiveSession($exe,$arguments)
} else {
    $actionParameters=@{Execute=$exe}; if($arguments){ $actionParameters.Argument=$arguments }
    $action=New-ScheduledTaskAction @actionParameters
    $principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel $(if($Target -eq 'normal'){'Limited'}else{'Highest'})
    $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask $taskName
}
try {
    Start-Sleep -Seconds 4
    Remove-Item $Out -ErrorAction SilentlyContinue
    & (Join-Path $here 'as-user.ps1') -Script (Join-Path $here 'gaps4-real.ps1') -Arguments "-Dir '$Dir' -App $App -Target $Target -Out '$Out'" -TimeoutSeconds 400
} finally {
    $pidsToKill=@()
    if(Test-Path $Out){ try { $r=Get-Content $Out -Raw | ConvertFrom-Json; if($r.window.pid){ $pidsToKill+=[int]$r.window.pid } } catch {} }
    if($startedPid){ $pidsToKill+=$startedPid }
    foreach($p in ($pidsToKill | Sort-Object -Unique)){ taskkill /F /PID $p 2>&1 | Out-Null }
    if($Target -ne 'system'){ Unregister-ScheduledTask $taskName -Confirm:$false -ErrorAction SilentlyContinue }
    if($App -eq 'regedit'){
        Remove-Item 'HKCU:\ICU4KEY' -ErrorAction SilentlyContinue
        Remove-Item 'Registry::HKEY_USERS\.DEFAULT\ICU4KEY' -ErrorAction SilentlyContinue
    }
    # whatever the application left behind in this session (a conhost, a second window)
    Get-Process -Name $(switch($App){'notepad'{'notepad'}'cmd'{'conhost'}'regedit'{'regedit'}'services'{'mmc'}'taskmgr'{'Taskmgr'}}) -ErrorAction SilentlyContinue | Where-Object {$_.SessionId -ne 0} | ForEach-Object { if($App -ne 'cmd'){ Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue } }
}
