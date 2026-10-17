param(
    # The install directory holding the signed cua-driver.exe and cua-driver-uia.exe
    # (under Program Files, or UIAccess is not granted).
    [Parameter(Mandatory=$true)][string]$Dir,
    # normal:   a target at the daemon's own integrity (started here). Every action lands and no
    #           worker is started.
    # external: a target at higher integrity (High, or SYSTEM) in this session, started by
    #           gaps3-run.ps1, which passes -ExternalDir.
    [Parameter(Mandatory=$true)][ValidateSet('normal','external')][string]$Case,
    # forwarded: every supported action must land through the UIAccess worker.
    # refused:   the worker is not usable (outside Program Files, or removed): an action either
    #            lands anyway (a UIA pattern that crosses integrity) or the call fails saying UIPI.
    # In both, an action that reports success and changed nothing fails the run.
    [ValidateSet('forwarded','refused')][string]$Expect='forwarded',
    [string]$ExternalDir='',
    # Comma separated action names; default is all.
    [string]$Only='',
    # Comma separated action names that the target's integrity makes impossible even with the worker
    # (a SYSTEM window: nothing can be posted to it, and its UIA children cannot be read). They must
    # be refused and change nothing; every other action must land.
    [string]$MustRefuse='',
    [string]$Out=''
)
# Run as the interactive standard user (as-user.ps1), not SYSTEM: SSM's session 0 has no desktop.
# Drives every element-addressed and pixel gesture the driver has against one UiaGestureFixture
# window and reads, from the fixture's own files, what actually reached it.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
# This script runs in a scheduled task's console, which Windows cascades over the desktop where the fixture or the application
# under test is: a window of the harness over the point an action is aimed at makes a click land on the harness. Hide it (a minimized console is restored when the driver puts the previous foreground window back).
Add-Type -Name IcuConsole -Namespace IcuHarness -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow(); [System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr h, int c);'
$icuConsole=[IcuHarness.IcuConsole]::GetConsoleWindow(); if($icuConsole -ne [IntPtr]::Zero){ [void][IcuHarness.IcuConsole]::ShowWindow($icuConsole,0) }
if ((New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run at normal integrity (a filtered token is fine)' }
$driver=Join-Path $Dir 'cua-driver.exe'
$workerExe=Join-Path $Dir 'cua-driver-uia.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps3-'+$Case+'-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$pipe='\\.\pipe\icu-gaps3-'+[guid]::NewGuid().ToString('N')
$session='gaps3'
$report=[ordered]@{case=$Case;expect=$Expect;dir=$Dir;user=$env:USERNAME;driver_sha256=(Get-FileHash $driver -Algorithm SHA256).Hash;
    worker_sha256=$(if(Test-Path $workerExe){(Get-FileHash $workerExe -Algorithm SHA256).Hash}else{$null})}
$procs=@()
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuG3 {
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint a, bool i, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, out int v, int l, out int r);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, byte[] v, int l, out int r);
    public static int UIAccess(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int v, r; bool ok = GetTokenInformation(t, 26, out v, 4, out r); CloseHandle(t); CloseHandle(p); return ok ? v : -1;
    }
    // The integrity RID of a process (0x2000 medium, 0x3000 high, 0x4000 system), -1 when unreadable.
    public static int Integrity(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int need; GetTokenInformation(t, 25, null, 0, out need);
        byte[] b = new byte[need]; int got;
        bool ok = GetTokenInformation(t, 25, b, need, out got); CloseHandle(t); CloseHandle(p);
        if (!ok) return -1;
        IntPtr sid = (IntPtr)(Environment.Is64BitProcess ? BitConverter.ToInt64(b, 0) : BitConverter.ToInt32(b, 0));
        int count = Marshal.ReadByte(sid, 1);
        return Marshal.ReadInt32(sid, 8 + 4 * (count - 1));
    }
    [StructLayout(LayoutKind.Sequential)] public struct PT { public int x, y; }
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(PT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
    // The top-level window at a screen point.
    public static long RootAt(int x, int y) { PT p; p.x = x; p.y = y; IntPtr h = WindowFromPoint(p); return h == IntPtr.Zero ? 0 : (long)GetAncestor(h, 2); }
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    // Make a window the foreground one (the way the driver's own assisted foreground does): a window of a
    // background process is otherwise placed just under the foreground window, whatever SetWindowPos is told.
    public static void Foreground(IntPtr h) {
        uint pid; uint fgThread = GetWindowThreadProcessId(GetForegroundWindow(), out pid), me = GetCurrentThreadId();
        keybd_event(0xFC, 0, 0, UIntPtr.Zero); keybd_event(0xFC, 0, 2, UIntPtr.Zero);
        if (fgThread != 0 && fgThread != me) AttachThreadInput(me, fgThread, true);
        SetForegroundWindow(h);
        if (fgThread != 0 && fgThread != me) AttachThreadInput(me, fgThread, false);
    }
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
    // Class, title, pid and extended style of a window: what covered the target when a foreground click was refused.
    public static string Describe(long h) {
        uint pid; GetWindowThreadProcessId((IntPtr)h, out pid);
        var c = new System.Text.StringBuilder(128); GetClassNameW((IntPtr)h, c, 128); var t = new System.Text.StringBuilder(128); GetWindowTextW((IntPtr)h, t, 128);
        return "0x" + h.ToString("x") + " class " + c + " title '" + t + "' pid " + pid + " exstyle 0x" + ((long)GetWindowLongPtr((IntPtr)h, -20)).ToString("x");
    }
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
    // Foreground window and whether each of two windows is always-on-top, for a failed cover.
    public static string ZInfo(long a, long b) {
        long fg = (long)GetForegroundWindow();
        bool ta = (((long)GetWindowLongPtr((IntPtr)a, -20)) & 0x8) != 0, tb = (((long)GetWindowLongPtr((IntPtr)b, -20)) & 0x8) != 0;
        return "fg=" + fg + " target_topmost=" + ta + " decoy_topmost=" + tb;
    }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, ref RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hgt, uint flags);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
    // Where the window's screenshot starts on screen (the driver's own rule: DWM frame + 1 px).
    public static int[] BitmapOrigin(long hwnd) {
        RECT r; DwmGetWindowAttribute((IntPtr)hwnd, 9, out r, 16); return new int[] { r.left + 1, r.top + 1 };
    }
}
'@
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
function Workers { @(Get-Process -Name 'cua-driver-uia' -ErrorAction SilentlyContinue) }
function Run-Driver([string]$arguments) {
    $info=New-Object Diagnostics.ProcessStartInfo $driver
    $info.Arguments=$arguments
    $info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.CreateNoWindow=$true
    $process=[Diagnostics.Process]::Start($info)
    $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEnd()
    $process.WaitForExit()
    return @{exit=$process.ExitCode;stdout=$stdout.Result.Trim();stderr=$stderr.Trim()}
}
function Call([string]$tool,$arguments) {
    $json=$arguments | ConvertTo-Json -Depth 8 -Compress
    return Run-Driver ('--socket '+$pipe+' call '+$tool+' "'+($json -replace '"','\"')+'"')
}
# Read without taking a lock the fixture's own writes would trip over.
function Read-Text([string]$name) {
    $p=Join-Path $textDir $name
    if(-not (Test-Path $p)){ return '' }
    try {
        $f=[IO.File]::Open($p,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
        try { $r=New-Object IO.StreamReader($f); return $r.ReadToEnd() } finally { $f.Dispose() }
    } catch { return '' }
}
# The pad's drags: "drag downX downY upX upY moves". A drag landed when the pad saw the press, at least 3
# moves with the button held, and a release about 80 px to the right of the press.
function Drags { @((Read-Text 'events.txt') -split "`n" | Where-Object {$_ -like 'drag *'}) }
function Drag-Landed($before) {
    $d=@(Drags); if($d.Count -le $before){return $false}
    $f=$d[-1].Trim().Split(' '); return ([int]$f[5] -ge 3) -and ([Math]::Abs(([int]$f[3]-[int]$f[1])-80) -le 8)
}
function Count-Events([string]$kind) { @((Read-Text 'events.txt') -split "`n" | Where-Object {$_ -like "$kind*"}).Count }
function Scroll-Top { $t=(Read-Text 'scroll.txt').Trim(); if($t){[int]$t}else{0} }
function Elem([string]$label) { @($script:state.elements | Where-Object {$_.label -eq $label})[0] }

try {
    $report.workers_before=(Workers).Count
    if($Case -eq 'normal'){
        $csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
        $fx=Join-Path $work 'UiaGestureFixture.exe'
        & $csc /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll ("/out:$fx") (Join-Path $PSScriptRoot 'UiaGestureFixture.cs') | Out-Null
        if(-not (Test-Path $fx)){throw 'fixture did not compile'}
        $target=Start-Process $fx -ArgumentList ('"'+$work+'"') -PassThru; $procs+=$target
        $textDir=$work
    } else {
        if(-not $ExternalDir){throw '-ExternalDir is required for this case'}
        $textDir=$ExternalDir
    }
    $ready=Wait-For {if(Test-Path (Join-Path $textDir 'target.ready')){(Get-Content (Join-Path $textDir 'target.ready')).Split(' ')}} 15 'target window is not up'
    $hwnd=[int64]$ready[0]; $targetPid=[int]$ready[1]
    $report.target=@{hwnd=$hwnd;pid=$targetPid;integrity=('0x{0:x}' -f [IcuG3]::Integrity($targetPid))}
    $report.self_integrity=('0x{0:x}' -f [IcuG3]::Integrity($PID))

    $env:RUST_LOG='info,uia=debug,uia_worker=debug'
    $serveErr=Join-Path $work 'serve.err'
    $procs+=(Start-Process $driver -ArgumentList 'serve','--no-overlay','--socket',$pipe -WindowStyle Hidden -PassThru -RedirectStandardError $serveErr)
    Wait-For {(Call 'list_windows' @{}).exit -eq 0} 20 'driver did not serve' | Out-Null
    $report.workers_after_serve=(Workers).Count

    function Observe {
        $r=Call 'get_window_state' @{pid=$targetPid;window_id=$hwnd;include_screenshot=$true;include_accessibility_tree=$true;session=$session}
        $script:stateCall=$r
        if($r.exit -ne 0){ return $null }
        return ($r.stdout | ConvertFrom-Json)
    }
    $script:state=Observe
    $report.get_window_state=@{exit=$script:stateCall.exit;elements=@($script:state.elements).Count;screenshot_id=$script:state.screenshot_id;
        error=$(if($script:stateCall.exit -ne 0){($script:stateCall.stdout+' '+$script:stateCall.stderr)}else{$null})}
    if(-not $script:state){ throw 'get_window_state failed against this target' }
    $report.elements_sample=@($script:state.elements | Select-Object -First 8 | ForEach-Object {'{0} {1} {2}' -f $_.element_index,$_.role,$_.label})
    $btn=Elem 'ICU-BTN'; $txt=Elem 'ICU-TEXT'; $lst=Elem 'ICU-LIST'
    # A window whose UIA tree is closed (SYSTEM) is described from its child windows: no accessible
    # names, so the controls are found by what they are.
    if(-not $txt){ $txt=@($script:state.elements | Where-Object {$_.role -eq 'Edit'})[0] }
    if(-not $lst){ $lst=@($script:state.elements | Where-Object {$_.role -eq 'List'})[0] }
    $report.tree_visible=[bool]($btn -and $txt -and $lst)
    # A target whose tree the daemon cannot read has no element ids: those actions are sent with a
    # placeholder token and must be refused, never reported ok.
    $missing=@{element_token='tk_missing_from_this_snapshot';element_index=0}
    if(-not $btn){$btn=$missing}; if(-not $txt){$txt=$missing}; if(-not $lst){$lst=$missing}
    $origin=[IcuG3]::BitmapOrigin($hwnd)
    $layout=(Read-Text 'layout.json') | ConvertFrom-Json
    function PxOf($c) { @([int]($c[0] - $origin[0]), [int]($c[1] - $origin[1])) }
    $bp=PxOf $layout.button; $lp=PxOf $layout.list; $tp=PxOf $layout.text; $pp=PxOf $layout.pad
    $report.pixels=@{button=$bp;list=$lp;text=$tp;pad=$pp;origin=$origin}
    $base=@{pid=$targetPid;window_id=$hwnd;session=$session}
    function A([hashtable]$extra) { $a=$base.Clone(); foreach($k in $extra.Keys){$a[$k]=$extra[$k]}; return $a }
    $sid=$script:state.screenshot_id
    $px=@{screenshot_id=$sid}

    # name, tool, arguments, kind (what to read), what must change
    $actions=@(
        @{name='click_el_bg';   tool='click';        args=(A @{element_token=$btn.element_token}); kind='click'},
        @{name='click_el_fg';   tool='click';        args=(A @{element_token=$btn.element_token;delivery_mode='foreground'}); kind='click'},
        @{name='click_px_bg';   tool='click';        args=(A @{x=$bp[0];y=$bp[1];screenshot_id=$sid}); kind='click'},
        @{name='click_px_fg';   tool='click';        args=(A @{x=$bp[0];y=$bp[1];screenshot_id=$sid;delivery_mode='foreground'}); kind='click'},
        @{name='dblclick_px_fg';tool='double_click'; args=(A @{x=$bp[0];y=$bp[1];screenshot_id=$sid;delivery_mode='foreground'}); kind='click2'},
        @{name='rclick_px_fg';  tool='right_click';  args=(A @{x=$bp[0];y=$bp[1];screenshot_id=$sid;delivery_mode='foreground'}); kind='rclick'},
        @{name='scroll_el_bg';  tool='scroll';       args=(A @{direction='down';amount=3;element_token=$lst.element_token}); kind='scroll'},
        @{name='scroll_win_bg'; tool='scroll';       args=(A @{direction='down';amount=3}); kind='scroll'},
        # What the app's tile wheel sends: a pixel, the frame id, and no delivery mode.
        @{name='scroll_px_bg';  tool='scroll';       args=(A @{direction='down';amount=3;x=$lp[0];y=$lp[1];screenshot_id=$sid;by='line'}); kind='scroll'},
        @{name='scroll_px_fg';  tool='scroll';       args=(A @{direction='down';amount=3;x=$lp[0];y=$lp[1];screenshot_id=$sid;delivery_mode='foreground'}); kind='scroll'},
        @{name='scroll_win_fg'; tool='scroll';       args=(A @{direction='down';amount=3;delivery_mode='foreground'}); kind='scroll'},
        @{name='type_el_bg';    tool='type_text';    args=(A @{element_token=$txt.element_token;text='b1'}); kind='text'; marker='b1'},
        @{name='type_el_fg';    tool='type_text';    args=(A @{element_token=$txt.element_token;text='f1';delivery_mode='foreground'}); kind='text'; marker='f1'},
        @{name='setvalue_el';   tool='set_value';    args=(A @{element_token=$txt.element_token;value='sv1'}); kind='text'; marker='sv1'},
        @{name='key_el_fg';     tool='press_key';    args=(A @{element_token=$txt.element_token;key='x';delivery_mode='foreground'}); kind='text'; marker='x'},
        @{name='type_noel_fg';  tool='type_text';    args=(A @{text='n1';delivery_mode='foreground'}); kind='text'; marker='n1'},
        # A drag across the pad: 80 px to the right, 20 steps. Foreground is forwarded to the worker (SendInput); the
        # background one is a posted-message stream, which UIPI denies into a higher window and must refuse there.
        @{name='drag_px_fg';    tool='drag';         args=(A @{from_x=($pp[0]-40);from_y=$pp[1];to_x=($pp[0]+40);to_y=$pp[1];screenshot_id=$sid;delivery_mode='foreground'}); kind='drag'},
        @{name='drag_px_bg';    tool='drag';         args=(A @{from_x=($pp[0]-40);from_y=$pp[1];to_x=($pp[0]+40);to_y=$pp[1];screenshot_id=$sid;delivery_mode='background'}); kind='drag'},
        # A second window sits on top of the target at the same screen place (started just before these two).
        # The wheel aimed at the target must scroll the target and leave the decoy alone, never the reverse.
        # Same for a drag: another window covers the pad at the press point; the drag must reach the target's pad (the target is raised
        # first) and never the decoy, or refuse. A drag that pressed over the decoy and said ok is the defect.
        @{name='drag_px_fg_decoy'; tool='drag'; args=(A @{from_x=($pp[0]-40);from_y=$pp[1];to_x=($pp[0]+40);to_y=$pp[1];screenshot_id=$sid;delivery_mode='foreground'}); kind='drag_decoy'},
        @{name='scroll_px_bg_decoy'; tool='scroll';  args=(A @{direction='down';amount=3;x=$lp[0];y=$lp[1];screenshot_id=$sid;by='line'}); kind='scroll_decoy'},
        @{name='scroll_px_fg_decoy'; tool='scroll';  args=(A @{direction='down';amount=3;x=$lp[0];y=$lp[1];screenshot_id=$sid;delivery_mode='foreground'}); kind='scroll_decoy'}
    )
    # The two decoy cases need a target nothing has raised yet (an earlier activation can leave it above a
    # new window, so the decoy would not cover it): they run in their own pass, -Only scroll_px_bg_decoy,scroll_px_fg_decoy.
    if($Only){ $want=$Only.Split(','); $actions=@($actions | Where-Object {$want -contains $_.name}) } else { $actions=@($actions | Where-Object {$_.kind -ne 'scroll_decoy' -and $_.kind -ne 'drag_decoy'}) }
    $rows=@()
    $decoyDir=Join-Path $work 'decoy'
    function Decoy-Events([string]$kind) { $p=Join-Path $decoyDir 'events.txt'; if(Test-Path $p){ try { @(([IO.File]::ReadAllText($p)) -split "`n" | Where-Object {$_ -like "$kind*"}).Count } catch { 0 } } else { 0 } }
    function Decoy-Top { $p=Join-Path $decoyDir 'scroll.txt'; if(Test-Path $p){ try { $t=([IO.File]::ReadAllText($p)).Trim(); if($t){return [int]$t} } catch {} }; return -1 }
    # Put the decoy on top of the target and look until it really is what is at the aimed point: a test where the decoy does not cover
    # the target proves nothing. An earlier action may have raised the target, so this runs again before every decoy action.
    function Cover-Decoy($dh,$r) {
        for($i=0;$i -lt 10;$i++){
            [IcuG3]::SetWindowPos($dh,[IntPtr]::Zero,$r.left,$r.top,($r.right-$r.left),($r.bottom-$r.top),0x0040) | Out-Null
            if($i -ge 1){ [IcuG3]::Foreground($dh) }
            Start-Sleep -Milliseconds 300
            if([IcuG3]::RootAt($origin[0]+$lp[0],$origin[1]+$lp[1]) -eq [int64]$dh){break}
        }
    }
    function Start-Decoy {
        $csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
        $fx=Join-Path $work 'DecoyFixture.exe'
        if(-not (Test-Path $fx)){ & $csc /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll ("/out:$fx") (Join-Path $PSScriptRoot 'UiaGestureFixture.cs') | Out-Null }
        New-Item -ItemType Directory -Force $decoyDir | Out-Null
        $p=Start-Process $fx -ArgumentList ('"'+$decoyDir+'"'),'ICU-DECOY' -PassThru
        $script:procs+=$p
        Wait-For {Test-Path (Join-Path $decoyDir 'target.ready')} 15 'decoy did not start' | Out-Null
        $dh=[IntPtr][int64]((Get-Content (Join-Path $decoyDir 'target.ready')).Split(' ')[0])
        # Same place and size as the target (its rectangle is readable whatever its integrity).
        $r=New-Object IcuG3+RECT
        [IcuG3]::GetWindowRect([IntPtr]$hwnd,[ref]$r) | Out-Null
        # Put it on top and look until it really is what is at the aimed point: a test where the decoy does
        # not cover the target proves nothing.
        Cover-Decoy $dh $r
        $script:decoyHwnd=$dh
        $rt=New-Object IcuG3+RECT;$rd=New-Object IcuG3+RECT
        [IcuG3]::GetWindowRect([IntPtr]$hwnd,[ref]$rt) | Out-Null;[IcuG3]::GetWindowRect($dh,[ref]$rd) | Out-Null
        $report.decoy_z=([IcuG3]::ZInfo([int64]$hwnd,[int64]$dh))+(' at='+[IcuG3]::RootAt($origin[0]+$lp[0],$origin[1]+$lp[1])+' target='+$hwnd+' decoy='+[int64]$dh+' trect='+$rt.left+','+$rt.top+','+$rt.right+','+$rt.bottom+' drect='+$rd.left+','+$rd.top+','+$rd.right+','+$rd.bottom+' point='+($origin[0]+$lp[0])+','+($origin[1]+$lp[1]))
    }
    foreach($a in $actions){
        if($a.kind -eq 'scroll_decoy' -or $a.kind -eq 'drag_decoy'){ if(-not $script:decoyHwnd){ Start-Decoy; $report.decoy_started=$true } else { $rr=New-Object IcuG3+RECT; [IcuG3]::GetWindowRect([IntPtr]$hwnd,[ref]$rr) | Out-Null; Cover-Decoy $script:decoyHwnd $rr } }
        $beforeDecoy=Decoy-Top; $beforeDecoyDown=Decoy-Events 'mousedown'
        # What covers the aimed point right before the call: the decoy, or the target.
        $coveredBy=$(if($a.kind -eq 'scroll_decoy' -or $a.kind -eq 'drag_decoy'){ $at=[IcuG3]::RootAt($(if($a.kind -eq 'drag_decoy'){$origin[0]+$pp[0]}else{$origin[0]+$lp[0]}),$(if($a.kind -eq 'drag_decoy'){$origin[1]+$pp[1]}else{$origin[1]+$lp[1]})); if($at -eq [int64]$script:decoyHwnd){'decoy'}elseif($at -eq [int64]$hwnd){'target'}else{"other:$at"} }else{''})
        $beforeClick=Count-Events 'click'; $beforeDown=Count-Events 'mousedown'; $beforeRight=Count-Events 'mousedown Right'
        $beforeScroll=Scroll-Top; $beforeText=Read-Text 'text.txt'; $beforeDrags=@(Drags).Count
        # What was where right before the call (for the decoy cases): the foreground window, who is at the aimed point, the wheel
        # notches each window has seen. A failed foreground wheel is read from these, not guessed.
        $isDecoyKind=($a.kind -eq 'scroll_decoy' -or $a.kind -eq 'drag_decoy')
        $diagBefore=$(if($isDecoyKind){ ([IcuG3]::ZInfo([int64]$hwnd,[int64]$script:decoyHwnd))+' at='+[IcuG3]::RootAt($origin[0]+$lp[0],$origin[1]+$lp[1]) }else{$null})
        $wheelBefore=@((Count-Events 'wheel'),(Decoy-Events 'wheel'))
        $call=Call $a.tool $a.args
        # A UIA write can reach a WinForms control a second or two after the call returns: wait for the
        # effect (at most 3 s, 8 s for a text write) before saying it did not land.
        $deadline=(Get-Date).AddSeconds($(if($a.kind -eq 'text'){8}else{3}))
        do {
            Start-Sleep -Milliseconds 150
            $afterClick=Count-Events 'click'; $afterDown=Count-Events 'mousedown'; $afterRight=Count-Events 'mousedown Right'
            $afterScroll=Scroll-Top; $afterText=Read-Text 'text.txt'
            $landed=switch($a.kind){
                'click'   { $afterClick -gt $beforeClick }
                'click2'  { ($afterClick - $beforeClick) -ge 2 }
                'rclick'  { $afterRight -gt $beforeRight }
                'scroll'  { $afterScroll -gt $beforeScroll }
                'scroll_decoy' { ($coveredBy -eq 'decoy') -and ($afterScroll -gt $beforeScroll) -and ((Decoy-Top) -eq $beforeDecoy) }
                'text'    { ($afterText -ne $beforeText) -and $afterText.Contains($a.marker) }
                'drag'    { Drag-Landed $beforeDrags }
                'drag_decoy' { ($coveredBy -eq 'decoy') -and (Drag-Landed $beforeDrags) -and ((Decoy-Events 'mousedown') -eq $beforeDecoyDown) }
                'refusal' { $afterClick -ne $beforeClick -or $afterDown -ne $beforeDown }
            }
        } while(-not $landed -and (Get-Date) -lt $deadline -and $a.kind -ne 'refusal')
        # A wheel that did not land is given a few more seconds before it is called lost: it may only have been slow.
        $lateScroll=$null
        if($a.kind -eq 'scroll_decoy' -and -not $landed){ Start-Sleep -Seconds 4; $lateScroll=Scroll-Top }
        $diagAfter=$(if($isDecoyKind){ ([IcuG3]::ZInfo([int64]$hwnd,[int64]$script:decoyHwnd))+' at='+[IcuG3]::RootAt($origin[0]+$lp[0],$origin[1]+$lp[1]) }else{$null})
        $wheelAfter=@((Count-Events 'wheel'),(Decoy-Events 'wheel'))
        $said=($call.stdout+' '+$call.stderr)
        # The CLI exits 0 for a structured refusal; only an ok reply with no effect is a silent success.
        # A successful call prints a JSON object; a tool error prints its text and a structured refusal
        # prints {"status":"refused"}. Neither is an ok reply.
        $refused=($call.exit -ne 0) -or (-not $said.TrimStart().StartsWith('{')) -or ($said -match '"status":\s*"refused"') -or ($said -match '"effect":\s*"refused"')
        $rows+=[ordered]@{name=$a.name;tool=$a.tool;exit=$call.exit;refused=[bool]$refused;landed=[bool]$landed;
            physical=($afterDown -gt $beforeDown);scroll=@($beforeScroll,$afterScroll);decoy_scroll=@($beforeDecoy,(Decoy-Top));covered_by=$coveredBy;text_after=$afterText;
            diag_before=$diagBefore;diag_after=$diagAfter;wheel_target_decoy_before=$wheelBefore;wheel_target_decoy_after=$wheelAfter;late_scroll=$lateScroll;
            uipi=($said -match 'UIPI');said=$said.Substring(0,[Math]::Min(300,$said.Length));said_tail=$said.Substring([Math]::Max(0,$said.Length-260));
            cover=$(if($said -match '\(window (0x[0-9a-fA-F]+) is\)'){ [IcuG3]::Describe([Convert]::ToInt64($Matches[1].Substring(2),16)) }else{$null})}
    }
    $report.actions=$rows
    $report.serve_log=$(if(Test-Path $serveErr){((Get-Content $serveErr -Tail 12 | Where-Object {$_ -notmatch 'is available|Update with|Release notes'}) -join ' | ')}else{''})
    $w=Workers
    $report.workers_after=$w.Count
    if($w.Count){$report.worker=@{pid=$w[0].Id;uiaccess=[IcuG3]::UIAccess($w[0].Id);path=$w[0].Path}}
    # An action that says ok and changed nothing is the defect this harness exists to catch.
    $mustList=@($(if($MustRefuse){$MustRefuse.Split(',')}else{@()}))
    $report.must_refuse_ok=(@($rows | Where-Object {$mustList -contains $_.name -and ($_.landed -or -not $_.refused)}).Count -eq 0)
    $rows=@($rows)
    $report.silent_ok=@($rows | Where-Object {-not $_.refused -and -not $_.landed -and $mustList -notcontains $_.name} | ForEach-Object {$_.name})
    $notLanded=@($rows | Where-Object {-not $_.landed -and $mustList -notcontains $_.name} | ForEach-Object {$_.name})
    $report.not_landed=$notLanded
    # A posted drag into a window above the daemon's integrity is what UIPI denies: it has to refuse and say UIPI.
    $drag=@($rows | Where-Object {$_.name -eq 'drag_px_bg'})[0]
    $highTarget=($Case -eq 'external')
    $report.drag_refused_ok=$(if($drag){ if($highTarget -and $Expect -eq 'forwarded'){ (-not $drag.landed) -and $drag.refused -and $drag.uipi } else { $true } } else { $true })
    switch("$Case/$Expect"){
        'normal/forwarded' { $report.pass=($report.silent_ok.Count -eq 0) -and ($notLanded.Count -eq 0) -and ($report.workers_after -eq 0) }
        'external/forwarded' { $report.pass=($report.silent_ok.Count -eq 0) -and ($notLanded.Count -eq 0) -and $report.drag_refused_ok -and $report.must_refuse_ok -and ($report.worker.uiaccess -eq 1) }
        # refused: the worker cannot be used. Anything may land by another route (a pattern that crosses integrity),
        # but nothing may report success and do nothing.
        default { $report.pass=($report.silent_ok.Count -eq 0) -and $report.drag_refused_ok }
    }
} catch {
    $report.error=$_.Exception.Message
    $report.pass=$false
} finally {
    foreach($p in $procs){ try { if($p -and -not $p.HasExited){$p.Kill()} } catch {} }
    foreach($w in (Workers)){ try { Stop-Process -Id $w.Id -Force -ErrorAction Stop } catch {} }
    $json=$report | ConvertTo-Json -Depth 6 -Compress
    if($Out){$json | Set-Content -Encoding UTF8 $Out}
    $json
}
if(-not $report.pass){exit 1}
