using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Forms.Integration;
using System.Windows.Interop;
using System.Windows.Media;

// A WPF window with the same 3x3 colour grid as PanelFixture, drawn as
// Borders in one HWND: no child windows and no UIA Invoke, so a pixel click
// has neither a child window to post to nor an accessible control to invoke.
// Every MouseDown a Border receives is logged, in PanelFixture's event format.
// Usage: ICUWpfFixture.exe <default|pmv2|elementhost|wfhost> <label> <x> <y> <work>
//   default: WPF's own DPI awareness; pmv2: Per-Monitor-v2 set before WPF starts;
//   elementhost: the same grid in a WinForms form, through an ElementHost, so
//   the WPF HwndWrapper is a child of a non-WPF root (review R3-4).
//   wfhost: the other way round, a WPF window whose content is a WindowsFormsHost
//   holding 9 WinForms Panels in the grid's colours and places, each logging its
//   MouseDown (final review D7). The panels are real Win32 child windows.
public static class WpfFixture {
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr window);
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    static readonly byte[][] Colors = {
        new byte[]{230,25,75}, new byte[]{60,180,75}, new byte[]{0,130,200},
        new byte[]{245,130,48}, new byte[]{145,30,180}, new byte[]{70,240,240},
        new byte[]{240,50,230}, new byte[]{210,245,60}, new byte[]{128,0,0} };

    [STAThread] public static void Main(string[] args) {
        // Before any WPF type is touched: compiling a method that uses WPF
        // loads PresentationCore, which sets the process's DPI awareness.
        if (args[0] == "pmv2" && !SetProcessDpiAwarenessContext(new IntPtr(-4))) throw new Exception("PMv2 unavailable");
        Run(args);
    }

    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    static void Run(string[] args) {
        string mode = args[0], label = args[1], work = args[4];
        Directory.CreateDirectory(work);
        string log = Path.Combine(work, "panel-events-" + label + ".jsonl");
        var window = new Window { Title = "ICU WPF " + label, Width = 346, Height = 249, Left = double.Parse(args[2]), Top = double.Parse(args[3]),
            ResizeMode = ResizeMode.NoResize, ShowActivated = false, WindowStartupLocation = WindowStartupLocation.Manual, Background = Brushes.White };
        var grid = new UniformGrid { Rows = 3, Columns = 3, Background = Brushes.White };
        IntPtr handle = IntPtr.Zero;
        for (int i = 0; i < 9; i++) {
            string cell = ((char)('A' + i)).ToString();
            var border = new Border { Margin = new Thickness(4), Background = new SolidColorBrush(Color.FromRgb(Colors[i][0], Colors[i][1], Colors[i][2])) };
            border.MouseDown += (s, e) => {
                var p = e.GetPosition(border);
                File.AppendAllText(log, Json.Serialize(new { window = label, kind = e.ClickCount > 1 ? "double" : "down",
                    button = e.ChangedButton.ToString(), cell, where = "wpf", x = (int)p.X, y = (int)p.Y,
                    device = e.StylusDevice != null ? "stylus" : "mouse", dpi = GetDpiForWindow(handle),
                    time = Stopwatch.GetTimestamp() }) + Environment.NewLine);
            };
            grid.Children.Add(border);
        }
        if (mode == "elementhost") {
            var form = new System.Windows.Forms.Form { Text = "ICU WPF " + label, StartPosition = System.Windows.Forms.FormStartPosition.Manual,
                Location = new System.Drawing.Point(int.Parse(args[2]), int.Parse(args[3])), ClientSize = new System.Drawing.Size(330, 210),
                FormBorderStyle = System.Windows.Forms.FormBorderStyle.FixedSingle, MaximizeBox = false, BackColor = System.Drawing.Color.White };
            form.Controls.Add(new System.Windows.Forms.Integration.ElementHost { Dock = System.Windows.Forms.DockStyle.Fill, Child = grid });
            form.Shown += (s, e) => {
                handle = form.Handle;
                File.WriteAllText(Path.Combine(work, "panel-ready-" + label + ".json"), Json.Serialize(new {
                    pid = Process.GetCurrentProcess().Id, window = handle.ToInt64(), mode, shape = "wpf", dpi = GetDpiForWindow(handle),
                    tablet_devices = System.Windows.Input.Tablet.TabletDevices.Count }));
            };
            System.Windows.Forms.Application.Run(form);
            return;
        }
        if (mode == "wfhost") {
            var host = new System.Windows.Forms.Panel { BackColor = System.Drawing.Color.White };
            for (int i = 0; i < 9; i++) {
                string cell = ((char)('A' + i)).ToString();
                var panel = new System.Windows.Forms.Panel { BackColor = System.Drawing.Color.FromArgb(Colors[i][0], Colors[i][1], Colors[i][2]),
                    Location = new System.Drawing.Point(10 + (i % 3) * 105, 10 + (i / 3) * 65), Size = new System.Drawing.Size(100, 60) };
                panel.MouseDown += (s, e) => File.AppendAllText(log, Json.Serialize(new { window = label, kind = e.Clicks > 1 ? "double" : "down",
                    button = e.Button.ToString(), cell, where = "winforms", x = e.X, y = e.Y, dpi = GetDpiForWindow(handle),
                    time = Stopwatch.GetTimestamp() }) + Environment.NewLine);
                host.Controls.Add(panel);
            }
            window.Content = new WindowsFormsHost { Child = host };
        } else {
            window.Content = grid;
        }
        window.ContentRendered += (s, e) => {
            handle = new WindowInteropHelper(window).Handle;
            File.WriteAllText(Path.Combine(work, "panel-ready-" + label + ".json"), Json.Serialize(new {
                pid = Process.GetCurrentProcess().Id, window = handle.ToInt64(), mode, shape = "wpf", dpi = GetDpiForWindow(handle),
                tablet_devices = System.Windows.Input.Tablet.TabletDevices.Count }));
        };
        new Application().Run(window);
    }
}
