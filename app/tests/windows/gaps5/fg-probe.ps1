param(
    # cua-driver.exe to drive (an unsigned build output is fine: Notepad at the daemon's own integrity needs no worker).
    [Parameter(Mandatory=$true)][string]$Driver,
    # How Notepad was started: sched (a task of this user started it before this script ran, as gaps4-realrun does),
    # self (this script starts it).
    [Parameter(Mandatory=$true)][ValidateSet('sched','self')][string]$Start,
    [string]$Label='',
    [string]$Out=''
)
# Run as the interactive standard-user token (as-user.ps1). Foreground delivery into a NORMAL integrity Notepad with no
# UIAccess worker, with a recorder that logs every change of the foreground window (class, title, pid, image) from a 2 ms
# poll, and an oracle that reads Notepad itself: GUI_INMENUMODE for the menu click and Escape, WM_GETTEXT for typing.
# gaps-windows-4 measured the menu click and Escape not landing and the element-addressed actions refused with
# foreground_unavailable "actual foreground HWND 0x...": this finds out what window that is and why.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
Add-Type -ReferencedAssemblies System.Drawing,System.Windows.Forms @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text; using System.Threading; using System.Diagnostics;
// A topmost window that never takes the foreground and counts the presses it receives: a window covering the target.
public class IcuCover5 : System.Windows.Forms.Form {
    public static int Clicks; static IcuCover5 form; static Thread th;
    protected override bool ShowWithoutActivation { get { return true; } }
    protected override System.Windows.Forms.CreateParams CreateParams { get { var cp = base.CreateParams; cp.ExStyle |= 0x8 | 0x08000000 | 0x80; return cp; } }
    public static void Open(int x, int y, int w, int h) {
        Clicks = 0; var ready = new ManualResetEvent(false);
        th = new Thread(() => { form = new IcuCover5(); form.FormBorderStyle = System.Windows.Forms.FormBorderStyle.None; form.StartPosition = System.Windows.Forms.FormStartPosition.Manual;
            form.Bounds = new System.Drawing.Rectangle(x, y, w, h); form.TopMost = true; form.BackColor = System.Drawing.Color.Crimson; form.ShowInTaskbar = false;
            form.MouseDown += (o, e) => { Clicks++; }; form.Shown += (o, e) => ready.Set(); System.Windows.Forms.Application.Run(form); });
        th.SetApartmentState(ApartmentState.STA); th.IsBackground = true; th.Start(); ready.WaitOne(5000);
    }
    public static void Dismiss() { try { form.Invoke((Action)(() => form.Close())); } catch { } }
}
public static class IcuFg5 {
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint c);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b; }
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);
    public static string PointerState() { POINT p; GetCursorPos(out p); return "pointer (" + p.x + "," + p.y + ") left " + ((GetAsyncKeyState(1) & 0x8000) != 0 ? "DOWN" : "up") + " over " + Describe(WindowFromPoint(p)); }
    public static int ProbeX = -1, ProbeY = -1;
    public static string ProbeState() { if (ProbeX < 0) return ""; POINT p; p.x = ProbeX; p.y = ProbeY; return "window at the click point (" + ProbeX + "," + ProbeY + ") is " + Describe(WindowFromPoint(p)); }
    public static string CursorInfo() { POINT p; GetCursorPos(out p); return "cursor (" + p.x + "," + p.y + ") over " + Describe(WindowFromPoint(p)); }
    public static string RectOf(long h) { RECT r; GetWindowRect((IntPtr)h, out r); return "(" + r.l + "," + r.t + ")-(" + r.r + "," + r.b + ")"; }
    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr h);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool SystemParametersInfo(uint a, uint p, ref uint v, uint f);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageNameW(IntPtr p, int f, StringBuilder s, ref int n);
    [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO { public int cbSize, flags; public IntPtr hwndActive, hwndFocus, hwndCapture, hwndMenuOwner, hwndMoveSize, hwndCaret; public int l, t, r, b; }
    [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint tid, ref GUITHREADINFO i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeoutW(IntPtr h, uint m, IntPtr w, StringBuilder l, uint f, uint t, out IntPtr res);
    public static string Describe(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        uint pid; GetWindowThreadProcessId(h, out pid);
        var t = new StringBuilder(128); GetWindowTextW(h, t, 128); var c = new StringBuilder(128); GetClassNameW(h, c, 128);
        string img = ""; IntPtr p = OpenProcess(0x1000, false, pid);
        if (p != IntPtr.Zero) { var sb = new StringBuilder(512); int n = 512; if (QueryFullProcessImageNameW(p, 0, sb, ref n)) img = System.IO.Path.GetFileName(sb.ToString()); CloseHandle(p); }
        return "0x" + ((long)h).ToString("x") + " " + c + " '" + t + "' pid " + pid + " " + img + " root 0x" + ((long)GetAncestor(h, 2)).ToString("x") + " parent 0x" + ((long)GetParent(h)).ToString("x") + (IsWindowVisible(h) ? "" : " (hidden)");
    }
    public static string Foreground() { return Describe(GetForegroundWindow()); }
    public static long ForegroundHandle() { return (long)GetForegroundWindow(); }
    public static uint LockTimeout() { uint v = 0; SystemParametersInfo(0x2000, 0, ref v, 0); return v; }
    // GUI thread flags of the thread that owns hwnd: GUI_INMENUMODE 0x4, GUI_POPUPMENUMODE 0x10
    public static int ThreadFlags(long hwnd) {
        uint pid; uint tid = GetWindowThreadProcessId((IntPtr)hwnd, out pid);
        var i = new GUITHREADINFO(); i.cbSize = Marshal.SizeOf(i); if (!GetGUIThreadInfo(tid, ref i)) return -1; return i.flags;
    }
    public static string Text(long hwnd) {
        IntPtr res; var sb = new StringBuilder(65536);
        IntPtr ok = SendMessageTimeoutW((IntPtr)hwnd, 0xD, (IntPtr)65535, sb, 2, 2000, out res); return ok == IntPtr.Zero ? null : sb.ToString();
    }
    // Whether the process has a visible drop-down menu window (class #32768) open
    public static bool PopupMenuOpen(uint pid) {
        bool found = false;
        EnumWindows((h, l) => { if (!IsWindowVisible(h)) return true; var c = new StringBuilder(64); GetClassNameW(h, c, 64); if (c.ToString() != "#32768") return true;
            uint p; GetWindowThreadProcessId(h, out p); if (p == pid) found = true; return true; }, IntPtr.Zero);
        return found;
    }
    public static string[] TopLevel() {
        var list = new List<string>();
        EnumWindows((h, l) => { if (IsWindowVisible(h) && GetWindow(h, 4) == IntPtr.Zero) list.Add(Describe(h)); return true; }, IntPtr.Zero);
        return list.ToArray();
    }
    // Recorder: every change of the foreground window, with milliseconds since start.
    static Thread th; static volatile bool stop; static List<string> log = new List<string>(); static Stopwatch sw;
    public static void StartRecorder() {
        stop = false; log = new List<string>(); sw = Stopwatch.StartNew();
        th = new Thread(() => { long last = -1; string lastPointer = ""; while (!stop) { long f = ForegroundHandle(); if (f != last) { lock (log) log.Add(sw.ElapsedMilliseconds + " ms: FOREGROUND " + Describe((IntPtr)f)); last = f; }
            string ps = PointerState() + " | " + ProbeState(); if (ps != lastPointer) { lock (log) log.Add(sw.ElapsedMilliseconds + " ms: " + ps); lastPointer = ps; } Thread.Sleep(2); } });
        th.IsBackground = true; th.Start();
    }
    public static void Mark(string m) { lock (log) log.Add(sw.ElapsedMilliseconds + " ms: ---- " + m); }
    public static string[] StopRecorder() { stop = true; th.Join(); lock (log) return log.ToArray(); }
}
'@
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type 'using System; using System.Runtime.InteropServices; public static class IcuDwm5 { [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s); [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l,t,r,b; } }'
$os=Get-CimInstance Win32_OperatingSystem
$report=[ordered]@{label=$Label;start=$Start;user=$env:USERNAME;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;os=$os.Caption;build=$os.BuildNumber;
    foreground_lock_timeout_ms=[IcuFg5]::LockTimeout();foreground_at_start=[IcuFg5]::Foreground();top_level_at_start=@([IcuFg5]::TopLevel())}
$pipe='\\.\pipe\icu-gaps5-'+[guid]::NewGuid().ToString('N')
$procs=@(); $notepad=$null; $recording=$false
function Run-Driver([string]$arguments) {
    $info=New-Object Diagnostics.ProcessStartInfo $Driver
    $info.Arguments=$arguments; $info.UseShellExecute=$false; $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true; $info.CreateNoWindow=$true
    $p=[Diagnostics.Process]::Start($info); $o=$p.StandardOutput.ReadToEndAsync(); $e=$p.StandardError.ReadToEnd(); $p.WaitForExit()
    return @{exit=$p.ExitCode;stdout=$o.Result.Trim();stderr=$e.Trim()}
}
function Call([string]$tool,$arguments) { $json=$arguments | ConvertTo-Json -Depth 8 -Compress; Run-Driver ('--socket '+$pipe+' call '+$tool+' "'+($json -replace '"','\"')+'"') }
function Find-Class($ae,[string]$class) { $ae.FindFirst([Windows.Automation.TreeScope]::Descendants,(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ClassNameProperty,$class))) }
try {
    if($Start -eq 'self'){ $notepad=Start-Process notepad.exe -PassThru; Start-Sleep -Seconds 3 }
    $deadline=(Get-Date).AddSeconds(25); $win=$null
    while((Get-Date) -lt $deadline -and -not $win){
        $win=@([IcuFg5]::TopLevel() | Where-Object {$_ -match 'Notepad' -and $_ -match 'notepad.exe'})[0]; if(-not $win){Start-Sleep -Milliseconds 300}
    }
    if(-not $win){ throw 'no Notepad window' }
    $hwnd=[Convert]::ToInt64(($win -split ' ')[0].Substring(2),16); $np=[int](($win -split 'pid ')[1] -split ' ')[0]
    $report.notepad=$win
    $report.foreground_before_driver=[IcuFg5]::Foreground()
    $serveErr=Join-Path $env:TEMP ('g5-serve-'+[guid]::NewGuid().ToString('N')+'.err')
    $env:RUST_LOG='debug'
    $procs+=(Start-Process $Driver -ArgumentList 'serve','--no-overlay','--socket',$pipe -WindowStyle Hidden -PassThru -RedirectStandardError $serveErr)
    $d=(Get-Date).AddSeconds(25); while((Call 'list_windows' @{}).exit -ne 0 -and (Get-Date) -lt $d){Start-Sleep -Milliseconds 300}
    $st=Call 'get_window_state' @{pid=$np;window_id=$hwnd;include_screenshot=$true;include_accessibility_tree=$true;session='g5'}
    $state=$st.stdout | ConvertFrom-Json
    $sid=$state.screenshot_id
    $el=@($state.elements | Where-Object {$_.role -match '^(Edit|Document)$'})[0]
    $report.elements=@($state.elements).Count
    $report.window_rect=[IcuFg5]::RectOf($hwnd)
    $ae=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$hwnd)
    $edit0=Find-Class $ae 'Edit'
    $rect=$edit0.Current.BoundingRectangle
    $r=New-Object IcuDwm5+RECT; [IcuDwm5]::DwmGetWindowAttribute([IntPtr]$hwnd,9,[ref]$r,16) | Out-Null
    # the menu bar sits directly above the Edit control: 20 px in from its left edge and 10 px above its top edge is the File menu
    $sx=[int]($rect.X+20); $sy=[int]($rect.Y-10)
    $px=$sx - ($r.l+1); $py=$sy - ($r.t+1)
    $report.file_item_screen_centre=('({0},{1})' -f $sx,$sy); $report.frame_origin=('({0},{1})' -f ($r.l+1),($r.t+1)); $report.click_px=@($px,$py)
    $edit=Find-Class $ae 'Edit'
    $editHwnd=[int64]$edit.Current.NativeWindowHandle
    $base=@{pid=$np;window_id=$hwnd;session='g5'}
    function A([hashtable]$extra){ $a=$base.Clone(); foreach($k in $extra.Keys){$a[$k]=$extra[$k]}; $a }
    $editText={ 'edit text: '+[IcuFg5]::Text($editHwnd) }
    # $expect gets the step result and returns $null when it is as it should be, or why it is not
    function Step([string]$name,[string]$tool,$callArgs,[scriptblock]$observe,[scriptblock]$expect){
        [IcuFg5]::Mark("BEGIN $name")
        $fgBefore=[IcuFg5]::Foreground()
        $c=Call $tool $callArgs
        Start-Sleep -Milliseconds 600
        [IcuFg5]::Mark("END $name")
        $said=($c.stdout+' '+$c.stderr)
        $row=[ordered]@{name=$name;exit=$c.exit;said=$said.Substring(0,[Math]::Min(500,$said.Length)).Replace("`n",' ');foreground_before=$fgBefore;foreground_after=[IcuFg5]::Foreground();cursor=[IcuFg5]::CursorInfo();observed=(& $observe)}
        $row.problem=(& $expect $row $c)
        $row
    }
    $inMenu={ ([IcuFg5]::ThreadFlags($hwnd) -band 0x14) -ne 0 }
    $popup={ [IcuFg5]::PopupMenuOpen([uint32]$np) }
    $menuState={ 'thread flags 0x{0:x}, in menu: {1}, drop-down open: {2}' -f [IcuFg5]::ThreadFlags($hwnd),(& $inMenu),(& $popup) }
    $nowFg={ [IcuFg5]::ForegroundHandle() }
    [IcuFg5]::ProbeX=$sx; [IcuFg5]::ProbeY=$sy
    $prevForeground=[IcuFg5]::ForegroundHandle()   # what the driver should put back (nothing, when Notepad already is the foreground window)
    [IcuFg5]::StartRecorder(); $recording=$true
    $steps=@()
    # a foreground click on the File menu opens it and leaves it open, with Notepad still the foreground window
    $steps+=Step 'click_file_menu_fg' 'click' (A @{x=$px;y=$py;screenshot_id=$sid;delivery_mode='foreground'}) $menuState {
        param($row,$c) if(($c.stdout+$c.stderr) -match 'foreground_unavailable'){ return "the click was refused: $($row.said)" }
        if(-not (& $popup)){ return 'the File drop-down is not open 600 ms after the click (the restore of the previous foreground closed it)' }
        if((& $nowFg) -ne $hwnd){ return 'Notepad is not the foreground window while its menu is open' } }
    # the first Escape closes the drop-down (the menu bar stays active), the second leaves menu mode, and then the previous
    # foreground window comes back
    $steps+=Step 'key_escape_fg_1' 'press_key' (A @{key='escape';delivery_mode='foreground'}) $menuState {
        param($row,$c) if(($c.stdout+$c.stderr) -match 'foreground_unavailable'){ return "Escape was refused: $($row.said)" }; if(& $popup){ return 'the drop-down is still open after Escape' } }
    $steps+=Step 'key_escape_fg_2' 'press_key' (A @{key='escape';delivery_mode='foreground'}) $menuState {
        param($row,$c) if(($c.stdout+$c.stderr) -match 'foreground_unavailable'){ return "Escape was refused: $($row.said)" }; if(& $inMenu){ return 'Notepad is still in menu mode after the second Escape' }
        if($prevForeground -ne $hwnd -and (& $nowFg) -eq $hwnd){ return 'the previous foreground window was not put back after the menu closed' } }
    $steps+=Step 'type_el_fg' 'type_text' (A @{element_token=$el.element_token;text='EL5';delivery_mode='foreground'}) $editText {
        param($row,$c) if($c.exit -ne 0){ return "refused: $($row.said)" }; if(-not ([string]$row.observed).Contains('EL5')){ return 'the text did not arrive' } }
    $steps+=Step 'key_el_fg' 'press_key' (A @{element_token=$el.element_token;key='x';delivery_mode='foreground'}) $editText {
        param($row,$c) if($c.exit -ne 0){ return "refused: $($row.said)" }; if(-not ([string]$row.observed).Contains('EL5x')){ return 'the key did not arrive' } }
    $steps+=Step 'type_noel_fg' 'type_text' (A @{text='NOEL5';delivery_mode='foreground'}) $editText {
        param($row,$c) if($c.exit -ne 0){ return "refused: $($row.said)" }; if(-not ([string]$row.observed).Contains('NOEL5')){ return 'the text did not arrive' } }
    # a topmost window that cannot be activated covers the File menu: the click must be refused with nothing sent, not land on it
    [IcuCover5]::Open($sx-60,$sy-25,160,50); Start-Sleep -Milliseconds 400
    $steps+=Step 'click_file_menu_fg_covered' 'click' (A @{x=$px;y=$py;screenshot_id=$sid;delivery_mode='foreground'}) { 'cover received {0} press(es); {1}' -f [IcuCover5]::Clicks,(& $menuState) } {
        param($row,$c) if([IcuCover5]::Clicks -ne 0){ return "the click landed on the covering window ($([IcuCover5]::Clicks) press)" }
        if(($c.stdout+$c.stderr) -notmatch 'foreground_unavailable'){ return 'the click was reported as done though a window covered the point' }
        if(& $inMenu){ return 'a menu opened although a window covered it' } }
    [IcuCover5]::Dismiss()
    $report.steps=$steps
    $report.problems=@($steps | Where-Object {$_.problem} | ForEach-Object {'{0}: {1}' -f $_.name,$_.problem})
    $report.pass=($report.problems.Count -eq 0)
    $report.foreground_log=@([IcuFg5]::StopRecorder()); $recording=$false
    $report.serve_log=$(if(Test-Path $serveErr){@(Get-Content $serveErr | ForEach-Object {[string]$_} | Where-Object {$_ -notmatch 'is available|Update with|Release notes'} | Select-Object -Last 150)}else{@()})
} catch {
    $report.error=$_.Exception.Message+' @ '+$_.InvocationInfo.PositionMessage
    if($recording){ try{ $report.foreground_log=@([IcuFg5]::StopRecorder()) }catch{} }
} finally {
    foreach($p in $procs){ try { if($p -and -not $p.HasExited){$p.Kill()} } catch {} }
    if($notepad){ try{ Stop-Process -Id $notepad.Id -Force }catch{} }
    $json=$report | ConvertTo-Json -Depth 7
    if($Out){$json | Set-Content -Encoding UTF8 $Out}
    $json
}
