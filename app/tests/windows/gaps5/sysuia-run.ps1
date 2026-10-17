param(
    [Parameter(Mandatory=$true)][string]$Driver,
    [string]$Apps='notepad,cmd,regedit,services,taskmgr',
    [string]$Out='C:\Users\Public\gaps5-sysuia.json'
)
# Run as SYSTEM (SSM). Starts gaps5\sysuia.ps1 with this process's SYSTEM token moved to the interactive session
# (CreateProcessAsUser), so the UIA client and the driver in it are SYSTEM integrity next to SYSTEM windows. Waits for the file.
$ErrorActionPreference='Stop'
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
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
        if (!CreateProcessAsUser(token, exe, "\"" + exe + "\" " + args, IntPtr.Zero, IntPtr.Zero, false, 0x10, IntPtr.Zero, null, ref si, out pi)) throw new System.ComponentModel.Win32Exception();
        CloseHandle(pi.hP); CloseHandle(pi.hT); CloseHandle(token); CloseHandle(self);
        return pi.pid;
    }
}
'@
Remove-Item $Out -ErrorAction SilentlyContinue
$ps=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$pidStarted=[IcuSession5]::StartInActiveSession($ps,"-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $here 'sysuia.ps1')`" -Driver `"$Driver`" -Apps `"$Apps`" -Out `"$Out`"")
$deadline=(Get-Date).AddMinutes(12)
while(-not (Test-Path $Out) -and (Get-Date) -lt $deadline){ Start-Sleep -Seconds 3 }
Start-Sleep -Seconds 1
try { Stop-Process -Id $pidStarted -Force -ErrorAction SilentlyContinue } catch {}
if(Test-Path $Out){ Get-Content $Out -Raw } else { 'no result' }
