param(
    [Parameter(Mandatory=$true)][ValidateSet('notepad','cmd','regedit','services','taskmgr')][string]$App,
    [Parameter(Mandatory=$true)][ValidateSet('high','system')][string]$Target,
    [string]$Out='C:\Users\Public\gaps4-uiaraw.json'
)
# Run as SYSTEM (SSM) with ICUAudit logged on and a member of Administrators. What does plain UI Automation give for a REAL
# application above the daemon's integrity? Starts the application at $Target (as gaps4-realrun.ps1 does) and reads its UIA tree with
# a client at normal integrity and with a client at High integrity (a highest-run-level task of the same user: the integrity a UIAccess
# process runs at). Counts descendants, the control types and a sample of names. Compare with the daemon's own get_window_state
# (gaps4-real.ps1), which describes a SYSTEM window's child windows when UIA gives less.
$ErrorActionPreference='Stop'
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$commands=@{ notepad=@('C:\Windows\System32\notepad.exe','','Notepad$'); cmd=@('C:\Windows\System32\cmd.exe','/k title ICU4CMD','ICU4CMD|cmd\.exe'); regedit=@('C:\Windows\regedit.exe','','Registry Editor');
             services=@('C:\Windows\System32\mmc.exe','C:\Windows\System32\services.msc','^Services'); taskmgr=@('C:\Windows\System32\Taskmgr.exe','','Task Manager') }
$exe=$commands[$App][0]; $arguments=$commands[$App][1]; $titleRegex=$commands[$App][2]
# the user-side counter, written next to this script's results
$counter='C:\Users\Public\gaps4-uiacount.ps1'
@'
param([string]$TitleRegex,[string]$Out)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$root=[Windows.Automation.AutomationElement]::RootElement
$rep=[ordered]@{client_integrity=(whoami /groups | Select-String 'Mandatory Label').ToString().Trim()}
$win=$null
foreach($w in $root.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)){ try{ if($w.Current.Name -match $TitleRegex){ $win=$w; break } }catch{} }
if(-not $win){ $rep.error='window not found'; ($rep | ConvertTo-Json) | Set-Content $Out -Encoding UTF8; exit 1 }
$rep.window=$win.Current.Name
$sw=[Diagnostics.Stopwatch]::StartNew()
$nodes=@($win.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition))
$rep.descendants=$nodes.Count; $rep.seconds=[Math]::Round($sw.Elapsed.TotalSeconds,1)
$types=@{}; foreach($n in $nodes){ try{ $t=$n.Current.ControlType.ProgrammaticName -replace '^ControlType\.',''; $types[$t]=1+[int]$types[$t] }catch{} }
$rep.control_types=$types
$rep.sample=@($nodes | Select-Object -First 25 | ForEach-Object { try{ ($_.Current.ControlType.ProgrammaticName -replace '^ControlType\.','')+' '+$_.Current.Name }catch{ '?' } })
($rep | ConvertTo-Json -Depth 4) | Set-Content $Out -Encoding UTF8
'@ | Set-Content $counter -Encoding UTF8
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuSession6 {
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
function Run-Client([string]$level,[string]$file) {
    Remove-Item $file -ErrorAction SilentlyContinue
    $principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel $(if($level -eq 'high'){'Highest'}else{'Limited'})
    $action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ("-NoProfile -ExecutionPolicy Bypass -File `"$counter`" -TitleRegex `"$titleRegex`" -Out `"$file`"")
    Register-ScheduledTask -TaskName ICUUiaRaw -Action $action -Principal $principal -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 3) -AllowStartIfOnBatteries) -Force | Out-Null
    Start-ScheduledTask ICUUiaRaw
    $d=(Get-Date).AddSeconds(150); while(-not (Test-Path $file) -and (Get-Date) -lt $d){ Start-Sleep -Seconds 2 }
    Start-Sleep -Milliseconds 500
    Unregister-ScheduledTask ICUUiaRaw -Confirm:$false -ErrorAction SilentlyContinue
    if(Test-Path $file){ Get-Content $file -Raw | ConvertFrom-Json } else { [pscustomobject]@{error='no result'} }
}
$startedPid=0; $taskName='ICUUiaRawApp'
if($Target -eq 'system'){ $startedPid=[IcuSession6]::StartInActiveSession($exe,$arguments) }
else {
    $ap=@{Execute=$exe}; if($arguments){ $ap.Argument=$arguments }
    Register-ScheduledTask -TaskName $taskName -Action (New-ScheduledTaskAction @ap) -Principal (New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel Highest) -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries) -Force | Out-Null
    Start-ScheduledTask $taskName
}
try {
    Start-Sleep -Seconds 5
    $result=[ordered]@{app=$App;target=$Target}
    $result.client_normal=Run-Client 'normal' 'C:\Users\Public\gaps4-uiacount-normal.json'
    $result.client_high=Run-Client 'high' 'C:\Users\Public\gaps4-uiacount-high.json'
    ($result | ConvertTo-Json -Depth 6) | Set-Content $Out -Encoding UTF8
    $result | ConvertTo-Json -Depth 3 -Compress
} finally {
    if($startedPid){ taskkill /F /PID $startedPid 2>&1 | Out-Null }
    Get-Process -Name $(switch($App){'notepad'{'notepad'}'cmd'{'cmd'}'regedit'{'regedit'}'services'{'mmc'}'taskmgr'{'Taskmgr'}}) -ErrorAction SilentlyContinue | Where-Object {$_.SessionId -ne 0} | Stop-Process -Force -ErrorAction SilentlyContinue
    Unregister-ScheduledTask $taskName -Confirm:$false -ErrorAction SilentlyContinue
}
