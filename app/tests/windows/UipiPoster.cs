// UipiPoster.exe <dir>: posts a left click (WM_LBUTTONDOWN, WM_LBUTTONUP) to the control under the
// UiaGestureFixture button whose files are in <dir>, and writes what Windows answered to
// <dir>\poster-<label>.json ("ok" or the Win32 error per message, the sender's integrity RID).
// gaps4-uipi-matrix.ps1 starts it at normal integrity, at High and as SYSTEM to show who may post to whom.
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;

static class UipiPoster
{
    [StructLayout(LayoutKind.Sequential)] struct PT { public int x, y; }
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(PT p);
    [DllImport("user32.dll", SetLastError = true)] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool ScreenToClient(IntPtr h, ref PT p);
    [DllImport("user32.dll")] static extern IntPtr ChildWindowFromPointEx(IntPtr parent, PT p, uint flags);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool GetTokenInformation(IntPtr t, int c, byte[] v, int l, out int r);

    static int Integrity()
    {
        IntPtr t; OpenProcessToken(GetCurrentProcess(), 8, out t);
        int need; GetTokenInformation(t, 25, null, 0, out need);
        byte[] b = new byte[need]; int got; GetTokenInformation(t, 25, b, need, out got);
        IntPtr sid = (IntPtr)(Environment.Is64BitProcess ? BitConverter.ToInt64(b, 0) : BitConverter.ToInt32(b, 0));
        return Marshal.ReadInt32(sid, 8 + 4 * (Marshal.ReadByte(sid, 1) - 1));
    }

    static int Main(string[] args)
    {
        string dir = args[0];
        string label = args.Length > 1 ? args[1] : "x";
        string layout = File.ReadAllText(Path.Combine(dir, "layout.json"));
        var m = Regex.Match(layout, "\"button\":\\[(\\d+),(\\d+)\\]");
        PT p; p.x = int.Parse(m.Groups[1].Value); p.y = int.Parse(m.Groups[2].Value);
        // The control under the button's centre inside the fixture's own window (a window of another process that happens to
        // cover that point, such as a console, must not be what gets the post): walk down from the fixture's top-level window.
        IntPtr control = (IntPtr)long.Parse(File.ReadAllText(Path.Combine(dir, "target.ready")).Split(' ')[0]);
        for (int depth = 0; depth < 8; depth++)
        {
            PT local = p; ScreenToClient(control, ref local);
            IntPtr child = ChildWindowFromPointEx(control, local, 0);
            if (child == IntPtr.Zero || child == control) break;
            control = child;
        }
        PT c = p; ScreenToClient(control, ref c);
        IntPtr lp = (IntPtr)((c.y << 16) | (c.x & 0xFFFF));
        string down = PostMessageW(control, 0x201, (IntPtr)1, lp) ? "ok" : "error " + Marshal.GetLastWin32Error();
        string up = PostMessageW(control, 0x202, IntPtr.Zero, lp) ? "ok" : "error " + Marshal.GetLastWin32Error();
        File.WriteAllText(Path.Combine(dir, "poster-" + label + ".json"),
            "{\"integrity\":\"0x" + Integrity().ToString("x") + "\",\"control\":" + (long)control + ",\"down\":\"" + down + "\",\"up\":\"" + up + "\"}");
        return 0;
    }
}
