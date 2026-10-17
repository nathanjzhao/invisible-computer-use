using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows.Forms;

public static class CalculatorFixture {
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    static string Work;
    static Form Calculator;
    static Label Display;
    static string Current = "0";
    static int? Left;
    static bool Fresh = true;
    static readonly List<string> Buttons = new List<string>();

    static void Save() {
        string path = Path.Combine(Work,"state.json");
        string temp = path + ".tmp";
        File.WriteAllText(temp,Json.Serialize(new { display=Display.Text, buttons=Buttons }));
        if (File.Exists(path)) File.Replace(temp,path,null); else File.Move(temp,path);
    }
    static void Press(string key) {
        Buttons.Add(key);
        string value;
        int digit;
        if (Int32.TryParse(key,out digit)) {
            Current = Fresh || Current=="0" ? key : Current+key;
            Fresh=false; value=Current;
        } else if (key=="×") {
            Left=Int32.Parse(Current); Fresh=true; value=Current+" ×";
        } else if (key=="=" && Left.HasValue) {
            Current=(Left.Value*Int32.Parse(Current)).ToString(); Left=null;
            Fresh=true; value=Current;
        } else {
            Current="0"; Left=null; Fresh=true; value=Current;
        }
        Display.Text=value; Display.AccessibleName=value;
        File.AppendAllText(Path.Combine(Work,"events.jsonl"),Json.Serialize(new {
            button=key, display=value, foreground=GetForegroundWindow().ToInt64(),
            target=Calculator.Handle.ToInt64(), time=Stopwatch.GetTimestamp()
        })+Environment.NewLine);
        Save();
    }
    [STAThread] public static void Main(string[] args) {
        Application.EnableVisualStyles();
        if(args[0]=="--guard") {
            Work=args[1];
            Form guard=new Form { Text="ICU Focus Guard", TopMost=true,
                StartPosition=FormStartPosition.Manual, Location=new Point(720,520),
                ClientSize=new Size(270,150) };
            guard.Controls.Add(new TextBox { Text="Keep keyboard focus here during the task.",
                Multiline=true, Dock=DockStyle.Fill, AccessibleName="Focus ownership sentinel" });
            guard.Shown+=(sender,e)=>File.WriteAllText(Path.Combine(Work,"guard-ready.json"),Json.Serialize(new {
                pid=Process.GetCurrentProcess().Id, window=guard.Handle.ToInt64()
            }));
            Application.Run(guard); return;
        }
        Work=args[0]; Directory.CreateDirectory(Work);
        Calculator=new Form { Text="ICU Calculator", StartPosition=FormStartPosition.Manual,
            Location=new Point(620,70), ClientSize=new Size(350,270) };
        Display=new Label { Name="readout", Text="0", AccessibleName="0",
            Location=new Point(20,20), Size=new Size(300,35), TextAlign=ContentAlignment.MiddleCenter };
        Calculator.Controls.Add(Display);
        string[] names={"6","×","7","=","C"};
        for(int i=0;i<names.Length;i++) {
            string key=names[i];
            Button button=new Button { Name="button"+i, Text=key, AccessibleName=key,
                Location=new Point(20+(i%3)*105,70+(i/3)*75), Size=new Size(95,60) };
            button.Click+=(sender,e)=>Press(key); Calculator.Controls.Add(button);
        }
        Calculator.Shown+=(sender,e)=> {
            Save();
            File.WriteAllText(Path.Combine(Work,"ready.json"),Json.Serialize(new {
                pid=Process.GetCurrentProcess().Id, calculator=Calculator.Handle.ToInt64()
            }));
        };
        Application.Run(Calculator);
    }
}
