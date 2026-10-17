// A window with a text box for uia-forward.ps1: UiaForwardFixture.exe target <dir>
// Every change to the text is written to <dir>\text.txt, and <dir>\target.ready holds
// "<hwnd> <pid>" once the window is up.
using System;
using System.Diagnostics;
using System.IO;
using System.Windows.Forms;

static class UiaForwardFixture
{
    [STAThread]
    static int Main(string[] args)
    {
        string dir = args[1];
        try { return Run(dir); }
        catch (Exception e) { File.WriteAllText(Path.Combine(dir, "error.txt"), e.ToString()); return 1; }
    }

    static int Run(string dir)
    {
        var form = new Form
        {
            Text = "ICU-UIA-TARGET",
            Width = 420, Height = 220,
            StartPosition = FormStartPosition.Manual,
            Left = 40, Top = 60,
        };
        var box = new TextBox { Dock = DockStyle.Fill, Multiline = true };
        form.Controls.Add(box);
        box.TextChanged += (s, e) => File.WriteAllText(Path.Combine(dir, "text.txt"), box.Text);
        form.Shown += (s, e) =>
            File.WriteAllText(Path.Combine(dir, "target.ready"),
                form.Handle.ToInt64() + " " + Process.GetCurrentProcess().Id);
        Application.Run(form);
        return 0;
    }
}
