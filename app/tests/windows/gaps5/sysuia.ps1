param(
    [Parameter(Mandatory=$true)][string]$Driver,
    [Parameter(Mandatory=$true)][string]$Out,
    [string]$Apps='notepad,cmd,regedit,services,taskmgr'
)
# Runs AS SYSTEM IN THE INTERACTIVE SESSION (sysuia-run.ps1 starts it with the SYSTEM token moved to the console session).
# gaps-windows-4 could only measure UIA clients at normal and High integrity against SYSTEM windows. This is a SYSTEM
# client: for each real application, started here as SYSTEM, it counts
#   raw   a plain UIA FindAll(Descendants) from this process (System.Windows.Automation)
#   daemon the driver itself (served as SYSTEM, so it is the same integrity as the target: no worker, no child-window
#          description) -> get_window_state elements and roles
# and records which integrity this process really has.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type @'
using System; using System.Collections.Generic; using System.Text; using System.Runtime.InteropServices;
public static class IcuSys5 {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint c);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    public static string[] Windows(string re) {
        var rx = new System.Text.RegularExpressions.Regex(re); var l = new List<string>();
        EnumWindows((h, p) => { if (!IsWindowVisible(h) || GetWindow(h, 4) != IntPtr.Zero) return true;
            var sb = new StringBuilder(512); GetWindowTextW(h, sb, 512);
            if (sb.Length > 0 && rx.IsMatch(sb.ToString())) { uint pid; GetWindowThreadProcessId(h, out pid); l.Add((long)h + "|" + pid + "|" + sb); } return true; }, IntPtr.Zero);
        return l.ToArray();
    }
}
'@
$commands=@{ notepad=@('C:\Windows\System32\notepad.exe','','Notepad$'); cmd=@('C:\Windows\System32\cmd.exe','/k title ICU4CMD','ICU4CMD|cmd\.exe');
    regedit=@('C:\Windows\regedit.exe','','Registry Editor'); services=@('C:\Windows\System32\mmc.exe','C:\Windows\System32\services.msc','^Services'); taskmgr=@('C:\Windows\System32\Taskmgr.exe','','Task Manager') }
$report=[ordered]@{whoami=(whoami /user /fo csv /nh);mandatory=(whoami /groups | Select-String 'Mandatory Label').ToString().Trim();session=[Diagnostics.Process]::GetCurrentProcess().SessionId;apps=[ordered]@{}}
$pipe='\\.\pipe\icu-gaps5-sys-'+[guid]::NewGuid().ToString('N')
function Run-Driver([string]$arguments) {
    $info=New-Object Diagnostics.ProcessStartInfo $Driver
    $info.Arguments=$arguments; $info.UseShellExecute=$false; $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true; $info.CreateNoWindow=$true
    $p=[Diagnostics.Process]::Start($info); $o=$p.StandardOutput.ReadToEndAsync(); $e=$p.StandardError.ReadToEnd(); $p.WaitForExit()
    @{exit=$p.ExitCode;stdout=$o.Result.Trim();stderr=$e.Trim()}
}
function Call([string]$tool,$arguments) { $json=$arguments | ConvertTo-Json -Depth 8 -Compress; Run-Driver ('--socket '+$pipe+' call '+$tool+' "'+($json -replace '"','\"')+'"') }
$serve=Start-Process $Driver -ArgumentList 'serve','--no-overlay','--socket',$pipe -WindowStyle Hidden -PassThru
try {
    $d=(Get-Date).AddSeconds(30); while((Call 'list_windows' @{}).exit -ne 0 -and (Get-Date) -lt $d){Start-Sleep -Milliseconds 400}
    foreach($app in $Apps.Split(',')){
        $row=[ordered]@{}
        $proc=$null
        try {
            $exe=$commands[$app][0]; $args2=$commands[$app][1]; $re=$commands[$app][2]
            if($app -eq 'regedit'){ foreach($hive in 'Registry::HKEY_USERS\.DEFAULT'){ Remove-ItemProperty "$hive\Software\Microsoft\Windows\CurrentVersion\Applets\Regedit" -Name LastKey -ErrorAction SilentlyContinue } }
            $proc=if($args2){Start-Process $exe -ArgumentList $args2 -PassThru}else{Start-Process $exe -PassThru}
            $deadline=(Get-Date).AddSeconds(25); $w=$null
            while((Get-Date) -lt $deadline -and -not $w){ Start-Sleep -Milliseconds 400; $w=@([IcuSys5]::Windows($re))[0] }
            if(-not $w){ $row.error='no window'; continue }
            Start-Sleep -Seconds 3
            $hwnd=[int64]($w.Split('|')[0]); $wpid=[int]($w.Split('|')[1])
            $row.title=$w.Split('|')[2]; $row.pid=$wpid
            $ae=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$hwnd)
            $sw=[Diagnostics.Stopwatch]::StartNew()
            $nodes=@($ae.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition))
            $types=@{}; foreach($n in $nodes){ try{ $t=$n.Current.ControlType.ProgrammaticName -replace '^ControlType\.',''; $types[$t]=1+[int]$types[$t] }catch{} }
            $row.raw=[ordered]@{descendants=$nodes.Count;seconds=[Math]::Round($sw.Elapsed.TotalSeconds,1);control_types=$types}
            $sw.Restart()
            $st=Call 'get_window_state' @{pid=$wpid;window_id=$hwnd;include_screenshot=$false;include_accessibility_tree=$true;session='g5sys'}
            $roles=@{}; $count=0; $sample=@()
            if($st.exit -eq 0){ $state=$st.stdout | ConvertFrom-Json; foreach($e in @($state.elements)){ $count++; $roles[[string]$e.role]=1+[int]$roles[[string]$e.role] }; $sample=@($state.elements | Select-Object -First 25 | ForEach-Object {'{0} {1} {2}' -f $_.element_index,$_.role,$_.label}) }
            $row.daemon=[ordered]@{exit=$st.exit;elements=$count;seconds=[Math]::Round($sw.Elapsed.TotalSeconds,1);roles=$roles;sample=$sample;error=$(if($st.exit -ne 0){($st.stdout+' '+$st.stderr).Substring(0,[Math]::Min(400,($st.stdout+' '+$st.stderr).Length))}else{$null})}
        } catch { $row.error=$_.Exception.Message }
        finally {
            if($proc){ try{ Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }catch{} }
            if($row.pid){ try{ Stop-Process -Id $row.pid -Force -ErrorAction SilentlyContinue }catch{} }
            $report.apps[$app]=$row
        }
    }
} finally {
    try{ if(-not $serve.HasExited){$serve.Kill()} }catch{}
    foreach($n in 'conhost'){ }
    ($report | ConvertTo-Json -Depth 8) | Set-Content -Encoding UTF8 $Out
}
