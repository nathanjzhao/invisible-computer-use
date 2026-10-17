param(
    [Parameter(Mandatory=$true)][string]$Dir,
    [Parameter(Mandatory=$true)][ValidateSet('elevated','outside','impostor')][string]$Case,
    [ValidateSet('forwarded','refused')][string]$Expect='forwarded',
    [string]$Out='C:\Users\Public\uia-forward-result.json'
)
# Run as SYSTEM in a scheduled task (as-system.ps1). Azure command jobs can prevent
# cross-session process creation even after assigned privileges are enabled.
# Starts the uia-forward target window at SYSTEM
# integrity in the interactive user's session, so a standard user's driver cannot send it input
# (UIPI), then runs uia-forward.ps1 as that standard user against it and prints the result.
$ErrorActionPreference='Stop'
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class IcuSession {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct STARTUPINFO { public int cb; public string r, d, t; public int x,y,w,h,xc,yc,fa,fl; public short sw, cb2; public IntPtr r2, si, so, se; }
    [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr hP, hT; public int pid, tid; }
    [DllImport("kernel32.dll")] static extern int WTSGetActiveConsoleSessionId();
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool DuplicateTokenEx(IntPtr t, uint a, IntPtr attrs, int imp, int type, out IntPtr n);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool SetTokenInformation(IntPtr t, int c, ref int v, int l);
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool CreateProcessAsUser(IntPtr t, string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inh, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    [StructLayout(LayoutKind.Sequential)] struct LUID { public uint Low; public int High; }
    [StructLayout(LayoutKind.Sequential)] struct TOKEN_PRIVILEGES { public uint Count; public LUID Luid; public uint Attributes; }
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool LookupPrivilegeValue(string system, string name, out LUID luid);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr token, bool disable, ref TOKEN_PRIVILEGES privileges, uint size, IntPtr previous, IntPtr returned);
    static void Enable(string name) {
        IntPtr token; if (!OpenProcessToken(GetCurrentProcess(), 0x28, out token)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "OpenProcessToken for "+name);
        try {
            LUID luid; if (!LookupPrivilegeValue(null,name,out luid)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "LookupPrivilegeValue "+name);
            var privileges=new TOKEN_PRIVILEGES{Count=1,Luid=luid,Attributes=2};
            if (!AdjustTokenPrivileges(token,false,ref privileges,0,IntPtr.Zero,IntPtr.Zero) || Marshal.GetLastWin32Error()!=0) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Enable "+name);
        } finally { CloseHandle(token); }
    }
    public static int StartInActiveSession(string exe, string args) {
        // Azure's SYSTEM command host does not enable these inherited privileges.
        // Enable only the rights already assigned to this disposable-host helper.
        Enable("SeTcbPrivilege"); Enable("SeAssignPrimaryTokenPrivilege"); Enable("SeIncreaseQuotaPrivilege");
        IntPtr self=IntPtr.Zero, token=IntPtr.Zero;
        try {
        if (!OpenProcessToken(GetCurrentProcess(), 0xF01FF, out self)) throw new System.ComponentModel.Win32Exception();
        if (!DuplicateTokenEx(self, 0x2000000, IntPtr.Zero, 2, 1, out token)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "DuplicateTokenEx");
        int session = WTSGetActiveConsoleSessionId();
        if (!SetTokenInformation(token, 12, ref session, 4)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "SetTokenInformation session");
        var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si); si.d = "winsta0\\default";
        PROCESS_INFORMATION pi;
        if (!CreateProcessAsUser(token, exe, new StringBuilder("\"" + exe + "\" " + args), IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref si, out pi)) { int error=Marshal.GetLastWin32Error(); throw new System.ComponentModel.Win32Exception(error, "CreateProcessAsUser error="+error+" session="+session); }
        CloseHandle(pi.hP); CloseHandle(pi.hT);
        return pi.pid;
        } finally { if(token!=IntPtr.Zero)CloseHandle(token); if(self!=IntPtr.Zero)CloseHandle(self); }
    }
}
'@
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$work='C:\Users\Public\uia-forward-external'
if(Test-Path $work){cmd /c "rd /s /q $work"}
New-Item -ItemType Directory $work | Out-Null
$fx=Join-Path $work 'UiaForwardFixture.exe'
& 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll "/out:$fx" (Join-Path $here 'UiaForwardFixture.cs') | Out-Null
$fixturePid=[IcuSession]::StartInActiveSession($fx,"target `"$work`"")
try {
    $targetProcess=Get-CimInstance Win32_Process -Filter "ProcessId=$fixturePid"
    $targetOwner=Invoke-CimMethod -InputObject $targetProcess -MethodName GetOwner
    @{pid=$fixturePid;session_id=$targetProcess.SessionId;owner=$targetOwner.User;owner_domain=$targetOwner.Domain;launcher=[Security.Principal.WindowsIdentity]::GetCurrent().Name}|ConvertTo-Json|Set-Content ($Out+'.target.json')
    $deadline=(Get-Date).AddSeconds(15)
    while(-not (Test-Path "$work\target.ready") -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 200}
    if(-not (Test-Path "$work\target.ready")){throw 'SYSTEM target window did not appear in the user session'}
    # The user's driver must be able to read what the target writes.
    icacls $work /grant 'Users:(OI)(CI)RX' | Out-Null
    & (Join-Path $here 'as-user.ps1') -Script (Join-Path $here 'uia-forward.ps1') -Arguments "-Dir '$Dir' -Case $Case -Expect $Expect -ExternalDir '$work' -Out '$Out'" -TimeoutSeconds 180
} finally {
    Stop-Process -Id $fixturePid -Force -ErrorAction SilentlyContinue
}
