param(
    # The install directory holding cua-driver.exe (and, for the UIAccess case, the signed cua-driver-uia.exe under Program Files).
    [Parameter(Mandatory=$true)][string]$Dir,
    # Which packaged (AppContainer / immersive) app to drive: settings (ms-settings:) or calculator (calculator:).
    [ValidateSet('settings','calculator')][string]$App='settings',
    # none: nothing special. lock: a second process takes the foreground and calls LockSetForegroundWindow(LSFW_LOCK)
    # first (the foreground-lock case the vendor documents for Store apps), then the driver is asked to type.
    [ValidateSet('none','lock')][string]$Case='none',
    [string]$Out=''
)
# Run as the interactive standard user (as-user.ps1). Drives a PACKAGED app (the Settings app is an immersive app hosted
# by ApplicationFrameHost; Calculator is one too) with the real driver, foreground delivery, and reads the text that
# reached its search/entry control through UI Automation, an oracle the driver does not touch.
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
if([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){ throw 'Interactive desktop required' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms
$driver=Join-Path $Dir 'cua-driver.exe'
$pipe='\\.\pipe\icu-store-'+[guid]::NewGuid().ToString('N')
$r=[ordered]@{app=$App;case=$Case;dir=$Dir;user=$env:USERNAME;driver_sha256=(Get-FileHash $driver -Algorithm SHA256).Hash;driver_signature="$((Get-AuthenticodeSignature $driver).Status)"}
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuLock {
    [DllImport("user32.dll")] public static extern bool LockSetForegroundWindow(uint code);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
}
'@
function Fg { $h=[IcuLock]::GetForegroundWindow(); $p=0; [void][IcuLock]::GetWindowThreadProcessId($h,[ref]$p); $s=New-Object Text.StringBuilder 256; $c=New-Object Text.StringBuilder 256; [void][IcuLock]::GetWindowText($h,$s,256); [void][IcuLock]::GetClassName($h,$c,256); @{hwnd=$h.ToInt64();pid=$p;title=$s.ToString();class=$c.ToString()} }
function Run-Driver([string]$arguments) {
    $info=New-Object Diagnostics.ProcessStartInfo $driver; $info.Arguments=$arguments
    $info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.CreateNoWindow=$true
    $p=[Diagnostics.Process]::Start($info); $o=$p.StandardOutput.ReadToEndAsync(); $e=$p.StandardError.ReadToEnd(); $p.WaitForExit()
    @{exit=$p.ExitCode;stdout=$o.Result.Trim();stderr=$e.Trim()}
}
function Call([string]$tool,$a){ $j=$a | ConvertTo-Json -Depth 8 -Compress; Run-Driver ('--socket '+$pipe+' call '+$tool+' "'+($j -replace '"','\"')+'"') }
function Wait-For($pred,[int]$s,[string]$msg){ $d=(Get-Date).AddSeconds($s); while((Get-Date) -lt $d){ $v=& $pred; if($v){return $v}; Start-Sleep -Milliseconds 200 }; throw $msg }
$daemon=$null; $locker=$null
try {
    $uri=@{settings='ms-settings:';calculator='calculator:'}[$App]
    $pkg=@{settings='*immersivecontrolpanel*|*Windows.ImmersiveControlPanel*';calculator='*WindowsCalculator*'}[$App]
    $r.package_present=@(Get-AppxPackage -ErrorAction SilentlyContinue | Where-Object { $_.Name -like ($pkg -split '\|')[0] -or $_.PackageFullName -like ($pkg -split '\|')[0] } | ForEach-Object { $_.PackageFullName })
    Start-Process $uri
    $root=[Windows.Automation.AutomationElement]::RootElement
    $win=Wait-For { foreach($w in $root.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)){ try{ if($w.Current.Name -eq 'Settings' -and $App -eq 'settings'){ return $w }; if($w.Current.Name -match 'Calculator' -and $App -eq 'calculator'){ return $w } }catch{} } } 25 "the $App window did not appear"
    $hwnd=[int64]$win.Current.NativeWindowHandle
    $r.window=@{hwnd=$hwnd;pid=$win.Current.ProcessId;name=$win.Current.Name;class=$win.Current.ClassName}
    $p=0; [void][IcuLock]::GetWindowThreadProcessId([IntPtr]$hwnd,[ref]$p); $r.window_owner_pid_of_top_level=$p
    Start-Sleep -Seconds 2
    # the control that takes text: Settings' search box, the Calculator has none (keys only)
    $edit=$null
    if($App -eq 'settings'){
        $cond=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit)
        $edit=Wait-For { $win.FindFirst([Windows.Automation.TreeScope]::Descendants,$cond) } 15 'no edit control in Settings'
        $r.edit=@{name=$edit.Current.Name;id=$edit.Current.AutomationId}
    }
    # the driver
    $daemon=Start-Process $driver -ArgumentList 'serve','--no-overlay','--socket',$pipe -WindowStyle Hidden -PassThru
    Wait-For { (Call 'list_windows' @{}).exit -eq 0 } 25 'driver did not serve' | Out-Null
    $lw=Call 'list_windows' @{}
    $r.list_windows_excerpt=($lw.stdout -split "`n" | Select-Object -First 3) -join ' '
    $r.driver_sees_window=($lw.stdout -match [regex]::Escape([string]$hwnd))
    if($Case -eq 'lock'){
        # another process owns the foreground and has locked it
        $helper=Join-Path $env:TEMP 'icu-lock.ps1'
        @'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class L {
    [DllImport("user32.dll")] public static extern bool LockSetForegroundWindow(uint c);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
"@
$f=New-Object Windows.Forms.Form; $f.Text='ICU lock holder'; $f.TopMost=$true; $f.Show()
# a process that has not received input cannot take the foreground; an Alt tap counts as input
[L]::keybd_event(0x12,0,0,[UIntPtr]::Zero); [L]::keybd_event(0x12,0,2,[UIntPtr]::Zero)
[void][L]::SetForegroundWindow($f.Handle)
Start-Sleep -Milliseconds 300
$fg=([L]::GetForegroundWindow() -eq $f.Handle)
$ok=[L]::LockSetForegroundWindow(1)
Set-Content $env:TEMP\icu-lock.state "locked=$ok holder_is_foreground=$fg"
$d=(Get-Date).AddSeconds(60); while((Get-Date) -lt $d){ [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 100 }
'@ | Set-Content $helper
        Remove-Item (Join-Path $env:TEMP 'icu-lock.state') -ErrorAction SilentlyContinue
        $locker=Start-Process powershell.exe -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',$helper -PassThru -WindowStyle Hidden
        Wait-For { Test-Path (Join-Path $env:TEMP 'icu-lock.state') } 10 'lock holder did not start' | Out-Null
        $r.lock_state=((Get-Content (Join-Path $env:TEMP 'icu-lock.state')) -join ' ')
        Start-Sleep -Milliseconds 500
    }
    $r.foreground_before=Fg
    if($App -eq 'settings'){
        $c1=Call 'type_text' @{pid=$win.Current.ProcessId;window_id=$hwnd;text='icuprobe';delivery_mode='foreground'}
    } else {
        $c1=Call 'press_key' @{pid=$win.Current.ProcessId;window_id=$hwnd;key='7';delivery_mode='foreground'}
    }
    $r.call=$c1
    Start-Sleep -Milliseconds 1200
    $r.foreground_after=Fg
    if($App -eq 'settings'){
        $vp=$null; $val=''; try{ if($edit.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$vp)){ $val=$vp.Current.Value } }catch{ $val="<unreadable: $($_.Exception.Message)>" }
        $r.edit_value=$val
        $r.landed=($val -match 'icuprobe')
    } else {
        $names=@(); foreach($n in $win.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)){ try{ if($n.Current.AutomationId -eq 'CalculatorResults' -or $n.Current.Name -match 'Display is'){ $names += $n.Current.Name } }catch{} }
        $r.display=$names; $r.landed=[bool]($names -match '7')
    }
    $r.worker_started=@(Get-Process -Name 'cua-driver-uia' -ErrorAction SilentlyContinue | ForEach-Object { @{pid=$_.Id} })
} catch { $r.error=$_.Exception.Message }
finally {
    if($locker -and -not $locker.HasExited){ $locker.Kill() }
    if($daemon -and -not $daemon.HasExited){ $daemon.Kill() }
    foreach($w in @(Get-Process -Name 'cua-driver-uia' -ErrorAction SilentlyContinue)){ try{ Stop-Process -Id $w.Id -Force }catch{} }
    foreach($n in 'SystemSettings','Calculator','CalculatorApp','ApplicationFrameHost'){ Get-Process -Name $n -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq [Diagnostics.Process]::GetCurrentProcess().SessionId } | Stop-Process -Force -ErrorAction SilentlyContinue }
    $json=$r | ConvertTo-Json -Depth 6
    if($Out){ $json | Set-Content -Encoding UTF8 $Out }
    $json
}
