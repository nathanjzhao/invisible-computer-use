param(
    [Parameter(Mandatory=$true)][string]$Dir,
    # system: the target window runs as SYSTEM in the user's session.
    # high:   it runs elevated (High integrity) as the interactive user, through a highest-privilege
    #         scheduled task. That needs the user to be a member of Administrators (a filtered
    #         token at normal integrity, which is what the driver then runs as) and logged on
    #         after the membership was added.
    [Parameter(Mandatory=$true)][ValidateSet('system','high')][string]$Target,
    [ValidateSet('forwarded','refused')][string]$Expect='forwarded',
    [string]$Only='',
    # Names (comma separated) that must refuse; defaults to what a SYSTEM window makes impossible.
    [string]$MustRefuse='default',
    # The user-side script to run against the fixture (gaps3-forward.ps1, or gaps3-sysprobe.ps1).
    [string]$UserScript='gaps3-forward.ps1',
    [string]$Out='C:\Users\Public\gaps3-result.json',
    # Extra fixture arguments after the directory, e.g. 'ICU-GESTURE-TARGET allow-messages'.
    [string]$FixtureArgs='',
    # gaps4-uipi.ps1 expects the posts to land when the fixture opted in (allow-messages). -WrongExpectation says the opposite on purpose: a guard check.
    [switch]$WrongExpectation
)
# Run as SYSTEM (SSM). Starts the gesture fixture at the requested integrity, then runs
# gaps3-forward.ps1 as the interactive user (normal integrity) against it and prints the result.
$ErrorActionPreference='Stop'
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuSession3 {
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
$work='C:\Users\Public\gaps3-external'
if(Test-Path $work){cmd /c "rd /s /q $work"}
New-Item -ItemType Directory $work | Out-Null
icacls $work /grant 'Users:(OI)(CI)M' | Out-Null
$fx=Join-Path $work 'UiaGestureFixture.exe'
& 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll "/out:$fx" (Join-Path $here 'UiaGestureFixture.cs') | Out-Null
$fixturePid=0
if($Target -eq 'system'){
    $fixturePid=[IcuSession3]::StartInActiveSession($fx,"`"$work`" $FixtureArgs")
} else {
    $action=New-ScheduledTaskAction -Execute $fx -Argument "`"$work`" $FixtureArgs"
    $principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel Highest
    $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries
    Register-ScheduledTask -TaskName ICUGaps3High -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask ICUGaps3High
}
try {
    $deadline=(Get-Date).AddSeconds(20)
    while(-not (Test-Path "$work\target.ready") -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 200}
    if(-not (Test-Path "$work\target.ready")){throw "$Target target window did not appear in the user session"}
    $only=$(if($Only){"-Only '$Only'"}else{''})
    # drag_px_bg: a posted drag stream is denied by UIPI above the daemon's integrity, High or SYSTEM.
    if($MustRefuse -eq 'default'){ $MustRefuse=$(if($Target -eq 'system'){'click_el_bg,click_px_bg,scroll_el_bg,scroll_win_bg,scroll_px_bg,type_el_bg,setvalue_el,scroll_px_bg_decoy,drag_px_bg'}else{'drag_px_bg'}) }
    if($MustRefuse){ $only="$only -MustRefuse '$MustRefuse'" }
    if($UserScript -in @('gaps3-sysprobe.ps1','gaps4-uipi.ps1')){
        & (Join-Path $here 'as-user.ps1') -Script (Join-Path $here $UserScript) -Arguments ("-Dir '$Dir' -ExternalDir '$work' -Out '$Out'"+$(if($UserScript -eq 'gaps4-uipi.ps1' -and (($FixtureArgs -match 'allow-messages') -xor $WrongExpectation)){' -ExpectOptIn'}else{''})+$(if($UserScript -eq 'gaps4-uipi.ps1'){" -TargetLevel $Target"}else{''})) -TimeoutSeconds 300
    } else {
        & (Join-Path $here 'as-user.ps1') -Script (Join-Path $here $UserScript) -Arguments "-Dir '$Dir' -Case external -Expect $Expect -ExternalDir '$work' $only -Out '$Out'" -TimeoutSeconds 300
    }
} finally {
    if($fixturePid){ Stop-Process -Id $fixturePid -Force -ErrorAction SilentlyContinue }
    if($Target -eq 'high'){
        $ready=Get-Content "$work\target.ready" -ErrorAction SilentlyContinue
        if($ready){ taskkill /F /PID ($ready.Split(' ')[1]) 2>&1 | Out-Null }
        Unregister-ScheduledTask ICUGaps3High -Confirm:$false -ErrorAction SilentlyContinue
    }
}
