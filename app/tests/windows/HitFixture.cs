using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;

// Raw Win32 fixtures for which window real input reaches at a pixel (adapted
// from the 2026-09-26 round-3 review's R3Fixture). Every child's style and
// z-order is exactly what the shape names. Cells A..I use PanelFixture's
// colours; J is navy. Every window logs the mouse messages it receives in
// PanelFixture's event format, with the window's name as the cell: A..I, J,
// "group" or "form".
// Usage: ICUHitFixture.exe <pmv2|unaware> <shape> <label> <x> <y> <work>
//   groupbox     9 cell windows, then a BUTTON/BS_GROUPBOX sibling enclosing them,
//                raised above them in z-order (a dialog template listing the group
//                box first gives this). It answers WM_NCHITTEST with HTTRANSPARENT.
//   transparent  9 cells, plus J, an opaque WS_EX_TRANSPARENT child over the
//                middle of E (40x30 in E's 100x60: J's centre is E's centre)
//   halfhit      9 cells, plus J over the middle of E, answering WM_NCHITTEST with
//                HTTRANSPARENT on its left half and HTCLIENT on its right half,
//                from the point it reads in its own coordinates
//   region       9 cells, E with an elliptic window region: the corners of E's rect
//                show the form
//   big          9 cells scaled to a 3000x1800 client, so its PNG is slow to downscale
//   nested       9 cells, plus S, a plain STATIC (answers HTTRANSPARENT) raised over the
//                middle of E, holding J, an opaque child whose centre is E's centre
//                (final review D1: real input reaches J; beside J, inside S, it reaches E)
//   gbchild      the same with a BUTTON/BS_GROUPBOX ("group") in place of S
//   wfgroup      the same in WinForms: 9 Panels, and a GroupBox with FlatStyle.System
//                (a BS_GROUPBOX window, "group") brought to the front over E, holding
//                a Panel J. Mouse downs are logged from WinForms' MouseDown.
public static class HitFixture {
    delegate IntPtr WndProcFn(IntPtr h, uint m, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct WNDCLASSEX {
        public uint cbSize, style; public WndProcFn lpfnWndProc; public int cbClsExtra, cbWndExtra;
        public IntPtr hInstance, hIcon, hCursor, hbrBackground; public string lpszMenuName, lpszClassName; public IntPtr hIconSm; }
    [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public int x, y; }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int left, top, right, bottom; }
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int x, y; }
    [StructLayout(LayoutKind.Sequential)] struct PAINTSTRUCT { public IntPtr hdc; public int fErase; public RECT rc; public int fRestore, fIncUpdate; [MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)] public byte[] r; }
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr c);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern ushort RegisterClassExW(ref WNDCLASSEX c);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateWindowExW(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr p);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProcW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr CallWindowProcW(IntPtr prev, IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool GetMessageW(out MSG m, IntPtr h, uint a, uint b);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessageW(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr BeginPaint(IntPtr h, out PAINTSTRUCT p);
    [DllImport("user32.dll")] static extern bool EndPaint(IntPtr h, ref PAINTSTRUCT p);
    [DllImport("user32.dll")] static extern int FillRect(IntPtr dc, ref RECT r, IntPtr brush);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool ScreenToClient(IntPtr h, ref POINT p);
    [DllImport("gdi32.dll")] static extern IntPtr CreateSolidBrush(int color);
    [DllImport("gdi32.dll")] static extern IntPtr CreateEllipticRgn(int l, int t, int r, int b);
    [DllImport("user32.dll")] static extern int SetWindowRgn(IntPtr h, IntPtr rgn, bool redraw);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int cy, uint f);
    [DllImport("user32.dll")] static extern bool AdjustWindowRectEx(ref RECT r, uint style, bool menu, uint ex);
    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")] static extern IntPtr SetWindowLongPtr(IntPtr h, int i, IntPtr v);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string n);

    const uint WS_CHILD = 0x40000000, WS_VISIBLE = 0x10000000, WS_CLIPSIBLINGS = 0x04000000, WS_CLIPCHILDREN = 0x02000000,
        WS_CAPTION = 0x00C00000, WS_SYSMENU = 0x00080000, WS_BORDER = 0x00800000;
    const uint WS_EX_TRANSPARENT = 0x20, BS_GROUPBOX = 0x7;
    static readonly IntPtr HWND_TOP = IntPtr.Zero;
    const uint SWP_NOSIZE = 0x1, SWP_NOMOVE = 0x2, SWP_NOZORDER = 0x4, SWP_NOACTIVATE = 0x10, SWP_SHOWWINDOW = 0x40;

    static readonly int[][] C = { new[]{230,25,75}, new[]{60,180,75}, new[]{0,130,200}, new[]{245,130,48}, new[]{145,30,180},
        new[]{70,240,240}, new[]{240,50,230}, new[]{210,245,60}, new[]{128,0,0}, new[]{0,0,128} };
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    static string log, label, mode, shape;
    static readonly Dictionary<IntPtr, string> Names = new Dictionary<IntPtr, string>();
    static readonly Dictionary<IntPtr, int> Colour = new Dictionary<IntPtr, int>();
    static IntPtr groupProc;
    static readonly IntPtr[] Brushes = new IntPtr[10];
    static readonly WndProcFn rootProc = RootProc, cellProc = CellProc, hookProc = HookProc;
    static IntPtr hInst, root, halfJ;
    static readonly IntPtr[] Cells = new IntPtr[9];

    static void LogMouse(IntPtr h, uint m, IntPtr l) {
        string kind = null, button = null;
        switch (m) {
            case 0x0201: kind = "down"; button = "Left"; break;
            case 0x0203: kind = "double"; button = "Left"; break;
            case 0x0204: kind = "down"; button = "Right"; break;
            case 0x0206: kind = "double"; button = "Right"; break;
        }
        if (kind == null) return;
        int x = (short)(l.ToInt64() & 0xFFFF), y = (short)((l.ToInt64() >> 16) & 0xFFFF);
        string name; if (!Names.TryGetValue(h, out name)) name = "?";
        File.AppendAllText(log, Json.Serialize(new { window = label, kind, button, cell = name, where = "hit", x, y,
            dpi = GetDpiForWindow(root), time = Stopwatch.GetTimestamp() }) + Environment.NewLine);
    }

    static IntPtr RootProc(IntPtr h, uint m, IntPtr w, IntPtr l) {
        LogMouse(h, m, l);
        if (m == 0x0002 && h == root) Environment.Exit(0);  // WM_DESTROY
        return DefWindowProcW(h, m, w, l);
    }

    static IntPtr CellProc(IntPtr h, uint m, IntPtr w, IntPtr l) {
        LogMouse(h, m, l);
        if (m == 0x000F) {  // WM_PAINT
            PAINTSTRUCT ps; IntPtr dc = BeginPaint(h, out ps);
            RECT r; GetClientRect(h, out r);
            int c; if (Colour.TryGetValue(h, out c)) FillRect(dc, ref r, Brushes[c]);
            EndPaint(h, ref ps); return IntPtr.Zero;
        }
        if (m == 0x0084 && h == halfJ) {  // WM_NCHITTEST: screen point, in this window's coordinates
            var p = new POINT { x = (short)(l.ToInt64() & 0xFFFF), y = (short)((l.ToInt64() >> 16) & 0xFFFF) };
            ScreenToClient(h, ref p);
            RECT r; GetClientRect(h, out r);
            return new IntPtr(p.x < r.right / 2 ? -1 : 1);  // HTTRANSPARENT : HTCLIENT
        }
        if (m == 0x0021) return new IntPtr(3);  // WM_MOUSEACTIVATE: MA_NOACTIVATE
        return DefWindowProcW(h, m, w, l);
    }

    // The group box is a system BUTTON (a static in `nested`); subclass it so
    // its messages are logged. One per fixture.
    static IntPtr HookProc(IntPtr h, uint m, IntPtr w, IntPtr l) {
        LogMouse(h, m, l);
        return CallWindowProcW(groupProc, h, m, w, l);
    }

    static IntPtr Hooked(string cls, uint style, string name, int x, int y, int w, int h) {
        IntPtr c = CreateWindowExW(0, cls, "", WS_CHILD | WS_VISIBLE | style, x, y, w, h, root, IntPtr.Zero, hInst, IntPtr.Zero);
        if (c == IntPtr.Zero) throw new Exception(cls + " failed " + Marshal.GetLastWin32Error());
        Names[c] = name;
        groupProc = GetWindowLongPtr(c, -4);
        SetWindowLongPtr(c, -4, Marshal.GetFunctionPointerForDelegate(hookProc));
        return c;
    }

    static IntPtr Cell(string name, int colour, int x, int y, int w, int h, uint style, uint ex) {
        return Cell(root, name, colour, x, y, w, h, style, ex);
    }

    static IntPtr Cell(IntPtr parent, string name, int colour, int x, int y, int w, int h, uint style, uint ex) {
        IntPtr c = CreateWindowExW(ex, "ICUHitCell", name, WS_CHILD | WS_VISIBLE | style, x, y, w, h, parent, IntPtr.Zero, hInst, IntPtr.Zero);
        if (c == IntPtr.Zero) throw new Exception("cell " + name + " failed " + Marshal.GetLastWin32Error());
        Names[c] = name; Colour[c] = colour; return c;
    }

    // Cells clip against siblings above them, except under the group box,
    // which encloses them all and would hide them.
    static void Grid(double kx, double ky, uint style) {
        for (int i = 0; i < 9; i++) {
            int l = (int)Math.Round((10 + (i % 3) * 105) * kx), t = (int)Math.Round((10 + (i / 3) * 65) * ky);
            Cells[i] = Cell(((char)('A' + i)).ToString(), i, l, t, (int)Math.Round(100 * kx), (int)Math.Round(60 * ky), style, 0);
        }
    }

    static void Raise(IntPtr h) { SetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE); }

    static int WinFormsGroup(int x, int y, string work) {
        var form = new System.Windows.Forms.Form { Text = "ICU Hit " + label, StartPosition = System.Windows.Forms.FormStartPosition.Manual,
            Location = new System.Drawing.Point(x, y), ClientSize = new System.Drawing.Size(330, 210),
            FormBorderStyle = System.Windows.Forms.FormBorderStyle.FixedSingle, MaximizeBox = false,
            BackColor = System.Drawing.Color.White, AutoScaleMode = System.Windows.Forms.AutoScaleMode.None };
        Action<System.Windows.Forms.Control, string> watch = (c, name) => c.MouseDown += (s, e) =>
            File.AppendAllText(log, Json.Serialize(new { window = label, kind = e.Clicks > 1 ? "double" : "down", button = e.Button.ToString(),
                cell = name, where = "winforms", x = e.X, y = e.Y, dpi = GetDpiForWindow(form.Handle), time = Stopwatch.GetTimestamp() }) + Environment.NewLine);
        watch(form, "form");
        for (int i = 0; i < 9; i++) {
            var cell = new System.Windows.Forms.Panel { BackColor = System.Drawing.Color.FromArgb(C[i][0], C[i][1], C[i][2]),
                Location = new System.Drawing.Point(10 + (i % 3) * 105, 10 + (i / 3) * 65), Size = new System.Drawing.Size(100, 60) };
            watch(cell, ((char)('A' + i)).ToString());
            form.Controls.Add(cell);
        }
        var group = new System.Windows.Forms.GroupBox { FlatStyle = System.Windows.Forms.FlatStyle.System, Text = "",
            Location = new System.Drawing.Point(135, 83), Size = new System.Drawing.Size(60, 44) };
        watch(group, "group");
        var j = new System.Windows.Forms.Panel { BackColor = System.Drawing.Color.FromArgb(C[9][0], C[9][1], C[9][2]),
            Location = new System.Drawing.Point(15, 12), Size = new System.Drawing.Size(30, 20) };
        watch(j, "J");
        group.Controls.Add(j);
        form.Controls.Add(group);
        group.BringToFront();
        form.Shown += (s, e) => File.WriteAllText(Path.Combine(work, "panel-ready-" + label + ".json"), Json.Serialize(new {
            pid = Process.GetCurrentProcess().Id, window = form.Handle.ToInt64(), mode, shape, dpi = GetDpiForWindow(form.Handle) }));
        System.Windows.Forms.Application.Run(form);
        return 0;
    }

    [STAThread] public static int Main(string[] args) {
        mode = args[0]; shape = args[1]; label = args[2];
        int x = int.Parse(args[3]), y = int.Parse(args[4]); string work = args[5];
        if (mode == "pmv2" && !SetProcessDpiAwarenessContext(new IntPtr(-4))) throw new Exception("PMv2 unavailable");
        Directory.CreateDirectory(work);
        log = Path.Combine(work, "panel-events-" + label + ".jsonl");
        if (shape == "wfgroup") return WinFormsGroup(x, y, work);
        hInst = GetModuleHandleW(null);
        for (int i = 0; i < Brushes.Length; i++) Brushes[i] = CreateSolidBrush(C[i][0] | (C[i][1] << 8) | (C[i][2] << 16));
        var rc = new WNDCLASSEX { cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX)), style = 0x8, lpfnWndProc = rootProc, hInstance = hInst,
            hbrBackground = CreateSolidBrush(0xFFFFFF), lpszClassName = "ICUHitRoot" };
        var cc = new WNDCLASSEX { cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX)), style = 0x8, lpfnWndProc = cellProc, hInstance = hInst,
            lpszClassName = "ICUHitCell" };
        if (RegisterClassExW(ref rc) == 0 || RegisterClassExW(ref cc) == 0) throw new Exception("RegisterClassEx failed");
        int cw = shape == "big" ? 3000 : 330, ch = shape == "big" ? 1800 : 210;
        uint style = WS_CAPTION | WS_SYSMENU | WS_BORDER | WS_CLIPCHILDREN;
        var wr = new RECT { right = cw, bottom = ch };
        AdjustWindowRectEx(ref wr, style, false, 0);
        root = CreateWindowExW(0, "ICUHitRoot", "ICU Hit " + label, style, x, y, wr.right - wr.left, wr.bottom - wr.top, IntPtr.Zero, IntPtr.Zero, hInst, IntPtr.Zero);
        if (root == IntPtr.Zero) throw new Exception("root failed " + Marshal.GetLastWin32Error());
        Names[root] = "form";
        if (shape == "big") {
            Grid(9.33, 8.48, WS_CLIPSIBLINGS);
        } else {
            Grid(1.0, 1.0, shape == "groupbox" ? 0 : WS_CLIPSIBLINGS);
            if (shape == "groupbox") {
                IntPtr g = CreateWindowExW(0, "BUTTON", "", WS_CHILD | WS_VISIBLE | BS_GROUPBOX, 3, 3, 324, 204, root, IntPtr.Zero, hInst, IntPtr.Zero);
                Names[g] = "group";
                groupProc = GetWindowLongPtr(g, -4);
                SetWindowLongPtr(g, -4, Marshal.GetFunctionPointerForDelegate(hookProc));
                Raise(g);
            } else if (shape == "transparent") {
                Raise(Cell("J", 9, 145, 90, 40, 30, WS_CLIPSIBLINGS, WS_EX_TRANSPARENT));
            } else if (shape == "halfhit") {
                halfJ = Cell("J", 9, 145, 90, 40, 30, WS_CLIPSIBLINGS, 0);
                Raise(halfJ);
            } else if (shape == "nested" || shape == "gbchild") {
                // S (or the group box) covers E's middle; J inside it is centred on E.
                IntPtr s = shape == "nested"
                    ? Hooked("STATIC", WS_CLIPSIBLINGS | WS_CLIPCHILDREN, "S", 135, 83, 60, 44)
                    : Hooked("BUTTON", WS_CLIPSIBLINGS | WS_CLIPCHILDREN | BS_GROUPBOX, "group", 135, 83, 60, 44);
                Raise(s);
                Cell(s, "J", 9, 15, 12, 30, 20, 0, 0);
            } else if (shape == "region") {
                SetWindowRgn(Cells[4], CreateEllipticRgn(0, 0, 100, 60), true);
            } else throw new Exception("unknown shape " + shape);
        }
        SetWindowPos(root, IntPtr.Zero, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        File.WriteAllText(Path.Combine(work, "panel-ready-" + label + ".json"), Json.Serialize(new {
            pid = Process.GetCurrentProcess().Id, window = root.ToInt64(), mode, shape, dpi = GetDpiForWindow(root) }));
        MSG msg;
        while (GetMessageW(out msg, IntPtr.Zero, 0, 0)) { TranslateMessage(ref msg); DispatchMessageW(ref msg); }
        return 0;
    }
}
