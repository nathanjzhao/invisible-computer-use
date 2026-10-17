param(
    [Parameter(Mandatory=$true)][string]$Exe,
    # uiaccess: the worker must start and its token must carry UIAccess.
    # none: the worker must not get UIAccess. Measured: unsigned or untrusted, Windows refuses to
    # start it ("A referral was returned from the server"); signed but outside a secure path, it
    # starts with TokenUIAccess 0. Both count; the report says which.
    [Parameter(Mandatory=$true)][ValidateSet('uiaccess','none')][string]$Expect,
    [string]$Out=''
)
# Start the UIAccess worker the way Windows requires (ShellExecuteEx: Start-Process without
# redirection) from this unelevated session, then read TokenUIAccess from its process token.
# A control process (PowerShell) is read the same way and must report 0, so a reader that always
# says 1 cannot pass. Run it as the interactive standard user (as-user.ps1), not SYSTEM.
$ErrorActionPreference='Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class IcuToken {
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int cls, IntPtr buffer, int length, out int returned);
    [DllImport("advapi32.dll")] static extern IntPtr GetSidSubAuthority(IntPtr sid, uint index);
    [DllImport("advapi32.dll")] static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);
    static IntPtr Query(int pid, int cls, out IntPtr token) {
        IntPtr process = OpenProcess(0x1000, false, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if (process == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "OpenProcess");
        try {
            if (!OpenProcessToken(process, 0x0008, out token)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "OpenProcessToken");
        } finally { CloseHandle(process); }
        int length;
        GetTokenInformation(token, cls, IntPtr.Zero, 0, out length);
        IntPtr buffer = Marshal.AllocHGlobal(length);
        if (!GetTokenInformation(token, cls, buffer, length, out length)) {
            int error = Marshal.GetLastWin32Error(); Marshal.FreeHGlobal(buffer); CloseHandle(token);
            throw new System.ComponentModel.Win32Exception(error, "GetTokenInformation");
        }
        return buffer;
    }
    public static int UIAccess(int pid) {
        IntPtr token; IntPtr buffer = Query(pid, 26, out token); // TokenUIAccess
        try { return Marshal.ReadInt32(buffer); } finally { Marshal.FreeHGlobal(buffer); CloseHandle(token); }
    }
    public static int Integrity(int pid) {
        IntPtr token; IntPtr buffer = Query(pid, 25, out token); // TokenIntegrityLevel
        try {
            IntPtr sid = Marshal.ReadIntPtr(buffer);
            int count = Marshal.ReadByte(GetSidSubAuthorityCount(sid));
            return Marshal.ReadInt32(GetSidSubAuthority(sid, (uint)(count - 1)));
        } finally { Marshal.FreeHGlobal(buffer); CloseHandle(token); }
    }
}
'@
$report=[ordered]@{exe=$Exe;expect=$Expect;user=$env:USERNAME;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;
    self_integrity=('0x{0:X}' -f [IcuToken]::Integrity($PID))}
$sig=Get-AuthenticodeSignature $Exe
$report.signature_status="$($sig.Status)"
$report.signer=if($sig.SignerCertificate){$sig.SignerCertificate.Subject}else{$null}
$report.exe_sha256=(Get-FileHash $Exe -Algorithm SHA256).Hash

$control=Start-Process powershell.exe -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 30' -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2
$report.control_uiaccess=[IcuToken]::UIAccess($control.Id)
Stop-Process -Id $control.Id -Force

$worker=$null
try {
    $worker=Start-Process -FilePath $Exe -ArgumentList '--authorized-parent-pid',$PID -PassThru -WindowStyle Hidden
} catch {
    $inner=$_.Exception
    while($inner.InnerException){$inner=$inner.InnerException}
    $report.launch_error=$_.Exception.Message
    if($inner -is [ComponentModel.Win32Exception]){$report.launch_win32_error=$inner.NativeErrorCode}
}
if($worker){
    Start-Sleep -Seconds 3
    $report.worker_pid=$worker.Id
    $report.worker_running=-not $worker.HasExited
    if($report.worker_running){
        $report.worker_uiaccess=[IcuToken]::UIAccess($worker.Id)
        $report.worker_integrity=('0x{0:X}' -f [IcuToken]::Integrity($worker.Id))
        $report.worker_path=(Get-Process -Id $worker.Id).Path
        $report.pipe_listening=[bool](@([IO.Directory]::GetFiles('\\.\pipe\')) -contains '\\.\pipe\cua-driver-uia')
        # A UIAccess process cannot be terminated from a plain medium-integrity one (measured:
        # Access is denied), so failing to stop it is itself a sign it runs with UIAccess. It
        # exits on its own when its authorized parent, this script, exits.
        try { Stop-Process -Id $worker.Id -Force -ErrorAction Stop; $report.worker_stopped_by_caller=$true }
        catch { $report.worker_stopped_by_caller=$false; $report.worker_stop_error=$_.Exception.Message }
    } else { $report.worker_exit_code=$worker.ExitCode }
}
$report.result=if(-not $worker){'refused'}elseif($report.worker_running -and $report.worker_uiaccess -eq 1){'uiaccess'}else{'started_without_uiaccess'}
$got=if($report.result -eq 'uiaccess'){'uiaccess'}else{'none'}
$report.pass=($report.control_uiaccess -eq 0) -and ($got -eq $Expect)
$json=$report | ConvertTo-Json
if($Out){$json | Set-Content -Encoding UTF8 $Out}
$json
if(-not $report.pass){exit 1}
