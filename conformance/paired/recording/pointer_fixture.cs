using System;using System.Drawing;using System.IO;using System.Collections.Generic;using System.Windows.Forms;
class PointerFixture:Form{
 string path;List<string> events=new List<string>();Panel pad;
 void Log(string kind,int button,int delta){events.Add("{\"kind\":\""+kind+"\",\"button\":"+button+",\"delta\":"+delta+"}");File.WriteAllText(path,"["+String.Join(",",events)+"]");}
 public PointerFixture(string output){path=output;Text="Pointer capture fixture";StartPosition=FormStartPosition.Manual;Location=new Point(100,100);ClientSize=new Size(560,300);
  pad=new Panel{Dock=DockStyle.Fill,BackColor=Color.FromArgb(30,64,102),AccessibleName="Pointer pad",AccessibleRole=AccessibleRole.Pane,TabStop=true};Controls.Add(pad);
  pad.MouseDown+=(s,e)=>{pad.Focus();Log("down",(int)e.Button,0);};pad.MouseUp+=(s,e)=>Log("up",(int)e.Button,0);pad.MouseMove+=(s,e)=>{if(e.Button!=MouseButtons.None)Log("drag",(int)e.Button,0);};pad.MouseWheel+=(s,e)=>Log("scroll",0,e.Delta);
  pad.Paint+=(s,e)=>e.Graphics.DrawString("Pointer capture fixture\nSynthetic input only",new Font("Segoe UI",18),Brushes.White,28,30);File.WriteAllText(path,"[]");Shown+=(s,e)=>{Activate();pad.Focus();};
 }
 [STAThread]static void Main(string[] a){Application.EnableVisualStyles();Application.Run(new PointerFixture(a[0]));}
}
