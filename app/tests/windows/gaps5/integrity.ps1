param([Parameter(Mandatory=$true)][string]$Name)
# Run as SYSTEM (SSM). Integrity level, owner and start time of every process with this name.
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuInt5 {
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, byte[] v, int l, out int r);
    public static int Integrity(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int need; GetTokenInformation(t, 25, null, 0, out need);
        byte[] b = new byte[need]; int got;
        bool ok = GetTokenInformation(t, 25, b, need, out got); CloseHandle(t); CloseHandle(p);
        if (!ok) return -1;
        IntPtr sid = (IntPtr)(Environment.Is64BitProcess ? BitConverter.ToInt64(b, 0) : BitConverter.ToInt32(b, 0));
        return Marshal.ReadInt32(sid, 8 + 4 * (Marshal.ReadByte(sid, 1) - 1));
    }
}
'@
foreach($p in @(Get-Process -Name $Name -ErrorAction SilentlyContinue)){
    $owner=(Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)").GetOwner()
    '{0} pid {1} integrity 0x{2:x} owner {3}\{4} started {5} path {6}' -f $p.Name,$p.Id,[IcuInt5]::Integrity($p.Id),$owner.Domain,$owner.User,$p.StartTime,$p.Path
}
