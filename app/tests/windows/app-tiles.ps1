param([Parameter(Mandatory=$true)][string]$ManifestPath,[string]$DriverOverride='')
# What the installed app's live tiles show on a multi-monitor, mixed-DPI
# desktop: the display tile and window tiles for fixtures on scaled monitors.
# Observation only; no model and no input to the fixtures.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'Interactive desktop required' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'NativeObserver.cs')
$manifest=Get-Content $ManifestPath -Encoding UTF8 | ConvertFrom-Json
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\tiles-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$report=@{revision=$manifest.revision};$app=$null;$fixtures=@()
$csc='C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$refs=@('/reference:System.Windows.Forms.dll','/reference:System.Drawing.dll','/reference:System.Web.Extensions.dll')
function Wait-For($predicate,[int]$seconds,[string]$message) {
    $deadline=(Get-Date).AddSeconds($seconds)
    while((Get-Date) -lt $deadline) { $value=& $predicate; if($value){return $value};Start-Sleep -Milliseconds 100 }
    throw $message
}
function Tool { $output=& $layout @args; if($LASTEXITCODE){throw "layout tool failed: $args"}; return $output | ConvertFrom-Json | ForEach-Object {$_} }
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
try {
    # Reinstall: an earlier -DriverOverride run may have left another sidecar.
    # installMode "both": a silent install ignores /D=, so install per user to the product's default folder.
    $install=Join-Path $env:LOCALAPPDATA 'Programs\Copilot'
    $installer=Start-Process $manifest.installer -ArgumentList '/S','/CurrentUser' -PassThru
    if(-not $installer.WaitForExit(120000)){$installer.Kill();throw 'Installer exceeded two minutes'}
    if($installer.ExitCode -ne 0){throw "Installer failed: $($installer.ExitCode)"}
    $exe=Join-Path $install 'app.exe';$driver=Join-Path $install 'cua-driver.exe'
    if((Get-FileHash $exe -Algorithm SHA256).Hash -ne $manifest.app_sha256){throw 'Installed app is not the manifest build'}
    if((Get-FileHash $driver -Algorithm SHA256).Hash -ne $manifest.driver_sha256){throw 'Installed driver is not the manifest build'}
    if($DriverOverride){Copy-Item $DriverOverride $driver -Force}
    $report.driver=@{override=[bool]$DriverOverride;packaged_sha256=$manifest.driver_sha256;sha256=(Get-FileHash $driver -Algorithm SHA256).Hash}
    $layout=Join-Path $work 'ICUDisplayLayout.exe'
    & $csc /nologo /target:exe /main:DisplayLayout @refs ("/out:"+$layout) (Join-Path $PSScriptRoot 'DisplayLayout.cs') (Join-Path $PSScriptRoot 'DpiFixture.cs')
    & $csc /nologo /target:winexe @refs ("/out:"+(Join-Path $work 'ICUDpiFixture.exe')) (Join-Path $PSScriptRoot 'DpiFixture.cs')
    $monitors=@(Tool list);$report.monitors=$monitors
    $scaled=@($monitors | Where-Object {-not $_.primary} | Sort-Object dpi -Descending)
    foreach($f in @(@('unaware','tile-unaware',$scaled[0]),@('pmv2','tile-pmv2',$scaled[-1]))){
        $p=Start-Process (Join-Path $work 'ICUDpiFixture.exe') -ArgumentList $f[0],$f[1],'100','100',$work -PassThru;$fixtures+=$p
        Wait-For {Test-Path (Join-Path $work "dpi-ready-$($f[1]).json")} 10 'fixture did not start' | Out-Null
        $ready=Get-Content (Join-Path $work "dpi-ready-$($f[1]).json") | ConvertFrom-Json
        $report[$f[1]]=Tool move $ready.window ($f[2].left+100) ($f[2].top+100)
    }
    $env:ICU_BRAIN='ollama'
    $app=Start-Process $exe -PassThru
    # By class and size, never MainWindowHandle (see NativeObserver.FindTopLevel).
    $appWindow=Wait-For {$w=[NativeObserver]::FindTopLevel($app.Id,'Tauri Window',200);if($w -ne [IntPtr]::Zero){$w}} 15 'App window did not appear'
    Tool move $appWindow 0 0 1000 740 | Out-Null
    foreach($pick in @('Main display','ICU DPI tile-unaware','ICU DPI tile-pmv2')){
        Press (Wait-For {Find-Name '+ window'} 20 'Window picker unavailable')
        Press (Wait-For {Find-Name $pick $false} 10 "$pick missing from picker")
        Start-Sleep -Milliseconds 500
    }
    Press (Wait-For {Find-Name '+ window'} 5 'picker')
    Start-Sleep -Milliseconds 800
    $report.picker=@(Nodes | ForEach-Object {$_.Current.Name} | Where-Object {$_})
    Press (Find-Name '+ window')
    Start-Sleep -Seconds 5
    $report.nodes=@(Nodes | ForEach-Object {$_.Current.Name} | Where-Object {$_})
    $report.app_window=Tool window $appWindow
    $report.shot=Tool shot (Join-Path $work 'virtual-desktop.png')
    $report.ok=$true
} catch {
    $report.ok=$false;$report.error=$_.Exception.Message
} finally {
    if($app -and -not $app.HasExited){if($appWindow){[NativeObserver]::PostClose($appWindow) | Out-Null};if(-not $app.WaitForExit(20000)){$app.Kill()}}
    foreach($p in $fixtures){if(-not $p.HasExited){$p.Kill()}}
    $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $work 'result.json') -Encoding UTF8
    @{work=$work;ok=$report.ok;error=$report.error} | ConvertTo-Json | Set-Content (Join-Path $env:LOCALAPPDATA 'ICUAudit\latest.json') -Encoding UTF8
}
if(-not $report.ok){exit 1}
