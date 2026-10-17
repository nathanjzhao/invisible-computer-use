// A recording stand-in for the app's driver sidecar, for app-wheel.ps1.
//
// The app starts `cua-driver serve --socket \\.\pipe\icu-cua-driver-<pid>` and sends it one JSON
// request per line. Installed as cua-driver.exe beside the real driver (renamed cua-driver-real.exe),
// this serves that same pipe, starts the real driver on "<pipe>-real", relays every byte both ways
// and appends each line to a log:
//   C> <request line the app sent>
//   S> <reply line the driver sent>
// so a run can show what the app really sent for a gesture (a `scroll` with its screenshot_id) and
// which frame ids the driver had issued. Any other invocation (the app's CLI fallback) is passed
// straight to the real driver. The real driver is killed with the tap (job object).
using System;
using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

static class PipeTap
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateJobObject(IntPtr a, string name);
    [DllImport("kernel32.dll")] static extern bool SetInformationJobObject(IntPtr job, int cls, IntPtr info, int len);
    [DllImport("kernel32.dll")] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

    static readonly object LogLock = new object();
    static string LogPath;

    static void Log(string line)
    {
        lock (LogLock)
        {
            try { File.AppendAllText(LogPath, DateTime.UtcNow.ToString("HH:mm:ss.fff") + " " + line + "\n"); } catch { }
        }
    }

    // JOBOBJECT_EXTENDED_LIMIT_INFORMATION with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE (0x2000)
    static void KillWithMe(Process child)
    {
        IntPtr job = CreateJobObject(IntPtr.Zero, null);
        int size = IntPtr.Size == 8 ? 144 : 112;
        IntPtr info = Marshal.AllocHGlobal(size);
        for (int i = 0; i < size; i++) Marshal.WriteByte(info, i, 0);
        Marshal.WriteInt32(info, 16, 0x2000); // BasicLimitInformation.LimitFlags
        SetInformationJobObject(job, 9, info, size);
        AssignProcessToJobObject(job, child.Handle);
    }

    // The app's protocol is not necessarily one JSON line per read (it may frame its messages), so each chunk
    // that passes is logged as text with control characters shown as a dot.
    // Both pipe streams are overlapped (Asynchronous): a blocking Read and a Write on one synchronous handle
    // would serialize and deadlock the relay.
    static void Pump(Stream from, Stream to, string tag)
    {
        var buf = new byte[65536];
        try
        {
            while (true)
            {
                int n = from.Read(buf, 0, buf.Length);
                if (n <= 0) break;
                to.Write(buf, 0, n);
                to.Flush();
                var text = new StringBuilder();
                foreach (char c in Encoding.UTF8.GetString(buf, 0, Math.Min(n, 4000)))
                    text.Append(c < 32 || c == 127 ? '.' : c);
                Log(tag + "> " + text);
            }
        }
        catch { }
        finally { try { to.Dispose(); } catch { } }
    }

    static string Quote(string a) { return a.Length > 0 && a.IndexOfAny(new[] { ' ', '\t', '"' }) < 0 ? a : "\"" + a.Replace("\"", "\\\"") + "\""; }

    static string Join(System.Collections.Generic.IEnumerable<string> args)
    {
        var parts = new System.Collections.Generic.List<string>();
        foreach (var a in args) parts.Add(Quote(a));
        return string.Join(" ", parts.ToArray());
    }

    static int Main(string[] args)
    {
        string dir = AppDomain.CurrentDomain.BaseDirectory;
        string real = Path.Combine(dir, "cua-driver-real.exe");
        LogPath = Environment.GetEnvironmentVariable("ICU_TAP_LOG") ?? Path.Combine(dir, "tap.log");
        int socket = Array.IndexOf(args, "--socket");
        if (Array.IndexOf(args, "serve") < 0 || socket < 0 || socket + 1 >= args.Length)
        {
            // The app's own CLI calls (reads such as list_windows) read this process's stdout: relay all three streams.
            var psi = new ProcessStartInfo(real, Join(args))
            {
                UseShellExecute = false, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
            };
            using (var p = Process.Start(psi))
            {
                var so = Console.OpenStandardOutput();
                var se = Console.OpenStandardError();
                var t1 = new Thread(() => { try { p.StandardOutput.BaseStream.CopyTo(so); so.Flush(); } catch { } });
                var t2 = new Thread(() => { try { p.StandardError.BaseStream.CopyTo(se); se.Flush(); } catch { } });
                t1.Start(); t2.Start();
                new Thread(() =>
                {
                    try { Console.OpenStandardInput().CopyTo(p.StandardInput.BaseStream); } catch { }
                    try { p.StandardInput.Close(); } catch { }
                }) { IsBackground = true }.Start();
                p.WaitForExit();
                t1.Join(); t2.Join();
                Log("tap: cli " + Join(args).Substring(0, Math.Min(200, Join(args).Length)) + " -> exit " + p.ExitCode);
                return p.ExitCode;
            }
        }
        string pipe = args[socket + 1];
        const string prefix = @"\\.\pipe\";
        string name = pipe.StartsWith(prefix) ? pipe.Substring(prefix.Length) : pipe;
        string realName = name + "-real";
        var forReal = (string[])args.Clone();
        forReal[socket + 1] = prefix + realName;
        // The app keeps the sidecar alive by holding its stdin open and closing it to stop it
        // (CUA_DRIVER_PARENT_LIVENESS_STDIN): pass that through to the real driver explicitly.
        var psi2 = new ProcessStartInfo(real, Join(forReal)) { UseShellExecute = false, RedirectStandardInput = true };
        var child = Process.Start(psi2);
        KillWithMe(child);
        child.EnableRaisingEvents = true;
        child.Exited += (o, e) => Log("tap: real driver exited with " + child.ExitCode);
        new Thread(() =>
        {
            try
            {
                var stdin = Console.OpenStandardInput();
                var b = new byte[4096];
                while (stdin.Read(b, 0, b.Length) > 0) { }
            }
            catch { }
            try { child.StandardInput.Close(); } catch { }
            Log("tap: stdin closed, real driver told to stop");
        }) { IsBackground = true }.Start();
        Log("tap: serving " + name + " over real driver pid " + child.Id);
        // Several instances wait at once, so a client never meets a pipe with no free instance (the app treats
        // that gap as a dead driver and restarts it, which would throw away the frame ids it holds).
        for (int i = 0; i < 4; i++) new Thread(() => AcceptLoop(name, realName)) { IsBackground = true }.Start();
        Thread.Sleep(Timeout.Infinite);
        return 0;
    }

    static void AcceptLoop(string name, string realName)
    {
        while (true)
        {
            NamedPipeServerStream server;
            try
            {
                server = new NamedPipeServerStream(name, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
                server.WaitForConnection();
            }
            catch (Exception e) { Log("tap accept error: " + e.Message); Thread.Sleep(50); continue; }
            Log("tap: client connected");
            var captured = server;
            new Thread(() =>
            {
                try
                {
                    var client = new NamedPipeClientStream(".", realName, PipeDirection.InOut, PipeOptions.Asynchronous);
                    client.Connect(15000);
                    Log("tap: real driver connected");
                    var up = new Thread(() => Pump(captured, client, "C"));
                    up.IsBackground = true;
                    up.Start();
                    Pump(client, captured, "S");
                }
                catch (Exception e) { Log("tap error: " + e.Message); try { captured.Dispose(); } catch { } }
            }) { IsBackground = true }.Start();
        }
    }
}
