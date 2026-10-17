using System;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows.Forms;

// Independent receivers for native input probes. Every callback is appended
// to events.jsonl by the receiving control itself.
// Usage: ICUProbeFixture.exe --text <work> | --scroll <work> | --identity <work>
public static class ProbeFixture {
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    static string Work;

    static void Log(object value) {
        File.AppendAllText(Path.Combine(Work,"events.jsonl"),Json.Serialize(value)+Environment.NewLine);
    }

    // Top-level windows in one process and one UI thread, so all share one
    // Win32 focus state. Beta is shown last and keeps that focus. Locked's
    // field is read-only: UIA SetValue and WM_CHAR both leave it as it was.
    static Form Editor(string name,int x,int y,TextBox[] holder,bool readOnly=false) {
        var form=new Form { Text="ICU Text "+name, StartPosition=FormStartPosition.Manual,
            Location=new Point(x,y), ClientSize=new Size(300,120) };
        var box=new TextBox { Name="field"+name, Text=name.ToLowerInvariant(), AccessibleName="Field "+name,
            Location=new Point(15,15), Width=270, ReadOnly=readOnly };
        box.TextChanged+=(s,e)=>Log(new { kind="text", window=name, value=box.Text,
            foreground=GetForegroundWindow().ToInt64(), time=Stopwatch.GetTimestamp() });
        box.KeyDown+=(s,e)=>Log(new { kind="keydown", window=name, key=e.KeyCode.ToString(),
            modifiers=e.Modifiers.ToString(), time=Stopwatch.GetTimestamp() });
        box.GotFocus+=(s,e)=>Log(new { kind="focus", window=name, time=Stopwatch.GetTimestamp() });
        form.Controls.Add(box); holder[0]=box;
        return form;
    }

    [STAThread] public static void Main(string[] args) {
        Application.EnableVisualStyles();
        Work=args[1]; Directory.CreateDirectory(Work);
        if(args[0]=="--text") {
            var a=new TextBox[1]; var b=new TextBox[1]; var l=new TextBox[1];
            Form alpha=Editor("Alpha",40,80,a), beta=Editor("Beta",380,80,b), locked=Editor("Locked",40,260,l,true);
            beta.Shown+=(s,e)=> {
                b[0].Focus();
                File.WriteAllText(Path.Combine(Work,"text-ready.json"),Json.Serialize(new {
                    pid=Process.GetCurrentProcess().Id, alpha=alpha.Handle.ToInt64(), beta=beta.Handle.ToInt64(), locked=locked.Handle.ToInt64(),
                    alpha_field=a[0].Handle.ToInt64(), beta_field=b[0].Handle.ToInt64() }));
            };
            alpha.Show(); locked.Show();
            Application.Run(beta);
            return;
        }
        if(args[0]=="--identity") {
            // WinForms reports Control.Name as the UIA AutomationId. Keep has
            // one, Alpha has none; Still never changes. relabel.txt renames
            // the labels, and "rename-id" also replaces Keep's AutomationId.
            var identity=new Form { Text="ICU Identity", StartPosition=FormStartPosition.Manual,
                Location=new Point(40,300), ClientSize=new Size(300,140) };
            var keep=new Button { Name="btnKeep", Text="Keep", Location=new Point(15,15), Size=new Size(120,40) };
            var plain=new Button { Name="", Text="Alpha", Location=new Point(150,15), Size=new Size(120,40) };
            var still=new Button { Name="btnStill", Text="Still", Location=new Point(15,75), Size=new Size(120,40) };
            identity.Controls.AddRange(new Control[] { keep, plain, still });
            var watch=new Timer { Interval=50 };
            watch.Tick+=(s,e)=> {
                string path=Path.Combine(Work,"relabel.txt");
                if(!File.Exists(path)) return;
                string how=File.ReadAllText(path).Trim(); File.Delete(path);
                keep.Text="Kept"; plain.Text="Play";
                if(how=="rename-id") keep.Name="btnRenamed";
                Log(new { kind="relabel", how=how, keep_name=keep.Name, time=Stopwatch.GetTimestamp() });
            };
            identity.Shown+=(s,e)=> {
                watch.Start();
                File.WriteAllText(Path.Combine(Work,"identity-ready.json"),Json.Serialize(new {
                    pid=Process.GetCurrentProcess().Id, window=identity.Handle.ToInt64() }));
            };
            Application.Run(identity);
            return;
        }
        var list=new ListBox { Dock=DockStyle.Fill, IntegralHeight=false, AccessibleName="Rows" };
        for(int i=0;i<200;i++) list.Items.Add("Row "+i.ToString("000"));
        var form=new Form { Text="ICU Scroll", StartPosition=FormStartPosition.Manual,
            Location=new Point(40,300), ClientSize=new Size(260,220) };
        form.Controls.Add(list);
        list.MouseWheel+=(s,e)=>Log(new { kind="wheel", delta=e.Delta, time=Stopwatch.GetTimestamp() });
        int top=-1;
        var poll=new Timer { Interval=25 };
        poll.Tick+=(s,e)=> { if(list.TopIndex!=top) { top=list.TopIndex;
            Log(new { kind="top", top=top, time=Stopwatch.GetTimestamp() }); } };
        form.Shown+=(s,e)=> {
            list.TopIndex=100; poll.Start();
            File.WriteAllText(Path.Combine(Work,"scroll-ready.json"),Json.Serialize(new {
                pid=Process.GetCurrentProcess().Id, window=form.Handle.ToInt64(), list=list.Handle.ToInt64() }));
        };
        Application.Run(form);
    }
}
