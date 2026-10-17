using System;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;

// Adapted from the 2026-09-25 review probe ReviewCanvas.cs. The same 3x3
// colour grid as DpiFixture, but with no
// UIA-invokable buttons, so the driver's pixel click cannot take the UIA
// hit-test route and must post mouse messages. Every mouse message is logged
// with the button, the client point the window received, and the cell under
// that point.
//   canvas: one HWND, the grid is painted; the logged cell comes from the
//           client coordinates in the message.
//   panels: one child HWND per cell; the logged cell is the panel that got it,
//           plus the panel-local point.
//   hosted: mixed-DPI hosting. The form has <mode>'s awareness and hosts a
//           child container of the other awareness (pmv2 hosts unaware,
//           unaware hosts pmv2) holding one raw child HWND per cell. The
//           form and the container log what they receive as cells "form"
//           and "container".
//   lists:  two list boxes of 300 items, left (colour A) and right (colour
//           C); every change of either one's top item is logged as a "top"
//           event.
//   reuse:  a raw Win32 canvas. When panel-reuse-go-<label> appears it
//           destroys its window and creates windows until one gets the same
//           HWND back, then paints the grid in reverse and logs clicks by the
//           painted cell (adapted from the 2026-09-26 review's R2Fixture).
// Usage: ICUPanelFixture.exe <pmv2|unaware> <canvas|panels|hosted|lists|reuse> <label> <x> <y> <work>
public static class PanelFixture {
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiHostingBehavior(IntPtr behavior);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    public static readonly Color[] Colors = {
        Color.FromArgb(230,25,75), Color.FromArgb(60,180,75), Color.FromArgb(0,130,200),
        Color.FromArgb(245,130,48), Color.FromArgb(145,30,180), Color.FromArgb(70,240,240),
        Color.FromArgb(240,50,230), Color.FromArgb(210,245,60), Color.FromArgb(128,0,0) };
    static string Cell(Point p) {
        for(int i=0;i<9;i++) {
            var r=new Rectangle(10+(i%3)*105,10+(i/3)*65,100,60);
            if(r.Contains(p)) return ((char)('A'+i)).ToString();
        }
        return "-";
    }

    [STAThread] public static void Main(string[] args) {
        string mode=args[0], shape=args[1], label=args[2], work=args[5];
        if(mode=="pmv2" && !SetProcessDpiAwarenessContext(new IntPtr(-4))) throw new Exception("PMv2 unavailable");
        // A child of another awareness needs mixed hosting on the thread that
        // creates the top-level window (DPI_HOSTING_BEHAVIOR_MIXED).
        if(shape=="hosted" && SetThreadDpiHostingBehavior(new IntPtr(1))==new IntPtr(-1)) throw new Exception("mixed hosting unavailable");
        Directory.CreateDirectory(work);
        string log=Path.Combine(work,"panel-events-"+label+".jsonl");
        if(shape=="reuse") { Environment.Exit(Reuse.Run(int.Parse(args[3]),int.Parse(args[4]),label,mode,work,log)); }
        var form=new Form { Text="ICU Panels "+label, StartPosition=FormStartPosition.Manual,
            Location=new Point(int.Parse(args[3]),int.Parse(args[4])), ClientSize=shape=="hosted"?new Size(520,340):new Size(330,210),
            FormBorderStyle=FormBorderStyle.FixedSingle, MaximizeBox=false, AutoScaleMode=AutoScaleMode.None,
            BackColor=Color.White };
        Action<string,string,MouseEventArgs,string> write=(kind,cell,e,where)=>File.AppendAllText(log,Json.Serialize(new {
            window=label, kind, button=e.Button.ToString(), cell, where, x=e.X, y=e.Y, clicks=e.Clicks,
            dpi=GetDpiForWindow(form.Handle), foreground=GetForegroundWindow().ToInt64(),
            time=Stopwatch.GetTimestamp() })+Environment.NewLine);
        if(shape=="canvas") {
            form.Paint+=(s,e)=>{ for(int i=0;i<9;i++) using(var b=new SolidBrush(Colors[i]))
                e.Graphics.FillRectangle(b,10+(i%3)*105,10+(i/3)*65,100,60); };
            form.MouseDown+=(s,e)=>write("down",Cell(e.Location),e,"form");
            form.MouseDoubleClick+=(s,e)=>write("double",Cell(e.Location),e,"form");
        } else if(shape=="hosted") {
            form.MouseDown+=(s,e)=>write("down","form",e,"form");
            form.MouseDoubleClick+=(s,e)=>write("double","form",e,"form");
            form.HandleCreated+=(s,e)=>{
                // -1 unaware, -4 PMv2: the container takes the awareness the form does not have.
                IntPtr previous=SetThreadDpiAwarenessContext(new IntPtr(mode=="pmv2"?-1:-4));
                if(previous==IntPtr.Zero) throw new Exception("thread DPI context unavailable");
                try { Hosted.Build(form.Handle,(kind,cell,button,x,y)=>File.AppendAllText(log,Json.Serialize(new {
                    window=label, kind, button, cell, where=cell=="container"?"container":"hosted", x, y, clicks=kind=="double"?2:1,
                    dpi=GetDpiForWindow(form.Handle), foreground=GetForegroundWindow().ToInt64(),
                    time=Stopwatch.GetTimestamp() })+Environment.NewLine)); }
                finally { SetThreadDpiAwarenessContext(previous); }
            };
        } else if(shape=="lists") {
            var left=new ListBox { Location=new Point(10,10), Size=new Size(150,190), BackColor=Colors[0], ForeColor=Colors[0], IntegralHeight=false, BorderStyle=BorderStyle.None };
            var right=new ListBox { Location=new Point(170,10), Size=new Size(150,190), BackColor=Colors[2], ForeColor=Colors[2], IntegralHeight=false, BorderStyle=BorderStyle.None };
            for(int i=0;i<300;i++){ left.Items.Add("L"+i); right.Items.Add("R"+i); }
            form.Controls.Add(left); form.Controls.Add(right);
            int lastLeft=0,lastRight=0;
            var timer=new System.Windows.Forms.Timer { Interval=50 };
            timer.Tick+=(s,e)=>{
                if(left.TopIndex==lastLeft && right.TopIndex==lastRight) return;
                lastLeft=left.TopIndex; lastRight=right.TopIndex;
                File.AppendAllText(log,Json.Serialize(new { window=label, kind="top", left=lastLeft, right=lastRight,
                    foreground=GetForegroundWindow().ToInt64(), time=Stopwatch.GetTimestamp() })+Environment.NewLine);
            };
            timer.Start();
        } else {
            for(int i=0;i<9;i++) {
                string name=((char)('A'+i)).ToString();
                var panel=new Panel { BackColor=Colors[i], Location=new Point(10+(i%3)*105,10+(i/3)*65), Size=new Size(100,60) };
                panel.MouseDown+=(s,e)=>write("down",name,e,"panel");
                panel.MouseDoubleClick+=(s,e)=>write("double",name,e,"panel");
                form.Controls.Add(panel);
            }
        }
        form.Shown+=(s,e)=>File.WriteAllText(Path.Combine(work,"panel-ready-"+label+".json"),Json.Serialize(new {
            pid=Process.GetCurrentProcess().Id, window=form.Handle.ToInt64(), mode, shape, dpi=GetDpiForWindow(form.Handle) }));
        Application.Run(form);
    }
}

// Raw Win32 container and cells for the hosted shape: WinForms would create
// the child handles later, outside the thread DPI context that gives them
// their awareness. Same grid as the other shapes, in the container's units.
static class Hosted {
    delegate IntPtr WndProcFn(IntPtr h, uint m, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct WNDCLASSEX {
        public uint cbSize, style; public WndProcFn lpfnWndProc; public int cbClsExtra, cbWndExtra;
        public IntPtr hInstance, hIcon, hCursor, hbrBackground; public string lpszMenuName, lpszClassName; public IntPtr hIconSm; }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int left, top, right, bottom; }
    [StructLayout(LayoutKind.Sequential)] struct PAINTSTRUCT { public IntPtr hdc; public int fErase; public RECT rc; public int fRestore, fIncUpdate; [MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)] public byte[] r; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern ushort RegisterClassExW(ref WNDCLASSEX c);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateWindowExW(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr p);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProcW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr BeginPaint(IntPtr h, out PAINTSTRUCT p);
    [DllImport("user32.dll")] static extern bool EndPaint(IntPtr h, ref PAINTSTRUCT p);
    [DllImport("user32.dll")] static extern int FillRect(IntPtr dc, ref RECT r, IntPtr brush);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr h);
    [DllImport("gdi32.dll")] static extern IntPtr CreateSolidBrush(int color);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string n);
    static readonly WndProcFn proc = Proc;
    static readonly IntPtr[] brushes = new IntPtr[10];
    static Action<string,string,string,int,int> log;
    static IntPtr Proc(IntPtr h, uint m, IntPtr w, IntPtr l) {
        int id = GetDlgCtrlID(h);
        if (m == 0x000F) {
            PAINTSTRUCT ps; IntPtr dc = BeginPaint(h, out ps); RECT r; GetClientRect(h, out r);
            FillRect(dc, ref r, brushes[id >= 1 && id <= 9 ? id : 0]); EndPaint(h, ref ps); return IntPtr.Zero;
        }
        if (m == 0x0201 || m == 0x0203 || m == 0x0204 || m == 0x0206) {
            int x = (short)(l.ToInt64() & 0xFFFF), y = (short)((l.ToInt64() >> 16) & 0xFFFF);
            string cell = id >= 1 && id <= 9 ? ((char)('A' + id - 1)).ToString() : "container";
            log(m == 0x0203 || m == 0x0206 ? "double" : "down", cell, m <= 0x0203 ? "Left" : "Right", x, y);
            return IntPtr.Zero;
        }
        return DefWindowProcW(h, m, w, l);
    }
    public static void Build(IntPtr parent, Action<string,string,string,int,int> write) {
        log = write;
        brushes[0] = CreateSolidBrush(0xFFFFFF);
        for (int i = 0; i < 9; i++) { var c = PanelFixture.Colors[i]; brushes[i + 1] = CreateSolidBrush(c.R | (c.G << 8) | (c.B << 16)); }
        var wc = new WNDCLASSEX { cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX)), style = 0x0008 /* CS_DBLCLKS */,
            lpfnWndProc = proc, hInstance = GetModuleHandleW(null), lpszClassName = "ICUHostedCell" };
        if (RegisterClassExW(ref wc) == 0) throw new Exception("RegisterClassEx failed");
        const uint child = 0x40000000 | 0x10000000; // WS_CHILD | WS_VISIBLE
        IntPtr container = CreateWindowExW(0, "ICUHostedCell", "container", child, 5, 5, 330, 210, parent, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
        if (container == IntPtr.Zero) throw new Exception("container creation failed");
        for (int i = 0; i < 9; i++)
            if (CreateWindowExW(0, "ICUHostedCell", ((char)('A' + i)).ToString(), child, 10 + (i % 3) * 105, 10 + (i / 3) * 65, 100, 60,
                    container, new IntPtr(i + 1), wc.hInstance, IntPtr.Zero) == IntPtr.Zero) throw new Exception("cell creation failed");
    }
}

// Raw Win32 so no framework window is created between the destroy and the
// re-create. The recreated window paints cell i in colour 8-i, so a click at
// the old pixel of C lands on the cell now painted G.
static class Reuse {
    delegate IntPtr WndProcFn(IntPtr h, uint m, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct WNDCLASSEX {
        public uint cbSize, style; public WndProcFn lpfnWndProc; public int cbClsExtra, cbWndExtra;
        public IntPtr hInstance, hIcon, hCursor, hbrBackground; public string lpszMenuName, lpszClassName; public IntPtr hIconSm; }
    [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public int x, y; }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int left, top, right, bottom; }
    [StructLayout(LayoutKind.Sequential)] struct PAINTSTRUCT { public IntPtr hdc; public int fErase; public RECT rc; public int fRestore, fIncUpdate; [MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)] public byte[] r; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern ushort RegisterClassExW(ref WNDCLASSEX c);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateWindowExW(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr p);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProcW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool PeekMessageW(out MSG m, IntPtr h, uint a, uint b, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessageW(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr BeginPaint(IntPtr h, out PAINTSTRUCT p);
    [DllImport("user32.dll")] static extern bool EndPaint(IntPtr h, ref PAINTSTRUCT p);
    [DllImport("user32.dll")] static extern int FillRect(IntPtr dc, ref RECT r, IntPtr brush);
    [DllImport("gdi32.dll")] static extern IntPtr CreateSolidBrush(int color);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int cy, uint f);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] static extern bool InvalidateRect(IntPtr h, IntPtr r, bool erase);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string n);
    static readonly WndProcFn proc = Proc;
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    static bool reversed;
    static string log, label;
    static readonly IntPtr[] brushes = new IntPtr[9];
    static int Cell(int x, int y) {
        for (int i = 0; i < 9; i++) {
            int l = 10 + (i % 3) * 105, t = 10 + (i / 3) * 65;
            if (x >= l && x < l + 100 && y >= t && y < t + 60) return i;
        }
        return -1;
    }
    static int Painted(int i) { return reversed ? 8 - i : i; }
    static IntPtr Proc(IntPtr h, uint m, IntPtr w, IntPtr l) {
        if (m == 0x000F) {
            PAINTSTRUCT ps; IntPtr dc = BeginPaint(h, out ps);
            for (int i = 0; i < 9; i++) { var r = new RECT { left = 10 + (i % 3) * 105, top = 10 + (i / 3) * 65 }; r.right = r.left + 100; r.bottom = r.top + 60; FillRect(dc, ref r, brushes[Painted(i)]); }
            EndPaint(h, ref ps); return IntPtr.Zero;
        }
        if (m == 0x0201 || m == 0x0204) {
            int x = (short)(l.ToInt64() & 0xFFFF), y = (short)((l.ToInt64() >> 16) & 0xFFFF);
            int c = Cell(x, y);
            File.AppendAllText(log, Json.Serialize(new { window = label, kind = "down", button = m == 0x0201 ? "Left" : "Right",
                cell = c < 0 ? "-" : ((char)('A' + Painted(c))).ToString(), where = "reuse", x, y, hwnd = h.ToInt64(), reversed,
                time = Stopwatch.GetTimestamp() }) + Environment.NewLine);
            return IntPtr.Zero;
        }
        return DefWindowProcW(h, m, w, l);
    }
    static void Pump() { MSG msg; while (PeekMessageW(out msg, IntPtr.Zero, 0, 0, 1)) { TranslateMessage(ref msg); DispatchMessageW(ref msg); } }
    public static int Run(int x, int y, string windowLabel, string mode, string work, string eventLog) {
        log = eventLog; label = windowLabel;
        for (int i = 0; i < 9; i++) { var c = PanelFixture.Colors[i]; brushes[i] = CreateSolidBrush(c.R | (c.G << 8) | (c.B << 16)); }
        var wc = new WNDCLASSEX { cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX)), lpfnWndProc = proc, hInstance = GetModuleHandleW(null),
            hbrBackground = CreateSolidBrush(0xFFFFFF), lpszClassName = "ICUReuse" };
        if (RegisterClassExW(ref wc) == 0) throw new Exception("RegisterClassEx failed");
        const uint style = 0x00CA0000; // caption | sysmenu | border
        IntPtr hwnd = CreateWindowExW(0, "ICUReuse", "ICU Panels " + label, style | 0x10000000, x, y, 346, 249, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
        File.WriteAllText(Path.Combine(work, "panel-ready-" + label + ".json"), Json.Serialize(new {
            pid = Process.GetCurrentProcess().Id, window = hwnd.ToInt64(), mode, shape = "reuse", dpi = GetDpiForWindow(hwnd) }));
        string trigger = Path.Combine(work, "panel-reuse-go-" + label);
        while (!File.Exists(trigger)) { Pump(); Thread.Sleep(20); }
        RECT rect; GetWindowRect(hwnd, out rect);
        long target = hwnd.ToInt64();
        DestroyWindow(hwnd);
        var watch = Stopwatch.StartNew(); long tries = 0; IntPtr found = IntPtr.Zero;
        while (watch.Elapsed.TotalSeconds < 120) {
            IntPtr h = CreateWindowExW(0, "ICUReuse", "ICU Panels " + label, style, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
            tries++;
            if (h.ToInt64() == target) { found = h; break; }
            DestroyWindow(h);
            if ((tries & 1023) == 0) Pump();
        }
        if (found != IntPtr.Zero) {
            reversed = true;
            SetWindowPos(found, IntPtr.Zero, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, 0x0014); // NOZORDER | NOACTIVATE
            ShowWindow(found, 4); // SW_SHOWNOACTIVATE
            InvalidateRect(found, IntPtr.Zero, true);
        }
        File.WriteAllText(Path.Combine(work, "panel-reuse-done-" + label + ".json"), Json.Serialize(new {
            target, found = found.ToInt64(), tries, seconds = watch.Elapsed.TotalSeconds }));
        if (found == IntPtr.Zero) return 2;
        while (true) { Pump(); Thread.Sleep(10); }
    }
}
