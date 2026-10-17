param(
    # The install directory holding the signed cua-driver.exe and cua-driver-uia.exe (under Program Files).
    [Parameter(Mandatory=$true)][string]$Dir,
    [Parameter(Mandatory=$true)][ValidateSet('notepad','cmd','regedit','services','taskmgr')][string]$App,
    # high, system or normal: what integrity the real application was started at (normal is the control: no worker needed) (gaps4-realrun.ps1 does that).
    [Parameter(Mandatory=$true)][ValidateSet('high','system','normal')][string]$Target,
    [string]$Out=''
)
# Run as the interactive standard-user token (as-user.ps1), not SYSTEM: SSM's session 0 has no desktop.
# Drives a REAL application that runs above the daemon's integrity (not a fixture) through the installed,
# really signed driver and its UIAccess worker, and reads what happened from the application itself:
#   notepad   the text of its Edit control (WM_GETTEXT, which Windows lets a lower integrity read), a selection by drag
#   cmd       a file the typed command wrote
#   regedit   a registry key the Edit > New > Key menu path created, a splitter dragged (the list view's rectangle)
#   services  (mmc) pixels of the list before and after a click, a wheel and typed text
#   taskmgr   pixels before and after a click and a wheel
# An action that reports success and changed nothing is the defect this exists to catch.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
# This script runs in a scheduled task's console, which Windows cascades over the desktop where the fixture or the application
# under test is: a window of the harness over the point an action is aimed at makes a click land on the harness. Hide it (a minimized console is restored when the driver puts the previous foreground window back).
Add-Type -Name IcuConsole -Namespace IcuHarness -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow(); [System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr h, int c);'
$icuConsole=[IcuHarness.IcuConsole]::GetConsoleWindow(); if($icuConsole -ne [IntPtr]::Zero){ [void][IcuHarness.IcuConsole]::ShowWindow($icuConsole,0) }
$driver=Join-Path $Dir 'cua-driver.exe'
$workerExe=Join-Path $Dir 'cua-driver-uia.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps4-'+$App+'-'+$Target+'-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$pipe='\\.\pipe\icu-gaps4-'+[guid]::NewGuid().ToString('N')
$session='gaps4'
$report=[ordered]@{app=$App;target=$Target;dir=$Dir;user=$env:USERNAME}
foreach($f in 'cua-driver.exe','cua-driver-uia.exe'){
    $p=Join-Path $Dir $f
    if(Test-Path $p){ $s=Get-AuthenticodeSignature $p; $report[$f]=@{sha256=(Get-FileHash $p -Algorithm SHA256).Hash;status=[string]$s.Status;subject=$s.SignerCertificate.Subject;thumbprint=$s.SignerCertificate.Thumbprint} }
}
$procs=@()
Add-Type -ReferencedAssemblies System.Drawing @'
using System; using System.Collections.Generic; using System.Drawing; using System.Drawing.Imaging; using System.Runtime.InteropServices; using System.Text;
public static class IcuG4 {
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint a, bool i, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, out int v, int l, out int r);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, byte[] v, int l, out int r);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageNameW(IntPtr p, int f, StringBuilder s, ref int n);
    public static int UIAccess(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int v, r; bool ok = GetTokenInformation(t, 26, out v, 4, out r); CloseHandle(t); CloseHandle(p); return ok ? v : -1;
    }
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
    public static string Image(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return "";
        var sb = new StringBuilder(1024); int n = 1024; bool ok = QueryFullProcessImageNameW(p, 0, sb, ref n); CloseHandle(p); return ok ? sb.ToString() : "";
    }
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint c);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, ref RECT r);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeoutW(IntPtr h, uint m, IntPtr w, StringBuilder l, uint f, uint t, out IntPtr res);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeoutW(IntPtr h, uint m, IntPtr w, IntPtr l, uint f, uint t, out IntPtr res);
    [DllImport("user32.dll")] static extern int GetScrollPos(IntPtr h, int bar);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
    // Visible top-level windows whose title matches (a .NET regex), as "hwnd|pid|title".
    public static string[] Windows(string titleRegex) {
        var re = new System.Text.RegularExpressions.Regex(titleRegex); var list = new List<string>();
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h) || GetWindow(h, 4) != IntPtr.Zero) return true;
            var sb = new StringBuilder(512); GetWindowTextW(h, sb, 512);
            if (sb.Length > 0 && re.IsMatch(sb.ToString())) { uint pid; GetWindowThreadProcessId(h, out pid); list.Add((long)h + "|" + pid + "|" + sb); }
            return true; }, IntPtr.Zero);
        return list.ToArray();
    }
    // Child windows as "hwnd|class|left|top|right|bottom|text" (all depths).
    public static string[] Children(long hwnd) {
        var list = new List<string>();
        EnumChildWindows((IntPtr)hwnd, (h, l) => {
            var c = new StringBuilder(256); GetClassNameW(h, c, 256); RECT r = new RECT(); GetWindowRect(h, ref r);
            list.Add((long)h + "|" + c + "|" + r.left + "|" + r.top + "|" + r.right + "|" + r.bottom);
            return true; }, IntPtr.Zero);
        return list.ToArray();
    }
    // WM_GETTEXT, which Windows lets a lower integrity process send; null when it is refused.
    public static string Text(long hwnd) {
        IntPtr res; var sb = new StringBuilder(65536);
        IntPtr ok = SendMessageTimeoutW((IntPtr)hwnd, 0xD, (IntPtr)65535, sb, 2, 2000, out res);
        return ok == IntPtr.Zero ? null : sb.ToString();
    }
    public static int ScrollPos(long hwnd) { return GetScrollPos((IntPtr)hwnd, 1); }
    // The first visible line of an edit control (EM_GETFIRSTVISIBLELINE); -1 when the message is refused.
    public static int FirstLine(long hwnd) {
        IntPtr res; IntPtr ok = SendMessageTimeoutW((IntPtr)hwnd, 0xCE, IntPtr.Zero, IntPtr.Zero, 2, 2000, out res);
        return ok == IntPtr.Zero ? -1 : (int)res;
    }
    public static int[] Rect(long hwnd) { RECT r = new RECT(); GetWindowRect((IntPtr)hwnd, ref r); return new int[] { r.left, r.top, r.right, r.bottom }; }
    public static int[] BitmapOrigin(long hwnd) { RECT r; DwmGetWindowAttribute((IntPtr)hwnd, 9, out r, 16); return new int[] { r.left + 1, r.top + 1 }; }
    public static byte[] Shot(int x, int y, int w, int h) {
        using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb)) {
            using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(x, y, 0, 0, new Size(w, h));
            var d = bmp.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            byte[] bytes = new byte[d.Stride * h]; Marshal.Copy(d.Scan0, bytes, 0, bytes.Length); bmp.UnlockBits(d); return bytes; }
    }
    public static int Diff(byte[] a, byte[] b) { int n = 0; for (int i = 0; i + 3 < a.Length && i + 3 < b.Length; i += 4) if (a[i] != b[i] || a[i+1] != b[i+1] || a[i+2] != b[i+2]) n++; return n; }
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    public static long Foreground() { return (long)GetForegroundWindow(); }
    [StructLayout(LayoutKind.Sequential)] struct GUITHREADINFO { public int cbSize, flags; public IntPtr hwndActive, hwndFocus, hwndCapture, hwndMenuOwner, hwndMoveSize, hwndCaret; public RECT rcCaret; }
    [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint tid, ref GUITHREADINFO i);
    // Who has the foreground, and which window of the target's own thread has the keyboard focus: "fg=.. active=.. focus=.. focus_class=..".
    // A tree view draws its selection in the focus colour only while it has the focus, so this says which colour a "before" picture is in.
    public static string Focus(long hwnd) {
        uint pid; uint tid = GetWindowThreadProcessId((IntPtr)hwnd, out pid);
        GUITHREADINFO g = new GUITHREADINFO(); g.cbSize = Marshal.SizeOf(g);
        if (!GetGUIThreadInfo(tid, ref g)) return "fg=" + (long)GetForegroundWindow() + " gui=unreadable";
        var c = new StringBuilder(128); if (g.hwndFocus != IntPtr.Zero) GetClassNameW(g.hwndFocus, c, 128);
        return "fg=" + (long)GetForegroundWindow() + " active=" + (long)g.hwndActive + " focus=" + (long)g.hwndFocus + " focus_class=" + c;
    }
}
'@
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 150 }
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

# The window: found by title, as a visible top-level window.
$titles=@{notepad='Notepad$';cmd='ICU4CMD|cmd\.exe';regedit='Registry Editor';services='^Services';taskmgr='Task Manager'}
$cmdFile='C:\Users\Public\g4-cmd-'+$Target+'.txt'
try {
    $found=Wait-For { $w=@([IcuG4]::Windows($titles[$App])); if($w.Count){$w[0]} } 25 "no window titled like $($titles[$App])"
    $hwnd=[int64]($found.Split('|')[0]); $targetPid=[int]($found.Split('|')[1])
    $report.window=@{hwnd=$hwnd;pid=$targetPid;title=$found.Split('|')[2];image=[IcuG4]::Image($targetPid);integrity=('0x{0:x}' -f [IcuG4]::Integrity($targetPid))}
    $report.self_integrity=('0x{0:x}' -f [IcuG4]::Integrity($PID))
    $report.children=@([IcuG4]::Children($hwnd) | Select-Object -First 40)

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
    $sw=[Diagnostics.Stopwatch]::StartNew()
    $script:state=Observe
    $report.get_window_state=@{seconds=[Math]::Round($sw.Elapsed.TotalSeconds,1);exit=$script:stateCall.exit;elements=@($script:state.elements).Count;screenshot_id=$script:state.screenshot_id;
        error=$(if($script:stateCall.exit -ne 0){($script:stateCall.stdout+' '+$script:stateCall.stderr)}else{$null})}
    if(-not $script:state){ throw 'get_window_state failed against this target' }
    $roles=@{}; foreach($e in @($script:state.elements)){ $roles[[string]$e.role]=1+[int]$roles[[string]$e.role] }
    $report.roles=$roles
    $report.elements_sample=@($script:state.elements | Select-Object -First 30 | ForEach-Object {'{0} {1} {2}' -f $_.element_index,$_.role,$_.label})
    $sid=$script:state.screenshot_id
    $origin=[IcuG4]::BitmapOrigin($hwnd)
    function PxOf([int]$sx,[int]$sy) { @(($sx - $origin[0]),($sy - $origin[1])) }
    function Elem([string]$rolePattern,[string]$labelPattern='.') { @($script:state.elements | Where-Object {$_.role -match $rolePattern -and ([string]$_.label) -match $labelPattern})[0] }
    $missing=@{element_token='tk_missing_from_this_snapshot';element_index=0}
    $base=@{pid=$targetPid;window_id=$hwnd;session=$session}
    function A([hashtable]$extra) { $a=$base.Clone(); foreach($k in $extra.Keys){$a[$k]=$extra[$k]}; return $a }
    function ChildOf([string]$class) { foreach($l in [IcuG4]::Children($hwnd)){ $f=$l.Split('|'); if($f[1] -match $class){ return @{hwnd=[int64]$f[0];class=$f[1];rect=@([int]$f[2],[int]$f[3],[int]$f[4],[int]$f[5])} } } return $null }
    function Centre($rect) { @([int](($rect[0]+$rect[2])/2),[int](($rect[1]+$rect[3])/2)) }
    function Region-Shot($rect) { [IcuG4]::Shot($rect[0],$rect[1],[Math]::Max(1,$rect[2]-$rect[0]),[Math]::Max(1,$rect[3]-$rect[1])) }

    # Each action: name, tool, args, bg (a posted/UIA-pattern delivery that Windows denies into SYSTEM windows: expected to
    # refuse there), before (a scriptblock reading the application), landed (a scriptblock of $before and $after).
    $actions=@()
    switch($App){
      'notepad' {
        $edit=ChildOf '^Edit$'
        if(-not $edit){ throw 'notepad has no Edit child' }
        $ec=Centre $edit.rect; $ep=PxOf $ec[0] $ec[1]
        $txt={ [IcuG4]::Text($edit.hwnd) }
        function Has([string]$m){ $s={ param($b,$a) ($a -ne $null) -and ($a -ne $b) -and ([string]$a).Contains($m) }; return $s.GetNewClosure() }
        $menuShot={ Region-Shot @($edit.rect[0],($edit.rect[1]-24),($edit.rect[0]+300),($edit.rect[1]+120)) }
        $l0=PxOf ($edit.rect[0]+4) ($edit.rect[1]+10); $l1=PxOf ($edit.rect[2]-10) ($edit.rect[1]+10); $mn=PxOf ($edit.rect[0]+20) ($edit.rect[1]-10)
        $changedPx={ param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 300 }
        $e=Elem '^(Edit|Document)$'; $tok=$(if($e){$e.element_token}else{$missing.element_token})
        $actions=@(
          @{name='click_px_fg_menu'; tool='click'; args=(A @{x=$mn[0];y=$mn[1];screenshot_id=$sid;delivery_mode='foreground'}); before=$menuShot; landed=$changedPx; after=$menuShot},
          @{name='key_escape_fg'; tool='press_key'; args=(A @{key='escape';delivery_mode='foreground'}); before=$menuShot; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 300}; after=$menuShot},
          @{name='click_px_fg_edit'; tool='click'; args=(A @{x=$ep[0];y=$ep[1];screenshot_id=$sid;delivery_mode='foreground'}); before={0}; after={0}; landed={param($b,$a) $true}},
          @{name='type_noel_fg'; tool='type_text'; args=(A @{text='ICU4A';delivery_mode='foreground'}); before=$txt; landed=(Has 'ICU4A'); after=$txt},
          @{name='type_el_fg'; tool='type_text'; args=(A @{element_token=$tok;text='EL4';delivery_mode='foreground'}); before=$txt; landed=(Has 'EL4'); after=$txt},
          @{name='key_el_fg'; tool='press_key'; args=(A @{element_token=$tok;key='x';delivery_mode='foreground'}); before=$txt; landed=(Has 'x'); after=$txt},
          @{name='type_el_bg'; tool='type_text'; args=(A @{element_token=$tok;text='BG4'}); bg=$true; before=$txt; landed=(Has 'BG4'); after=$txt},
          @{name='setvalue_el'; tool='set_value'; args=(A @{element_token=$tok;value='SV4'}); bg=$true; before=$txt; landed=(Has 'SV4'); after=$txt},
          # a drag across the first line selects it all; the next typed character replaces it
          @{name='drag_px_fg_select'; tool='drag'; args=(A @{from_x=$l0[0];from_y=$l0[1];to_x=$l1[0];to_y=$l1[1];screenshot_id=$sid;delivery_mode='foreground'}); before=$txt; after=$txt;
             landed={param($b,$a) $true}; followup=@{tool='type_text';args=(A @{text='Z';delivery_mode='foreground'});oracle=$txt;landed={param($b,$a) $a -eq 'Z'}}}
        )
      }
      'cmd' {
        Remove-Item $cmdFile -ErrorAction SilentlyContinue
        $file={ if(Test-Path $cmdFile){ (Get-Content $cmdFile -Raw) } else { '' } }
        $actions=@(
          @{name='type_noel_fg'; tool='type_text'; args=(A @{text=('echo ICU4CMD>'+$cmdFile);delivery_mode='foreground'}); before=$file; after={ Start-Sleep -Milliseconds 200; & $file }; landed={param($b,$a) $true}; followup=@{tool='press_key';args=(A @{key='return';delivery_mode='foreground'});oracle=$file;landed={param($b,$a) ($a -is [string]) -and $a.Contains('ICU4CMD')}}}
        )
      }
      'regedit' {
        $tree=ChildOf 'SysTreeView32'; $list=ChildOf 'SysListView32'
        if(-not $tree -or -not $list){ throw 'regedit has no tree or list child' }
        $hive=$(if($Target -eq 'system'){'Registry::HKEY_USERS\.DEFAULT'}else{'HKCU:'})
        $keyPath=$hive+'\ICU4KEY'
        Remove-Item $keyPath -ErrorAction SilentlyContinue
        $tc=Centre $tree.rect; $rowOffset=$(if(Test-Path 'C:\Users\Public\tree-row.txt'){[int](Get-Content 'C:\Users\Public\tree-row.txt' -Raw)}else{12})   # 12 = the root row; a diagnostic run (that file holds 30) clicks the next row to tell a no-op click from a lost one
        $tp=PxOf ($tree.rect[0]+30) ($tree.rect[1]+$rowOffset)
        # How the tree click is judged (C:\Users\Public\tree-mode.txt):
        #   (absent)  the check. The root row is selected when regedit opens and, on some hosts, the tree already has the keyboard
        #             focus, so the click changes nothing a picture can show. The selection is moved off the root first (one Down
        #             key through the driver, shown by a change of the tree's pixels), then the click on the root row must change
        #             the pixels back. A Down key that changes nothing fails the cell as 'prep_failed', it does not pass it.
        #   old       the earlier check: the click alone, no preparation (what failed 6 of 6 on the final-build host).
        #   miss      the check, with the click aimed at the menu bar's empty right end instead of the row: it must go red.
        $treeMode=$(if(Test-Path 'C:\Users\Public\tree-mode.txt'){(Get-Content 'C:\Users\Public\tree-mode.txt' -Raw).Trim()}else{''})
        $report.tree_mode=$treeMode
        if($treeMode -eq 'miss'){ $tp=PxOf ($tree.rect[0]+($list.rect[2]-$tree.rect[0])-40) ($tree.rect[1]-12) }
        $treePrep=$(if($treeMode -eq 'old'){$null}else{ @{tool='press_key';args=(A @{key='down';delivery_mode='foreground'});shot={ Region-Shot $tree.rect };landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 20}} })
        $keyExists={ Test-Path $keyPath }
        $listLeft={ ([IcuG4]::Rect($list.hwnd))[0] }
        $splitter=[int](($tree.rect[2]+$list.rect[0])/2)
        $sp=PxOf $splitter $tc[1]
        $actions=@(
          @{name='click_px_fg_tree'; prep=$treePrep; tool='click'; args=(A @{x=$tp[0];y=$tp[1];screenshot_id=$sid;delivery_mode='foreground'}); before={ Region-Shot $tree.rect }; after={ Region-Shot $tree.rect }; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 20}},
          @{name='key_down_fg_1'; tool='press_key'; args=(A @{key='down';delivery_mode='foreground'}); before={ Region-Shot $tree.rect }; after={ Region-Shot $tree.rect }; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 20}},
          @{name='key_down_fg_2'; tool='press_key'; args=(A @{key='down';delivery_mode='foreground'}); before={ Region-Shot $tree.rect }; after={ Region-Shot $tree.rect }; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 20}},
          # Edit > New > Key through the menu bar's access keys, then the name, then Enter: a registry key appears
          @{name='key_alt_e_fg'; tool='press_key'; args=(A @{key='e';modifiers=@('alt');delivery_mode='foreground'}); before={ Region-Shot @($tree.rect[0],($tree.rect[1]-30),($tree.rect[0]+300),($tree.rect[1]+200)) }; after={ Region-Shot @($tree.rect[0],($tree.rect[1]-30),($tree.rect[0]+300),($tree.rect[1]+200)) }; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 300}},
          @{name='key_n_fg'; tool='press_key'; args=(A @{key='n';delivery_mode='foreground'}); before={ Region-Shot @($tree.rect[0],($tree.rect[1]-30),($tree.rect[0]+500),($tree.rect[1]+200)) }; after={ Region-Shot @($tree.rect[0],($tree.rect[1]-30),($tree.rect[0]+500),($tree.rect[1]+200)) }; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 300}},
          @{name='key_k_fg'; tool='press_key'; args=(A @{key='k';delivery_mode='foreground'}); before={ Region-Shot $tree.rect }; after={ Region-Shot $tree.rect }; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 20}},
          @{name='type_name_fg'; tool='type_text'; args=(A @{text='ICU4KEY';delivery_mode='foreground'}); before={ Region-Shot $tree.rect }; after={ Region-Shot $tree.rect }; landed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 20}},
          @{name='key_return_fg_creates_key'; tool='press_key'; args=(A @{key='return';delivery_mode='foreground'}); before=$keyExists; after={ Start-Sleep -Milliseconds 300; & $keyExists }; landed={param($b,$a) ($b -eq $false) -and ($a -eq $true)}},
          # the splitter between the tree and the list moves 100 px right: the list view's left edge follows
          @{name='drag_px_fg_splitter'; tool='drag'; args=(A @{from_x=$sp[0];from_y=$sp[1];to_x=($sp[0]+100);to_y=$sp[1];screenshot_id=$sid;delivery_mode='foreground'}); before=$listLeft; after={ Start-Sleep -Milliseconds 300; & $listLeft }; landed={param($b,$a) ([Math]::Abs(($a-$b)-100) -le 10)}},
          @{name='drag_px_bg_splitter'; tool='drag'; args=(A @{from_x=($sp[0]+100);from_y=$sp[1];to_x=($sp[0]);to_y=$sp[1];screenshot_id=$sid;delivery_mode='background'}); bg=$true; before=$listLeft; after={ Start-Sleep -Milliseconds 300; & $listLeft }; landed={param($b,$a) ($a -lt $b)}}
        )
      }
      'services' {
        $list=ChildOf 'SysListView32'
        if(-not $list){ $list=@{hwnd=$hwnd;rect=[IcuG4]::Rect($hwnd)} }
        $r=$list.rect; $rowShot={ Region-Shot @($r[0],($r[1]+20),$r[2],($r[1]+260)) }
        $cx=[int](($r[0]+$r[2])/2)
        $pxRow=PxOf ($r[0]+120) ($r[1]+60); $pxMid=PxOf $cx ([int](($r[1]+$r[3])/2))
        $changed={param($b,$a) ([IcuG4]::Diff($b,$a)) -gt 200}
        $actions=@(
          @{name='click_px_fg_row'; tool='click'; args=(A @{x=$pxRow[0];y=$pxRow[1];screenshot_id=$sid;delivery_mode='foreground'}); before=$rowShot; after=$rowShot; landed=$changed},
          @{name='scroll_px_fg'; tool='scroll'; args=(A @{direction='down';amount=5;x=$pxMid[0];y=$pxMid[1];screenshot_id=$sid;delivery_mode='foreground'}); before=$rowShot; after=$rowShot; landed=$changed},
          @{name='key_end_fg'; tool='press_key'; args=(A @{key='end';delivery_mode='foreground'}); before=$rowShot; after=$rowShot; landed=$changed},
          @{name='scroll_px_bg'; tool='scroll'; args=(A @{direction='up';amount=5;x=$pxMid[0];y=$pxMid[1];screenshot_id=$sid;by='line'}); bg=$true; before=$rowShot; after=$rowShot; landed=$changed},
          @{name='click_px_bg_row'; tool='click'; args=(A @{x=$pxRow[0];y=($pxRow[1]+40);screenshot_id=$sid}); bg=$true; before=$rowShot; after=$rowShot; landed=$changed}
        )
      }
      'taskmgr' {
        # Task Manager's graphs and lists redraw by themselves, so pixels prove nothing here. "Fewer details" opens as a small
        # window; its "More details" button (bottom left) makes the window larger, which the window rectangle shows.
        $r=[IcuG4]::Rect($hwnd)
        $size={ $q=[IcuG4]::Rect($hwnd); '{0}x{1}' -f ($q[2]-$q[0]),($q[3]-$q[1]) }
        $grew={param($b,$a) $b -ne $a}
        $more=PxOf ($r[0]+70) ($r[3]-24)
        $actions=@(
          @{name='click_px_fg_more_details'; tool='click'; args=(A @{x=$more[0];y=$more[1];screenshot_id=$sid;delivery_mode='foreground'}); before=$size; after=$size; landed=$grew}
        )
      }
    }

    $rows=@()
    function Said($call) { $said=($call.stdout+' '+$call.stderr); return $said }
    function Is-Refused($call) {
        $said=Said $call
        # "suspected_noop" with an escalation is the driver saying it could not tell the action landed and what to try next
        # (a background key into a window that drops posted keys): an answer that is not silent, so it counts with the refusals.
        return ($call.exit -ne 0) -or (-not $said.TrimStart().StartsWith('{')) -or ($said -match '"status":\s*"refused"') -or ($said -match '"effect":\s*"refused"') -or ($said -match '"effect":\s*"suspected_noop"')
    }
    foreach($a in $actions){
        $prepRow=$null
        if($a.prep){
            $p=$a.prep; $pb=& $p.shot
            $pc=Call $p.tool $p.args
            $pd=(Get-Date).AddSeconds(4)
            do { Start-Sleep -Milliseconds 250; $pa=& $p.shot; $pl=[bool](& $p.landed $pb $pa) } while(-not $pl -and (Get-Date) -lt $pd)
            $prepRow=[ordered]@{tool=$p.tool;landed=$pl;diff=[IcuG4]::Diff($pb,$pa);said=((Said $pc).Substring(0,[Math]::Min(200,(Said $pc).Length)))}
        }
        $focusBefore=[IcuG4]::Focus($hwnd)
        $before=& $a.before
        $call=Call $a.tool $a.args
        # The first foreground request after a High or SYSTEM application starts can be refused with foreground_unavailable
        # (the worker could not make the window the foreground one: "actual foreground HWND 0x0"). That is a refusal, not a
        # wrong delivery; the request is repeated once and both answers are kept.
        $firstAnswer=$null
        if(((Said $call) -match 'foreground_unavailable') -and ($a.args.delivery_mode -eq 'foreground')){ $firstAnswer=(Said $call); Start-Sleep -Seconds 2; $call=Call $a.tool $a.args }
        $deadline=(Get-Date).AddSeconds(4)
        do { Start-Sleep -Milliseconds 250; $after=& $a.after; $landed=[bool](& $a.landed $before $after) } while(-not $landed -and (Get-Date) -lt $deadline)
        $said=Said $call
        $refused=Is-Refused $call
        # A preparation that did not take leaves nothing to judge the action by: the cell fails (an honest red), whatever the action did.
        if($prepRow -and -not $prepRow.landed){ $landed=$false }
        $row=[ordered]@{name=$a.name;tool=$a.tool;prep=$prepRow;prep_failed=[bool]($prepRow -and -not $prepRow.landed);focus_before=$focusBefore;first_answer_was_foreground_unavailable=[bool]$firstAnswer;exit=$call.exit;refused=[bool]$refused;landed=$landed;bg=[bool]$a.bg;uipi=($said -match 'UIPI|Access is denied');
            before=$(if($before -is [byte[]]){'pixels'}else{[string]$before});after=$(if($after -is [byte[]]){'pixels diff '+[IcuG4]::Diff($before,$after)}else{[string]$after});
            said=$said.Substring(0,[Math]::Min(1200,$said.Length))}
        if($a.followup){
            $f=$a.followup; $fb=& $f.oracle
            $fc=Call $f.tool $f.args
            $dl=(Get-Date).AddSeconds(4)
            do { Start-Sleep -Milliseconds 250; $fa=& $f.oracle; $fl=[bool](& $f.landed $fb $fa) } while(-not $fl -and (Get-Date) -lt $dl)
            $row.followup=[ordered]@{tool=$f.tool;landed=$fl;before=[string]$fb;after=[string]$fa;refused=[bool](Is-Refused $fc);said=((Said $fc).Substring(0,[Math]::Min(200,(Said $fc).Length)))}
            $row.landed=$fl
        }
        $rows+=$row
    }
    $report.actions=$rows
    $report.serve_log=$(if(Test-Path $serveErr){((Get-Content $serveErr -Tail 12 | Where-Object {$_ -notmatch 'is available|Update with|Release notes'}) -join ' | ')}else{''})
    $w=Workers
    $report.workers_after=$w.Count
    if($w.Count){$report.worker=@{pid=$w[0].Id;uiaccess=[IcuG4]::UIAccess($w[0].Id);path=$w[0].Path}}
    # What must hold: no action said ok and changed nothing; every non-background action landed; at SYSTEM every
    # background one refused (Windows denies posted messages into a SYSTEM window even to the UIAccess worker).
    $report.silent_ok=@($rows | Where-Object {-not $_.refused -and -not $_.landed} | ForEach-Object {$_.name})
    $report.not_landed=@($rows | Where-Object {-not $_.landed -and -not ($_.bg -and $_.refused)} | ForEach-Object {$_.name})
    $report.bg_wrongly_landed=@($rows | Where-Object {$_.bg -and $Target -eq "system" -and $_.landed} | ForEach-Object {$_.name})
    $report.pass=($report.silent_ok.Count -eq 0) -and ($report.not_landed.Count -eq 0) -and ($report.bg_wrongly_landed.Count -eq 0) -and $(if($Target -eq 'normal'){ $report.workers_after -eq 0 }else{ $report.worker.uiaccess -eq 1 })
} catch {
    $report.error=$_.Exception.Message
    $report.pass=$false
} finally {
    foreach($p in $procs){ try { if($p -and -not $p.HasExited){$p.Kill()} } catch {} }
    foreach($w in (Workers)){ try { Stop-Process -Id $w.Id -Force -ErrorAction Stop } catch {} }
    $json=$report | ConvertTo-Json -Depth 7 -Compress
    if($Out){$json | Set-Content -Encoding UTF8 $Out}
    $json
}
if(-not $report.pass){exit 1}
