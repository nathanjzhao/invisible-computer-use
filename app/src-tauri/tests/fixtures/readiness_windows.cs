// Owned WinForms readiness receiver. No application activation or synthetic input.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows.Forms;

class QuietForm : Form {
    protected override bool ShowWithoutActivation { get { return true; } }
    protected override CreateParams CreateParams {
        get { var p = base.CreateParams; p.ExStyle |= 0x08000000; return p; }
    }
}

class ReadinessReceiver : ApplicationContext {
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    readonly string root, mode;
    readonly JavaScriptSerializer serializer = new JavaScriptSerializer();
    readonly Dictionary<string, object> state = new Dictionary<string, object>();
    readonly List<long> foreground = new List<long>();
    readonly List<object> events = new List<object>();
    readonly QuietForm target, decoy;
    readonly TextBox result = new TextBox();
    readonly ProgressBar spinner = new ProgressBar();
    readonly Timer sampler = new Timer(), updates = new Timer(), lifetime = new Timer();
    readonly Stopwatch operation = new Stopwatch();
    int tick;
    static double Now() { return (DateTime.UtcNow - new DateTime(1970, 1, 1)).TotalSeconds; }

    void Write(string name, object data) {
        string path = Path.Combine(root, name), temporary = path + ".tmp";
        File.WriteAllText(temporary, serializer.Serialize(data));
        if (File.Exists(path)) File.Replace(temporary, path, null); else File.Move(temporary, path);
    }
    void Save() { Write("state.json", state); }
    void Event(string kind) { events.Add(new { kind = kind, at = Now() }); Save(); }
    void Sample() {
        long current = GetForegroundWindow().ToInt64();
        if (!foreground.Contains(current)) foreground.Add(current);
        state["ever_active"] = (bool)state["ever_active"] || current == target.Handle.ToInt64() || current == decoy.Handle.ToInt64();
        state["ownership_samples"] = (int)state["ownership_samples"] + 1;
        state["ownership_sampled_at"] = Now();
        Save();
    }
    QuietForm Window(string title, int x, bool isTarget) {
        var form = new QuietForm { Text = title, Width = 420, Height = 240,
            StartPosition = FormStartPosition.Manual, Left = x, Top = 100 };
        TextBox field = isTarget ? result : new TextBox();
        field.AccessibleName = "Result"; field.Text = isTarget ? "0" : "decoy unchanged";
        field.SetBounds(20, 25, 330, 28); form.Controls.Add(field);
        var button = new Button { Text = "Compute", AccessibleName = "Compute" };
        button.SetBounds(20, 75, 180, 35); button.Click += delegate { Compute(isTarget); };
        form.Controls.Add(button);
        if (isTarget) {
            spinner.AccessibleName = "Loading result";
            spinner.Style = ProgressBarStyle.Marquee; spinner.MarqueeAnimationSpeed = 25;
            spinner.SetBounds(20, 130, 330, 24); spinner.Visible = false;
            form.Controls.Add(spinner);
        }
        return form;
    }
    void Compute(bool isTarget) {
        string key = isTarget ? "target_effects" : "decoy_effects";
        state[key] = (int)state[key] + 1;
        if (!isTarget || mode == "noeffect") { Event(isTarget ? "target_noeffect" : "decoy_compute"); return; }
        state["started_at"] = Now(); state["busy"] = true; state["result"] = "Working";
        result.Text = "Working"; spinner.Visible = true; operation.Restart(); Event("target_started");
        if (mode != "busy-static") updates.Start();
    }
    void Update() {
        if (mode == "delayed" && operation.ElapsedMilliseconds >= 1000) {
            updates.Stop(); result.Text = "42"; spinner.Visible = false;
            state["result"] = "42"; state["busy"] = false; state["completed_at"] = Now(); Event("target_completed"); return;
        }
        result.Text = "Working " + (++tick).ToString(); state["result"] = result.Text; Save();
    }
    public ReadinessReceiver(string root, string mode) {
        this.root = root; this.mode = mode;
        long front = GetForegroundWindow().ToInt64();
        if (front == 0) throw new Exception("an unrelated foreground guard window is required");
        state["mode"] = mode; state["pid"] = Process.GetCurrentProcess().Id;
        state["target_effects"] = 0; state["decoy_effects"] = 0; state["result"] = "0";
        state["busy"] = false; state["ever_active"] = false;
        state["ownership_samples"] = 0;
        state["foreground_windows"] = foreground; state["events"] = events;
        target = Window("ICU Portable Readiness Target", 80, true);
        decoy = Window("ICU Portable Readiness Decoy", 540, false);
        target.Show(); decoy.Show(); Sample();
        Write("ready.json", new { pid = Process.GetCurrentProcess().Id, target_window_id = target.Handle.ToInt64(),
            decoy_window_id = decoy.Handle.ToInt64(), front_window_id = front, toolkit = "winforms" });
        sampler.Interval = 20; sampler.Tick += delegate { Sample(); }; sampler.Start();
        updates.Interval = 30; updates.Tick += delegate { Update(); };
        lifetime.Interval = 35000; lifetime.Tick += delegate { Event("fixture_expired"); ExitThread(); }; lifetime.Start();
    }
    protected override void Dispose(bool disposing) {
        sampler.Dispose(); updates.Dispose(); lifetime.Dispose(); target.Dispose(); decoy.Dispose(); base.Dispose(disposing);
    }
    [STAThread] static void Main(string[] args) {
        if (Environment.GetEnvironmentVariable("ICU_DISPOSABLE_HOST") != "1" ||
            Environment.GetEnvironmentVariable("ICU_E2E_PORTABLE_READINESS") != "1") throw new Exception("explicit disposable host required");
        if (Environment.GetFolderPath(Environment.SpecialFolder.UserProfile).ToLowerInvariant().Contains("nathanzhao")) throw new Exception("never run on Nathan's desktop");
        if (Process.GetCurrentProcess().SessionId == 0 || Environment.GetEnvironmentVariable("SESSIONNAME") != "Console") throw new Exception("owned console session required");
        if (args.Length != 2 || Array.IndexOf(new [] { "delayed", "permanent", "noeffect", "busy-static" }, args[1]) < 0) throw new Exception("invalid fixture arguments");
        if (!Path.IsPathRooted(args[0]) || !Directory.Exists(args[0])) throw new Exception("private work directory required");
        Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
        using (var receiver = new ReadinessReceiver(args[0], args[1])) Application.Run(receiver);
    }
}
