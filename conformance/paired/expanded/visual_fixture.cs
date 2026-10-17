using System;using System.IO;using System.Drawing;using System.Windows.Forms;using System.Collections.Generic;using System.Web.Script.Serialization;
public class VisualFixture {
 static string output;static Form main,guard;static Canvas canvas;static TextBox other;static int seed;static bool selected;static List<string> actions=new List<string>();static readonly JavaScriptSerializer json=new JavaScriptSerializer();
 static void Save(){File.WriteAllText(output,json.Serialize(new{completed=selected,wrong=actions.FindAll(x=>x!="purple-circle").Count,actions=actions,guard=other.Text,pid=System.Diagnostics.Process.GetCurrentProcess().Id,window=main.Handle.ToInt64(),width=canvas.Width,height=canvas.Height,seed=seed}));}
 class Canvas:Control {
  public Rectangle target,decoy,triangle;
  public Canvas(){SetStyle(ControlStyles.UserPaint|ControlStyles.AllPaintingInWmPaint|ControlStyles.OptimizedDoubleBuffer,true);AccessibleRole=AccessibleRole.None;TabStop=false;}
  protected override void OnPaint(PaintEventArgs e){base.OnPaint(e);var g=e.Graphics;g.Clear(Color.White);g.SmoothingMode=System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
   g.FillEllipse(Brushes.Purple,target);g.FillRectangle(Brushes.Purple,decoy);g.FillPolygon(Brushes.Orange,new[]{new Point(triangle.X+40,triangle.Y),new Point(triangle.X,triangle.Y+80),new Point(triangle.X+80,triangle.Y+80)});
   using(var font=new Font("Segoe UI",18)){g.DrawString(selected?"Selection confirmed":"Choose the purple circle",font,Brushes.Black,new PointF(20,18));}
  }
  protected override void OnMouseDown(MouseEventArgs e){base.OnMouseDown(e);float dx=e.X-(target.X+40),dy=e.Y-(target.Y+40);bool hit=dx*dx+dy*dy<1600;actions.Add(hit?"purple-circle":"wrong");if(hit)selected=true;Save();Invalidate();}
 }
 [STAThread]public static void Main(string[] args){output=args[0];seed=int.Parse(args[1]);Application.EnableVisualStyles();main=new Form{Text="Visual parity fixture",ClientSize=new Size(600,340),StartPosition=FormStartPosition.Manual,Location=new Point(40,50)};
 canvas=new Canvas{Dock=DockStyle.Fill};int[] columns=seed%2==0?new[]{380,80,230}:new[]{80,380,230};canvas.target=new Rectangle(columns[0],150,80,80);canvas.decoy=new Rectangle(columns[1],150,80,80);canvas.triangle=new Rectangle(columns[2],150,80,80);main.Controls.Add(canvas);
 guard=new Form{Text="Unchanged visual guard",ClientSize=new Size(220,100),StartPosition=FormStartPosition.Manual,Location=new Point(740,60)};other=new TextBox{Text="guard unchanged",AccessibleName="Guard",Bounds=new Rectangle(10,30,190,28)};guard.Controls.Add(other);
 var timer=new Timer{Interval=100};timer.Tick+=(s,e)=>Save();main.Shown+=(s,e)=>{guard.Show();main.Activate();Save();timer.Start();};Application.Run(main);}
}
