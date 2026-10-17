param(
    [Parameter(Mandatory=$true)][string]$ManifestPath,
    # A driver to run instead of the packaged one (the build under test).
    [string]$DriverOverride='',
    [string]$Out='',
    # Leave the sidecar as it is (no recording stand-in): to tell a tap problem from an app problem.
    [switch]$NoTap
)
# The installed app's tile wheel, driven the way a user does it: a real mouse wheel over a window
# tile. The window the tile shows (BOUND) scrolls; a second window (DECOY) sits on top of it at the
# same screen place and must not move. A recording stand-in for the driver sidecar (PipeTap.cs)
# captures what the app sends: the `scroll` request must carry a screenshot_id the driver issued.
# Run in the interactive session (as-user.ps1).
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms
Add-Type -Path (Join-Path $PSScriptRoot 'NativeObserver.cs')
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuWheel {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, int data, UIntPtr extra);
    public static void Wheel(int notches) { mouse_event(0x0800, 0, 0, notches * 120, UIntPtr.Zero); }
}
'@
$manifest=Get-Content $ManifestPath -Encoding UTF8 | ConvertFrom-Json
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\wheel-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$report=[ordered]@{revision=$manifest.revision}
$app=$null;$fixtures=@();$tapLog=$null;$driver=$null;$realDriver=$null;$appWindow=$null
$csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
function Nodes { [System.Windows.Automation.AutomationElement]::FromHandle($appWindow).FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) }
function Find-Name([string]$text,[bool]$exact=$true) {
    foreach($node in (Nodes)) { try { $name=$node.Current.Name; if(-not $node.Current.IsOffscreen -and (($exact -and $name -eq $text) -or (-not $exact -and $name.Contains($text)))) {return $node} } catch {} }
    return $null
}
function Press($node) {
    for($i=0;$i -lt 8 -and $node;$i++) {
        $pattern=$null
        if($node.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) {$pattern.Invoke();return}
        $node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)
    }
    throw 'No invoke action for the selected control'
}
function Scroll-Top([string]$dir) { $p=Join-Path $dir 'scroll.txt'; if(Test-Path $p){ try { $t=([IO.File]::ReadAllText($p)).Trim(); if($t){return [int]$t} } catch {} }; return -1 }
try {
    $install=Join-Path $env:LOCALAPPDATA 'Programs\Copilot'
    # This user is a member of Administrators (a filtered token), so the installer would ask for elevation; a per-user
    # install needs none, and this session has no secure desktop to answer on.
    $env:__COMPAT_LAYER='RunAsInvoker'
    $installer=Start-Process $manifest.installer -ArgumentList '/S','/CurrentUser' -PassThru
    Remove-Item Env:\__COMPAT_LAYER -ErrorAction SilentlyContinue
    if(-not $installer.WaitForExit(120000)){$installer.Kill();throw 'Installer exceeded two minutes'}
    if($installer.ExitCode -ne 0){throw "Installer failed: $($installer.ExitCode)"}
    $exe=Join-Path $install 'app.exe';$driver=Join-Path $install 'cua-driver.exe';$realDriver=Join-Path $install 'cua-driver-real.exe'
    # A manifest from build.ps1 carries the payload hash; one written by hand (a build whose backend tests failed) may not.
    if($manifest.app_sha256 -and (Get-FileHash $exe -Algorithm SHA256).Hash -ne $manifest.app_sha256){throw 'Installed app is not the manifest build'}
    # The driver under test goes in as cua-driver-real.exe; the tap takes the sidecar's name.
    Copy-Item $(if($DriverOverride){$DriverOverride}else{$driver}) $realDriver -Force
    if(-not $NoTap){
        & $csc /nologo /target:exe ("/out:$driver") (Join-Path $PSScriptRoot 'PipeTap.cs') | Out-Null
        if(-not (Test-Path $driver)){throw 'PipeTap did not compile'}
    } else { Copy-Item $realDriver $driver -Force }
    $report.driver=@{override=[bool]$DriverOverride;sha256=(Get-FileHash $realDriver -Algorithm SHA256).Hash}
    $tapLog=Join-Path $work 'tap.log'
    $env:ICU_TAP_LOG=$tapLog

    # Two fixtures, one over the other. The first started is BOUND; DECOY starts after it, so it is in front.
    & $csc /nologo /target:winexe /reference:System.Windows.Forms.dll /reference:System.Drawing.dll ("/out:"+(Join-Path $work 'fixture.exe')) (Join-Path $PSScriptRoot 'UiaGestureFixture.cs') | Out-Null
    $dirB=Join-Path $work 'bound';$dirD=Join-Path $work 'decoy'
    New-Item -ItemType Directory $dirB,$dirD | Out-Null
    $pB=Start-Process (Join-Path $work 'fixture.exe') -ArgumentList ('"'+$dirB+'"'),'ICU-BOUND' -PassThru;$fixtures+=$pB
    Wait-For {Test-Path (Join-Path $dirB 'target.ready')} 15 'bound fixture did not start' | Out-Null
    $pD=Start-Process (Join-Path $work 'fixture.exe') -ArgumentList ('"'+$dirD+'"'),'ICU-DECOY' -PassThru;$fixtures+=$pD
    Wait-For {Test-Path (Join-Path $dirD 'target.ready')} 15 'decoy fixture did not start' | Out-Null
    $hb=[IntPtr][int64]((Get-Content (Join-Path $dirB 'target.ready')).Split(' ')[0])
    $hd=[IntPtr][int64]((Get-Content (Join-Path $dirD 'target.ready')).Split(' ')[0])
    $screen=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $report.screen=@{width=$screen.Width;height=$screen.Height}
    # Both fixtures at the same place: the decoy covers the bound window.
    foreach($h in @($hb,$hd)){ [NativeObserver]::SetWindowPos($h,[IntPtr]::Zero,10,10,520,520,0x0044) | Out-Null }
    $layoutB=(Get-Content (Join-Path $dirB 'layout.json') -Raw) | ConvertFrom-Json
    $report.fixtures=@{bound=$hb.ToInt64();decoy=$hd.ToInt64();layout=$layoutB}

    $env:ICU_BRAIN='ollama'
    $app=Start-Process $exe -PassThru
    $appWindow=Wait-For {$w=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($w -ne [IntPtr]::Zero){$w}} 20 'App window did not appear'
    $appLeft=545;$appWidth=[Math]::Max(700,$screen.Width-$appLeft-5)
    [NativeObserver]::SetWindowPos($appWindow,[IntPtr]::Zero,$appLeft,0,$appWidth,[Math]::Min($screen.Height-40,760),0x0044) | Out-Null
    Start-Sleep -Seconds 2
    # The picker and the tile come up asynchronously: try the pick up to three times, each time waiting for the
    # tile's title text to appear.
    function Tile-Label { foreach($node in (Nodes)) { try { if(-not $node.Current.IsOffscreen -and $node.Current.Name -eq 'ICU-BOUND' -and $node.Current.ControlType.ProgrammaticName -eq 'ControlType.Text'){return $node} } catch {} }; return $null }
    $report.pick_attempts=0
    for($attempt=1;$attempt -le 3 -and -not (Tile-Label);$attempt++){
        $report.pick_attempts=$attempt
        try {
            Press (Wait-For {Find-Name '+ window'} 30 'Window picker unavailable')
            Press (Wait-For {Find-Name 'ICU-BOUND' $false} 15 'ICU-BOUND missing from the picker')
        } catch { $report.pick_error=$_.Exception.Message }
        $deadline=(Get-Date).AddSeconds(12)
        while(-not (Tile-Label) -and (Get-Date) -lt $deadline){ Start-Sleep -Milliseconds 300 }
    }
    Start-Sleep -Seconds 3
    $report.nodes=@(Nodes | ForEach-Object { try { '{0}|{1}|{2}' -f $_.Current.ControlType.ProgrammaticName,$_.Current.Name,$_.Current.BoundingRectangle } catch {} } | Where-Object {$_} | Select-Object -First 80)
    # The tile is a group holding the window's title text; its picture has no accessible name. The picture sits
    # under the title row and is as wide as the group (the window is square), so a point 65% down it is over
    # the bound window's list.
    $label=$null
    foreach($node in (Nodes)) { try { if(-not $node.Current.IsOffscreen -and $node.Current.Name -eq 'ICU-BOUND' -and $node.Current.ControlType.ProgrammaticName -eq 'ControlType.Text'){$label=$node;break} } catch {} }
    if(-not $label){ throw 'no tile label for ICU-BOUND in the app (see nodes)' }
    $lr=$label.Current.BoundingRectangle
    $tile=$null;$area=[double]::MaxValue
    foreach($node in (Nodes)) { try {
        $r=$node.Current.BoundingRectangle
        if($node.Current.ControlType.ProgrammaticName -eq 'ControlType.Group' -and $r.Width -gt 250 -and $r.Left -le $lr.Left -and $r.Right -ge $lr.Right -and $r.Top -le $lr.Top -and $r.Bottom -ge $lr.Bottom -and $r.Width*$r.Height -lt $area){$tile=$node;$area=$r.Width*$r.Height}
    } catch {} }
    if(-not $tile){ throw 'no tile group around the ICU-BOUND label (see nodes)' }
    $rect=$tile.Current.BoundingRectangle
    $imgTop=$lr.Bottom+8;$imgW=$rect.Width-10
    $report.tile=@{x=$rect.X;y=$rect.Y;w=$rect.Width;h=$rect.Height;label_bottom=$lr.Bottom}
    $wx=[int]($rect.X+$rect.Width/2);$wy=[int]($imgTop+0.65*$imgW)
    $report.wheel_at=@{x=$wx;y=$wy}
    $before=@{bound=(Scroll-Top $dirB);decoy=(Scroll-Top $dirD)}
    [IcuWheel]::SetCursorPos($wx,$wy) | Out-Null
    Start-Sleep -Milliseconds 400
    for($i=0;$i -lt 4;$i++){ [IcuWheel]::Wheel(-1); Start-Sleep -Milliseconds 250 }
    Start-Sleep -Seconds 2
    $after=@{bound=(Scroll-Top $dirB);decoy=(Scroll-Top $dirD)}
    $report.scroll=@{before=$before;after=$after}
    $lines=@(if(Test-Path $tapLog){Get-Content $tapLog -Encoding UTF8})
    $scrollReq=@($lines | Where-Object {$_ -match ' C> ' -and $_ -match '"scroll"'})
    $report.scroll_requests=@($scrollReq | Select-Object -First 3 | ForEach-Object { $_.Substring(0,[Math]::Min(500,$_.Length)) })
    $report.scroll_request_count=$scrollReq.Count
    $id=$null
    if($scrollReq.Count -gt 0 -and $scrollReq[0] -match '"screenshot_id"\s*:\s*"([^"]+)"'){ $id=$Matches[1] }
    $report.screenshot_id=$id
    $issued=$false
    if($id){ $first=[Array]::IndexOf($lines,$scrollReq[0]); $issued=@($lines[0..([Math]::Max(0,$first-1))] | Where-Object {$_ -match ' S> ' -and $_.Contains($id)}).Count -gt 0 }
    $report.id_issued_by_driver_before_the_request=$issued
    $report.scroll_replies=@($lines | Where-Object {$_ -match ' S> ' -and ($_ -match 'Scrolled' -or $_ -match 'background_unavailable')} | Select-Object -First 3 | ForEach-Object { $_.Substring(0,[Math]::Min(420,$_.Length)) })
    $replies=@($lines | Where-Object {$_ -match ' S> '} | Select-Object -Last 2 | ForEach-Object { $_.Substring(0,[Math]::Min(300,$_.Length)) })
    $report.last_replies=$replies
    $report.bound_scrolled=($after.bound -gt $before.bound)
    $report.decoy_unmoved=($after.decoy -eq $before.decoy)
    $report.ok=($report.bound_scrolled -and $report.decoy_unmoved -and [bool]$id -and $issued)
} catch {
    $report.ok=$false;$report.error=$_.Exception.Message
    try { if($appWindow){ $report.nodes_on_error=@(Nodes | ForEach-Object { try { $_.Current.Name } catch {} } | Where-Object {$_} | Select-Object -First 60) } } catch {}
    try { if($tapLog -and (Test-Path $tapLog)){ $report.tap_tail=@(Get-Content $tapLog | Where-Object {$_ -notmatch 'client connected|real driver connected'} | Select-Object -Last 40 | ForEach-Object { $_.Substring(0,[Math]::Min(300,$_.Length)) }) } } catch {}
} finally {
    if($app -and -not $app.HasExited){if($appWindow){[NativeObserver]::PostClose($appWindow) | Out-Null};if(-not $app.WaitForExit(20000)){$app.Kill()}}
    foreach($p in $fixtures){ try { if(-not $p.HasExited){$p.Kill()} } catch {} }
    # Put the packaged driver back.
    if($realDriver -and $driver -and (Test-Path $realDriver) -and (Test-Path $driver)){ try { Copy-Item $realDriver $driver -Force; Remove-Item $realDriver -Force } catch {} }
    $json=$report | ConvertTo-Json -Depth 8
    $json | Set-Content (Join-Path $work 'result.json') -Encoding UTF8
    if($Out){ $json | Set-Content $Out -Encoding UTF8 }
    $json
}
if(-not $report.ok){exit 1}
