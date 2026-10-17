using System;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows.Forms;

// A color-coded 3x3 button grid. The driver's screenshot locates each color;
// the button's own Click callback, not the driver's reply, proves the target.
// Usage: ICUDpiFixture.exe <pmv2|unaware> <label> <x> <y> <work>
public static class DpiFixture {
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    public static readonly Color[] Colors = {
        Color.FromArgb(230,25,75), Color.FromArgb(60,180,75), Color.FromArgb(0,130,200),
        Color.FromArgb(245,130,48), Color.FromArgb(145,30,180), Color.FromArgb(70,240,240),
        Color.FromArgb(240,50,230), Color.FromArgb(210,245,60), Color.FromArgb(128,0,0) };

    [STAThread] public static void Main(string[] args) {
        string mode=args[0], label=args[1], work=args[4];
        // PMv2 must be chosen before the first window exists; "unaware" keeps
        // the process default so Windows bitmap-scales it on a high-DPI monitor.
        if(mode=="pmv2" && !SetProcessDpiAwarenessContext(new IntPtr(-4))) throw new Exception("PMv2 unavailable");
        Directory.CreateDirectory(work);
        string log=Path.Combine(work,"dpi-events-"+label+".jsonl");
        var form=new Form { Text="ICU DPI "+label, StartPosition=FormStartPosition.Manual,
            Location=new Point(int.Parse(args[2]),int.Parse(args[3])), ClientSize=new Size(330,210),
            FormBorderStyle=FormBorderStyle.FixedSingle, MaximizeBox=false, AutoScaleMode=AutoScaleMode.None };
        for(int i=0;i<9;i++) {
            string name=((char)('A'+i)).ToString();
            var button=new Button { Text=name, AccessibleName=label+" "+name, FlatStyle=FlatStyle.Flat,
                BackColor=Colors[i], ForeColor=Colors[i], Location=new Point(10+(i%3)*105,10+(i/3)*65), Size=new Size(100,60) };
            button.FlatAppearance.BorderSize=0;
            button.FlatAppearance.MouseOverBackColor=Colors[i];
            button.FlatAppearance.MouseDownBackColor=Colors[i];
            Point down=Point.Empty;
            button.MouseDown+=(s,e)=>down=e.Location;
            button.Click+=(s,e)=>File.AppendAllText(log,Json.Serialize(new {
                window=label, button=name, client_x=down.X, client_y=down.Y,
                dpi=GetDpiForWindow(form.Handle), foreground=GetForegroundWindow().ToInt64(),
                time=Stopwatch.GetTimestamp() })+Environment.NewLine);
            form.Controls.Add(button);
        }
        form.Shown+=(s,e)=>File.WriteAllText(Path.Combine(work,"dpi-ready-"+label+".json"),Json.Serialize(new {
            pid=Process.GetCurrentProcess().Id, window=form.Handle.ToInt64(), mode=mode, dpi=GetDpiForWindow(form.Handle) }));
        Application.Run(form);
    }
}
