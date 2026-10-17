param([ValidateSet('old','new')][string]$Selector='new',[int]$Runs=8,[string]$Exe="$env:LOCALAPPDATA\Programs\Copilot\app.exe",[switch]$Load,[int]$BudgetSeconds=0)
# Launch the installed app repeatedly and apply acceptance.ps1's three window
# operations to the window a selector picks:
#   old: the first non-zero Process.MainWindowHandle seen by a 100 ms Refresh()
#        loop (acceptance.ps1 before this change);
#   new: NativeObserver.FindTopLevel(pid, 'Tauri Window', 200).
# Per launch it records the picked class and the three symptoms of a wrong
# pick: SetWindowPos leaves the Copilot window at its default size, UIA finds
# no descendants, WM_CLOSE does not end the app within 20 s.
# -Load runs two busy loops during the launches (the review saw the race lost
# under build load). Each loop exits on its own once this script's process is
# gone, so a run killed by its task time limit leaves no burner behind.
# -BudgetSeconds stops launching when the slowest run so far would not fit in
# what is left, so the script ends (and writes result.json) before the task
# limit that runs it. Under -Load on m7i-flex.large one run took about three
# minutes. A run that completes fewer runs than asked exits 1.
$ErrorActionPreference='Stop'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'NativeObserver.cs')
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class LaunchRect {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l,t,r,b; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr w,out RECT r);
    public static string Of(IntPtr w){ RECT r; if(!GetWindowRect(w,out r)) return ""; return (r.r-r.l)+"x"+(r.b-r.t); }
}
'@
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\launchrace-'+$Selector+'-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$rows=@();$burners=@()
# The app's whole process tree (the WebView2 browser and its renderers are
# grandchildren), read before the app is closed.
function Tree([int]$root) {
    $all=@(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId)
    $found=@();$queue=@($root)
    while($queue.Count){
        $next=@()
        foreach($id in $queue){ foreach($child in @($all | Where-Object {$_.ParentProcessId -eq $id})){ $found+=$child.ProcessId;$next+=$child.ProcessId } }
        $queue=$next
    }
    return $found
}
function Wait-Gone {
    # WebView2 helpers can outlive the app and hold its profile. They are
    # given 30 s, then stopped (this host runs no other WebView2 app); the
    # count is recorded on the next row.
    $script:stoppedHelpers=0
    $deadline=(Get-Date).AddSeconds(30)
    while($true){
        $left=@(Get-Process -ErrorAction SilentlyContinue | Where-Object {$_.Name -eq 'msedgewebview2' -or ($_.Path -and ($_.Path -eq $Exe -or $_.Path -eq (Join-Path (Split-Path $Exe) 'cua-driver.exe')))})
        if(-not $left.Count){return}
        if((Get-Date) -ge $deadline){
            $script:stoppedHelpers=$left.Count
            $left | Stop-Process -Force -ErrorAction SilentlyContinue
            Start-Sleep -Seconds 2
            return
        }
        Start-Sleep -Milliseconds 500
    }
}
try {
    if($Load){
        $burn='while(Get-Process -Id '+$PID+' -ErrorAction SilentlyContinue){for($i=0;$i -lt 2000000;$i++){}}'
        foreach($i in 1..2){ $burners+=Start-Process powershell.exe -ArgumentList '-NoProfile','-Command',$burn -WindowStyle Hidden -PassThru }
    }
    $began=Get-Date;$slowest=0
    for($i=1;$i -le $Runs;$i++){
        $elapsed=((Get-Date)-$began).TotalSeconds
        if($BudgetSeconds -and $elapsed+$slowest -gt $BudgetSeconds){$budgetStop=$true;break}
        $runStart=Get-Date
        # Start each launch from a clean slate: the previous app and its driver
        # must be gone, or the next launch races their shutdown.
        Wait-Gone
        $app=Start-Process $Exe -PassThru
        $start=Get-Date;$picked=[IntPtr]::Zero
        while(((Get-Date)-$start).TotalSeconds -lt 15){
            if($Selector -eq 'old'){ $app.Refresh(); if($app.MainWindowHandle -ne 0){$picked=$app.MainWindowHandle;break} }
            else { $w=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200); if($w -ne [IntPtr]::Zero){$picked=$w;break} }
            Start-Sleep -Milliseconds 100
        }
        $row=@{run=$i;selector=$Selector;load=[bool]$Load;picked=$picked.ToInt64();picked_class=[NativeObserver]::ClassOf($picked);picked_after_s=[Math]::Round(((Get-Date)-$start).TotalSeconds,2)
            windows_at_pick=@([NativeObserver]::TopLevels($app.Id));stopped_leftover_processes=$script:stoppedHelpers}
        # Let the webview finish loading, as acceptance does before its UIA waits.
        Start-Sleep -Seconds 6
        $row.windows_at_6s=@([NativeObserver]::TopLevels($app.Id))
        $copilot=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200)
        $row.copilot_size_before=[LaunchRect]::Of($copilot)
        $row.setwindowpos=[NativeObserver]::SetWindowPos($picked,[IntPtr]::Zero,0,0,1004,748,0x14)
        Start-Sleep -Milliseconds 500
        $row.copilot_size_after=[LaunchRect]::Of($copilot)
        try { $row.uia_descendants=[System.Windows.Automation.AutomationElement]::FromHandle($picked).FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition).Count } catch { $row.uia_descendants=-1;$row.uia_error=$_.Exception.Message }
        $tree=@(Tree $app.Id)
        [NativeObserver]::PostClose($picked) | Out-Null
        $row.exited_after_close=$app.WaitForExit(20000)
        $row.symptoms=@()
        if($row.copilot_size_after -ne '1004x748'){$row.symptoms+='not_resized'}
        if($row.uia_descendants -le 0){$row.symptoms+='no_uia_nodes'}
        if(-not $row.exited_after_close){$row.symptoms+='did_not_close'}
        # Stop what the app left behind now, so the next launch does not wait
        # 30 s for each run's WebView2 helpers (under -Load that pushed eight
        # runs past the task's time limit).
        $tree=@($tree+@(Tree $app.Id) | Select-Object -Unique)
        if(-not $app.HasExited){$app.Kill();$app.WaitForExit(10000)|Out-Null}
        foreach($id in $tree){ Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
        $row.stopped_tree=$tree.Count
        $rows+=$row
        $row | ConvertTo-Json -Compress | Add-Content (Join-Path $work 'rows.jsonl') -Encoding UTF8
        Start-Sleep -Seconds 2
        $slowest=[Math]::Max($slowest,((Get-Date)-$runStart).TotalSeconds+30)
    }
} finally {
    foreach($b in $burners){ if(-not $b.HasExited){$b.Kill()} }
    $summary=@{selector=$Selector;runs=$rows.Count;runs_requested=$Runs;stopped_for_budget=[bool]$budgetStop;budget_seconds=$BudgetSeconds;load=[bool]$Load
        picked_event_target=@($rows | Where-Object {$_.picked_class -eq 'Tao Thread Event Target'}).Count
        runs_with_symptoms=@($rows | Where-Object {$_.symptoms.Count}).Count;work=$work}
    $summary | ConvertTo-Json | Set-Content (Join-Path $work 'result.json') -Encoding UTF8
    @{work=$work;ok=($summary.runs_with_symptoms -eq 0 -and $summary.runs -eq $Runs);summary=$summary} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $env:LOCALAPPDATA 'ICUAudit\latest.json') -Encoding UTF8
}
if($summary.runs_with_symptoms -or $summary.runs -ne $Runs){exit 1}
