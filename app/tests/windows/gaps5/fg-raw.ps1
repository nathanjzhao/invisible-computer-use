param([string]$Out='',[string]$Label='')
# Run as the interactive standard user (as-user.ps1) with a normal-integrity Notepad already open (fg-run.ps1 -Start sched).
# No driver: what does Windows itself do when an unprivileged process asks for the foreground, and does it stay? Each method
# is tried from this process; a 2 ms recorder logs every change of the foreground window (with its root and owner) for 1.5 s.
$ErrorActionPreference='Stop'
Add-Type @'
using System; using System.Collections.Generic; using System.Text; using System.Runtime.InteropServices; using System.Threading; using System.Diagnostics;
public static class IcuRaw5 {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte sc, uint fl, UIntPtr ex);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b; }
    // the window at a point inside the target's rectangle (its menu bar row), and whether it is the target's root
    public static string AtMenuBar(IntPtr target) { RECT r; GetWindowRect(target, out r); POINT p; p.x = r.l + 28; p.y = r.t + 41; return "point (" + p.x + "," + p.y + ") in " + "(" + r.l + "," + r.t + ")-(" + r.r + "," + r.b + ") -> " + D(WindowFromPoint(p)); }
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    public static string D(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0"; uint pid; GetWindowThreadProcessId(h, out pid);
        var c = new StringBuilder(100); GetClassNameW(h, c, 100); var t = new StringBuilder(100); GetWindowTextW(h, t, 100);
        return "0x" + ((long)h).ToString("x") + " " + c + " '" + t + "' pid " + pid + " root 0x" + ((long)GetAncestor(h, 2)).ToString("x") + " parent 0x" + ((long)GetParent(h)).ToString("x");
    }
    static Thread th; static volatile bool stop; static List<string> log = new List<string>(); static Stopwatch sw;
    public static void Start() { stop = false; log = new List<string>(); sw = Stopwatch.StartNew();
        th = new Thread(() => { long last = -1; while (!stop) { long f = (long)GetForegroundWindow(); if (f != last) { lock (log) log.Add(sw.ElapsedMilliseconds + " ms: " + D((IntPtr)f)); last = f; } Thread.Sleep(2); } });
        th.IsBackground = true; th.Start(); }
    public static void Mark(string m) { lock (log) log.Add(sw.ElapsedMilliseconds + " ms: ---- " + m); }
    public static string[] Stop() { stop = true; th.Join(); lock (log) return log.ToArray(); }
    public static bool Attached(IntPtr target) {
        IntPtr cur = GetForegroundWindow(); uint pid; uint curTid = GetWindowThreadProcessId(cur, out pid); uint my = GetCurrentThreadId();
        bool a = curTid != 0 && curTid != my; if (a) AttachThreadInput(my, curTid, true);
        bool r = SetForegroundWindow(target); if (a) AttachThreadInput(my, curTid, false); return r;
    }
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint tid, ref GTI i);
    [StructLayout(LayoutKind.Sequential)] public struct GTI { public int cb, flags; public IntPtr a, f, c, mo, ms, ca; public int l, t, r, b; }
    // left click at the menu bar row of target (the File item), through the system input queue
    public static void ClickMenuBar(IntPtr target) { RECT r; GetWindowRect(target, out r); SetCursorPos(r.l + 28, r.t + 41); mouse_event(2, 0, 0, 0, UIntPtr.Zero); mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
    public static int ThreadFlags(IntPtr h) { uint pid; uint tid = GetWindowThreadProcessId(h, out pid); var i = new GTI(); i.cb = Marshal.SizeOf(i); GetGUIThreadInfo(tid, ref i); return i.flags; }
    public static void NoName() { keybd_event(0xFC, 0, 0, UIntPtr.Zero); keybd_event(0xFC, 0, 2, UIntPtr.Zero); }
}
'@
$np=@(Get-Process notepad -ErrorAction SilentlyContinue | Where-Object {$_.MainWindowHandle -ne 0})[0]
if(-not $np){ throw 'no notepad' }
$h=$np.MainWindowHandle
$rep=[ordered]@{label=$Label;notepad=[IcuRaw5]::D($h);foreground_start=[IcuRaw5]::D([IcuRaw5]::GetForegroundWindow())}
$rep.methods=@()
function Try-Method([string]$name,[scriptblock]$act){
    $con=[IcuRaw5]::GetForegroundWindow()
    [IcuRaw5]::Start(); [IcuRaw5]::Mark("BEGIN $name")
    $ret=& $act
    Start-Sleep -Milliseconds 1500
    [IcuRaw5]::Mark('END')
    $log=[IcuRaw5]::Stop()
    $final=[IcuRaw5]::D([IcuRaw5]::GetForegroundWindow())
    $under=[IcuRaw5]::AtMenuBar($h)
    # put the original foreground back for the next method
    [IcuRaw5]::Attached($con) | Out-Null; Start-Sleep -Milliseconds 400
    $script:rep.methods += [ordered]@{method=$name;returned=[string]$ret;final_foreground=$final;window_at_menu_bar_point=$under;log=@($log)}
}
Try-Method 'SetForegroundWindow plain' { [IcuRaw5]::SetForegroundWindow($h) }
Try-Method 'AttachThreadInput then SetForegroundWindow' { [IcuRaw5]::Attached($h) }
Try-Method 'no-name key then AttachThreadInput trick' { [IcuRaw5]::NoName(); [IcuRaw5]::Attached($h) }
# the driver's click sequence done by hand: take the foreground the way the driver does, click the File menu, look for the menu
Try-Method 'attach trick, then a real click on File, then wait' { [IcuRaw5]::Attached($h) | Out-Null; [IcuRaw5]::Mark('clicking'); [IcuRaw5]::ClickMenuBar($h); Start-Sleep -Milliseconds 300; [IcuRaw5]::Mark(('thread flags 0x{0:x}' -f [IcuRaw5]::ThreadFlags($h))); 'clicked' }
$json=$rep | ConvertTo-Json -Depth 6
if($Out){ $json | Set-Content -Encoding UTF8 $Out }
$json
