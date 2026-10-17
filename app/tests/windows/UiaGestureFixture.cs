// A window for gaps3-forward.ps1: UiaGestureFixture.exe <dir> [title] [allow-messages]
// It has a button, a single-line text box and a list with 400 rows. What reached it is written
// under <dir>: events.txt (one line per mouse press, activation and wheel notch), text.txt (the
// text box content), scroll.txt (the list's top index) and target.ready ("<hwnd> <pid>") once the
// window is up. The mouse-press lines come from the window's own message pump, so a UIA Invoke
// (which raises Click without a press) is told apart from a real pointer click.
using System;
using System.Diagnostics;
using System.IO;
using System.Windows.Forms;

class GestureButton : Button
{
    public string Log;
    protected override void OnMouseDown(MouseEventArgs e)
    {
        File.AppendAllText(Log, "mousedown " + e.Button + "\n");
        base.OnMouseDown(e);
    }
}

class GestureList : ListBox
{
    public string Log;
    protected override void OnMouseWheel(MouseEventArgs e)
    {
        File.AppendAllText(Log, "wheel " + e.Delta + "\n");
        base.OnMouseWheel(e);
    }
}

// A pad that logs a drag: the press point, the release point and how many moves it saw with the
// button held ("drag <downX> <downY> <upX> <upY> <moves>"), in the pad's own client pixels.
class DragPad : Panel
{
    public string Log;
    int downX, downY, moves; bool down;
    protected override void OnMouseDown(MouseEventArgs e) { down = true; moves = 0; downX = e.X; downY = e.Y; base.OnMouseDown(e); }
    protected override void OnMouseMove(MouseEventArgs e) { if (down) moves++; base.OnMouseMove(e); }
    protected override void OnMouseUp(MouseEventArgs e)
    {
        if (down) File.AppendAllText(Log, "drag " + downX + " " + downY + " " + e.X + " " + e.Y + " " + moves + "\n");
        down = false;
        base.OnMouseUp(e);
    }
}

static class UiaGestureFixture
{
    [STAThread]
    static int Main(string[] args)
    {
        string dir = args[0];
        string title = args.Length > 1 ? args[1] : "ICU-GESTURE-TARGET";
        try { return Run(dir, title, args); }
        catch (Exception e) { File.WriteAllText(Path.Combine(dir, "error.txt"), e.ToString()); return 1; }
    }

    // A reader may hold the file for a moment: retry rather than let an exception reach the UI thread.
    static void Write(string path, string text)
    {
        for (int i = 0; i < 20; i++)
        {
            try
            {
                using (var f = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.ReadWrite))
                using (var w = new StreamWriter(f)) { w.Write(text); }
                return;
            }
            catch (IOException) { System.Threading.Thread.Sleep(10); }
        }
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    static extern bool ChangeWindowMessageFilterEx(IntPtr hwnd, uint message, uint action, IntPtr change);

    static int Run(string dir, string title, string[] args2)
    {
        string events = Path.Combine(dir, "events.txt");
        var form = new Form
        {
            Text = title,
            Width = 520, Height = 520,
            StartPosition = FormStartPosition.Manual,
            Left = 40, Top = 60,
        };
        var button = new GestureButton { Text = "ICU-BTN", AccessibleName = "ICU-BTN", Left = 10, Top = 10, Width = 200, Height = 40, Log = events };
        button.Click += (s, e) => File.AppendAllText(events, "click\n");
        var box = new TextBox { AccessibleName = "ICU-TEXT", Left = 10, Top = 60, Width = 480, Height = 24 };
        box.TextChanged += (s, e) => Write(Path.Combine(dir, "text.txt"), box.Text);
        var list = new GestureList { AccessibleName = "ICU-LIST", Left = 10, Top = 150, Width = 480, Height = 320, IntegralHeight = false, Log = events };
        for (int i = 0; i < 400; i++) list.Items.Add("row " + i);
        var pad = new DragPad { AccessibleName = "ICU-PAD", BackColor = System.Drawing.Color.LightSteelBlue, Left = 230, Top = 10, Width = 260, Height = 40, Log = events };
        form.Controls.Add(pad);
        form.Controls.Add(button);
        form.Controls.Add(box);
        form.Controls.Add(list);
        int last = -1;
        var timer = new Timer { Interval = 50 };
        timer.Tick += (s, e) =>
        {
            if (list.TopIndex != last)
            {
                last = list.TopIndex;
                Write(Path.Combine(dir, "scroll.txt"), last.ToString());
            }
        };
        timer.Start();
        form.Shown += (s, e) =>
        {
            // "allow-messages" (third argument): the window opts in to the mouse messages a lower-integrity
            // process posts to it (what a UIPI-aware application can do for itself). The same post that is
            // refused with ERROR_ACCESS_DENIED without this lands with it, which shows UIPI is the reason.
            if (args2.Length > 2 && args2[2] == "allow-messages")
            {
                foreach (Control c in new Control[] { form, button, box, list, pad })
                    foreach (uint m in new uint[] { 0x200, 0x201, 0x202, 0x203, 0x204, 0x205, 0x20A })
                        ChangeWindowMessageFilterEx(c.Handle, m, 1, IntPtr.Zero);
            }
            // Screen centres of the controls, so a harness can aim pixel gestures without the UIA tree.
            Func<Control, string> centre = c =>
            {
                var r = c.RectangleToScreen(c.ClientRectangle);
                return "[" + (r.Left + r.Width / 2) + "," + (r.Top + r.Height / 2) + "]";
            };
            File.WriteAllText(Path.Combine(dir, "layout.json"),
                "{\"button\":" + centre(button) + ",\"text\":" + centre(box) + ",\"list\":" + centre(list) + ",\"pad\":" + centre(pad) + "}");
            File.WriteAllText(Path.Combine(dir, "scroll.txt"), list.TopIndex.ToString());
            File.WriteAllText(Path.Combine(dir, "text.txt"), "");
            File.WriteAllText(Path.Combine(dir, "target.ready"), form.Handle.ToInt64() + " " + Process.GetCurrentProcess().Id);
        };
        Application.Run(form);
        return 0;
    }
}
